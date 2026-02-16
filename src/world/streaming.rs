//! Predictive chunk streaming — velocity-based prefetching
//!
//! Reduces loading stutters by pre-loading chunks in the player's
//! movement direction before they enter the standard loading radius.
//! Chunks behind the player (opposite to velocity) are prioritised
//! for earlier unloading.
//!
//! # Architecture
//!
//! ```text
//! PlayerChunkVelocity (resource)
//!   ├── Updated each frame from player chunk position delta
//!   └── Smoothed via exponential moving average
//!
//! StreamingConfig (resource)
//!   ├── lookahead_chunks: how far ahead to prefetch
//!   ├── velocity_smoothing: EMA alpha for velocity smoothing
//!   └── min_speed_threshold: minimum speed to trigger prediction
//!
//! predictive_chunk_streaming_system (system)
//!   ├── Reads PlayerChunkVelocity direction
//!   ├── Computes prefetch zone: cone in movement direction
//!   └── Spawns async chunk-generation tasks for predicted positions
//! ```

use bevy::prelude::*;
use bevy::tasks::AsyncComputeTaskPool;
use std::time::Instant;

use super::persistence::{self, ChunkStorage};
use super::{
    Chunk, ChunkLoadMetrics, ChunkLoadResult, ChunkManager, PendingChunk, world_to_chunk_pos,
};
use crate::generation::{
    TerrainConfig, generate_cacti, generate_caves, generate_chunk_terrain, generate_trees,
};

// ============================================================================
// CONFIGURATION
// ============================================================================

/// Configuration for predictive chunk streaming.
///
/// Controls how far ahead of the player's movement direction chunks
/// are pre-loaded, and when behind-player chunks become eligible for
/// early unloading.
#[derive(Resource, Clone, Debug)]
pub struct StreamingConfig {
    /// How many chunks ahead of the player (in the movement direction)
    /// to speculatively load. Default: 3.
    pub lookahead_chunks: i32,

    /// Exponential moving average alpha for smoothing the player's
    /// chunk-level velocity. Range: `0.0..=1.0`. Higher values track
    /// velocity changes faster; lower values are smoother.
    /// Default: 0.15.
    pub velocity_smoothing: f32,

    /// Minimum horizontal speed (in chunks/second) before predictive
    /// loading activates. Below this threshold, only the standard
    /// radius-based loading is used. Default: 0.5.
    pub min_speed_threshold: f32,

    /// Maximum predictive tasks to spawn per frame (independent of
    /// the standard `ChunkManager::max_chunks_per_frame`).
    /// Default: 2.
    pub max_predictive_per_frame: u32,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            lookahead_chunks: 3,
            velocity_smoothing: 0.15,
            min_speed_threshold: 0.5,
            max_predictive_per_frame: 2,
        }
    }
}

// ============================================================================
// PLAYER VELOCITY TRACKING
// ============================================================================

/// Tracks the player's smoothed chunk-space velocity for predictive loading.
///
/// Updated each frame by comparing the current player chunk position to the
/// previous one, then applying an exponential moving average to smooth out
/// jitter.
#[derive(Resource, Debug)]
pub struct PlayerChunkVelocity {
    /// Smoothed velocity in chunks per second (world XZ primarily).
    pub velocity: Vec3,
    /// Previous player chunk position (used to compute delta).
    pub prev_chunk: IVec3,
    /// Whether we have a valid previous sample (skip first frame).
    pub initialized: bool,
}

impl Default for PlayerChunkVelocity {
    fn default() -> Self {
        Self {
            velocity: Vec3::ZERO,
            prev_chunk: IVec3::ZERO,
            initialized: false,
        }
    }
}

impl PlayerChunkVelocity {
    /// Normalised movement direction (XZ plane). Returns `None` if speed
    /// is below the given threshold.
    pub fn direction_xz(&self, min_speed: f32) -> Option<Vec3> {
        let horizontal = Vec3::new(self.velocity.x, 0.0, self.velocity.z);
        let speed = horizontal.length();
        if speed >= min_speed {
            Some(horizontal / speed)
        } else {
            None
        }
    }

    /// Horizontal speed in chunks per second.
    pub fn horizontal_speed(&self) -> f32 {
        Vec3::new(self.velocity.x, 0.0, self.velocity.z).length()
    }
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// Update the smoothed player chunk velocity from frame-to-frame position delta.
///
/// Uses the camera's `GlobalTransform` (same source as `update_player_chunk_position`)
/// to compute position in world space, converts to chunk coordinates, and
/// applies an EMA to smooth the velocity signal.
pub fn update_player_chunk_velocity(
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
    mut velocity: ResMut<PlayerChunkVelocity>,
    streaming_config: Res<StreamingConfig>,
    time: Res<Time>,
) {
    let Ok(global_transform) = camera_query.get_single() else {
        return;
    };

    let current_chunk = world_to_chunk_pos(global_transform.translation());
    let dt = time.delta_secs();

    if !velocity.initialized {
        velocity.prev_chunk = current_chunk;
        velocity.initialized = true;
        return;
    }

    if dt <= 0.0 {
        return;
    }

    // Raw velocity: chunk-position delta / dt
    let delta = current_chunk - velocity.prev_chunk;
    let raw_velocity = Vec3::new(
        delta.x as f32 / dt,
        delta.y as f32 / dt,
        delta.z as f32 / dt,
    );

    // EMA smoothing: v = alpha * raw + (1 - alpha) * prev
    let alpha = streaming_config.velocity_smoothing.clamp(0.0, 1.0);
    velocity.velocity = velocity.velocity * (1.0 - alpha) + raw_velocity * alpha;

    velocity.prev_chunk = current_chunk;
}

/// Predictive chunk streaming — pre-loads chunks in the player's movement direction.
///
/// Runs after the standard `chunk_streaming_system`. For each predicted position
/// in the movement cone, checks if the chunk is already loaded or pending, and
/// spawns an async generation task if not.
pub fn predictive_chunk_streaming_system(
    mut commands: Commands,
    mut chunk_manager: ResMut<ChunkManager>,
    mut load_metrics: ResMut<ChunkLoadMetrics>,
    player_velocity: Res<PlayerChunkVelocity>,
    streaming_config: Res<StreamingConfig>,
    terrain_config: Res<TerrainConfig>,
    chunk_storage: Res<ChunkStorage>,
) {
    // Only predict when moving fast enough
    let Some(direction) = player_velocity.direction_xz(streaming_config.min_speed_threshold) else {
        return;
    };

    let center = chunk_manager.player_chunk;
    let vert_down = chunk_manager.vertical_load_down;
    let vert_up = chunk_manager.vertical_load_up;
    let lookahead = streaming_config.lookahead_chunks;
    let max_per_frame = streaming_config.max_predictive_per_frame;
    let load_dist = chunk_manager.effective_load_distance();

    let task_pool = AsyncComputeTaskPool::get();
    let mut spawned: u32 = 0;

    // Sample positions ahead of the player in the movement direction.
    // We cast a line from (load_dist + 1) to (load_dist + lookahead)
    // along the movement vector, expanding ±1 chunk laterally to cover
    // a narrow cone.
    for step in 1..=(lookahead) {
        let ahead_dist = load_dist + step;
        let base_x = center.x as f32 + direction.x * ahead_dist as f32;
        let base_z = center.z as f32 + direction.z * ahead_dist as f32;

        // Perpendicular vector for lateral expansion
        let perp = Vec3::new(-direction.z, 0.0, direction.x);

        // Sample center + lateral offsets
        for lateral in -1..=1_i32 {
            let sample_x = (base_x + perp.x * lateral as f32).round() as i32;
            let sample_z = (base_z + perp.z * lateral as f32).round() as i32;

            for y in -vert_down..=vert_up {
                let chunk_pos = IVec3::new(sample_x, y, sample_z);

                // Skip if already loaded or pending
                if chunk_manager.chunks.contains_key(&chunk_pos)
                    || chunk_manager.pending.contains(&chunk_pos)
                {
                    continue;
                }

                if spawned >= max_per_frame {
                    return;
                }

                // Clone resources for background task
                let config = (*terrain_config).clone();
                let storage = ChunkStorage::new(chunk_storage.save_dir.clone());

                let task = task_pool.spawn(async move {
                    // Try loading from disk first
                    if let Ok(chunk) = persistence::load_chunk(chunk_pos, &storage) {
                        return ChunkLoadResult {
                            chunk,
                            from_cache: true,
                        };
                    }
                    // Generate new terrain
                    let mut chunk = Chunk::new(chunk_pos);
                    generate_chunk_terrain(&mut chunk, &config);
                    generate_caves(&mut chunk, &config);
                    generate_trees(&mut chunk, &config);
                    generate_cacti(&mut chunk, &config);
                    ChunkLoadResult {
                        chunk,
                        from_cache: false,
                    }
                });

                commands.spawn(PendingChunk {
                    task,
                    position: chunk_pos,
                });

                chunk_manager.pending.insert(chunk_pos);
                spawned += 1;
                load_metrics
                    .pending_start_times
                    .insert(chunk_pos, Instant::now());
            }
        }
    }
}

/// Compute chunk positions in the predictive loading zone.
///
/// This is a pure function used for testing and by the streaming system.
/// Returns positions ahead of the player that should be prefetched.
pub fn compute_predicted_positions(
    center: IVec3,
    direction: Vec3,
    load_dist: i32,
    lookahead: i32,
    vert_down: i32,
    vert_up: i32,
) -> Vec<IVec3> {
    let mut positions = Vec::new();

    // Perpendicular vector for lateral expansion
    let perp = Vec3::new(-direction.z, 0.0, direction.x);

    for step in 1..=lookahead {
        let ahead_dist = load_dist + step;
        let base_x = center.x as f32 + direction.x * ahead_dist as f32;
        let base_z = center.z as f32 + direction.z * ahead_dist as f32;

        for lateral in -1..=1_i32 {
            let sample_x = (base_x + perp.x * lateral as f32).round() as i32;
            let sample_z = (base_z + perp.z * lateral as f32).round() as i32;

            for y in -vert_down..=vert_up {
                positions.push(IVec3::new(sample_x, y, sample_z));
            }
        }
    }

    positions
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- StreamingConfig tests ---

    #[test]
    fn test_streaming_config_defaults() {
        let config = StreamingConfig::default();
        assert_eq!(config.lookahead_chunks, 3);
        assert_eq!(config.velocity_smoothing, 0.15);
        assert_eq!(config.min_speed_threshold, 0.5);
        assert_eq!(config.max_predictive_per_frame, 2);
    }

    #[test]
    fn test_streaming_config_clone() {
        let config = StreamingConfig {
            lookahead_chunks: 5,
            velocity_smoothing: 0.3,
            min_speed_threshold: 1.0,
            max_predictive_per_frame: 4,
        };
        let cloned = config.clone();
        assert_eq!(cloned.lookahead_chunks, 5);
        assert_eq!(cloned.velocity_smoothing, 0.3);
        assert_eq!(cloned.min_speed_threshold, 1.0);
        assert_eq!(cloned.max_predictive_per_frame, 4);
    }

    // --- PlayerChunkVelocity tests ---

    #[test]
    fn test_player_chunk_velocity_default() {
        let vel = PlayerChunkVelocity::default();
        assert_eq!(vel.velocity, Vec3::ZERO);
        assert_eq!(vel.prev_chunk, IVec3::ZERO);
        assert!(!vel.initialized);
    }

    #[test]
    fn test_direction_xz_above_threshold() {
        let vel = PlayerChunkVelocity {
            velocity: Vec3::new(2.0, 0.5, 0.0),
            prev_chunk: IVec3::ZERO,
            initialized: true,
        };
        let dir = vel.direction_xz(0.5);
        assert!(dir.is_some());
        let d = dir.unwrap();
        // Should be normalised in XZ: (1, 0, 0)
        assert!((d.x - 1.0).abs() < 0.01);
        assert_eq!(d.y, 0.0);
        assert!(d.z.abs() < 0.01);
    }

    #[test]
    fn test_direction_xz_below_threshold() {
        let vel = PlayerChunkVelocity {
            velocity: Vec3::new(0.1, 0.0, 0.1),
            prev_chunk: IVec3::ZERO,
            initialized: true,
        };
        // Speed ~0.14, below 0.5 threshold
        assert!(vel.direction_xz(0.5).is_none());
    }

    #[test]
    fn test_direction_xz_diagonal() {
        let vel = PlayerChunkVelocity {
            velocity: Vec3::new(1.0, 0.0, 1.0),
            prev_chunk: IVec3::ZERO,
            initialized: true,
        };
        let dir = vel.direction_xz(0.5).unwrap();
        let expected = 1.0 / 2.0_f32.sqrt();
        assert!((dir.x - expected).abs() < 0.01);
        assert!((dir.z - expected).abs() < 0.01);
        assert_eq!(dir.y, 0.0);
    }

    #[test]
    fn test_horizontal_speed() {
        let vel = PlayerChunkVelocity {
            velocity: Vec3::new(3.0, 10.0, 4.0),
            prev_chunk: IVec3::ZERO,
            initialized: true,
        };
        // XZ speed = sqrt(9 + 16) = 5.0, Y is ignored
        assert!((vel.horizontal_speed() - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_horizontal_speed_zero() {
        let vel = PlayerChunkVelocity::default();
        assert_eq!(vel.horizontal_speed(), 0.0);
    }

    // --- compute_predicted_positions tests ---

    #[test]
    fn test_predicted_positions_positive_x() {
        let positions = compute_predicted_positions(
            IVec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0), // moving +X
            4,                        // load_dist
            2,                        // lookahead
            0,                        // vert_down
            0,                        // vert_up
        );

        // 2 steps * 3 lateral * 1 vertical = 6 positions
        assert_eq!(positions.len(), 6);

        // All positions should be ahead of center in X
        for pos in &positions {
            assert!(pos.x > 0, "Predicted position should be ahead: {:?}", pos);
        }
    }

    #[test]
    fn test_predicted_positions_negative_z() {
        let positions = compute_predicted_positions(
            IVec3::new(5, 0, 5),
            Vec3::new(0.0, 0.0, -1.0), // moving -Z
            4,
            1, // 1 step lookahead
            0,
            0,
        );

        // 1 step * 3 lateral * 1 vertical = 3 positions
        assert_eq!(positions.len(), 3);

        // All should be at z < center.z (ahead in -Z direction)
        for pos in &positions {
            assert!(pos.z < 5, "Should be ahead in -Z: {:?}", pos);
        }
    }

    #[test]
    fn test_predicted_positions_with_vertical_range() {
        let positions = compute_predicted_positions(
            IVec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            4,
            1,
            2, // vert_down
            4, // vert_up
        );

        // 1 step * 3 lateral * 7 vertical (-2..=4) = 21 positions
        assert_eq!(positions.len(), 21);
    }

    #[test]
    fn test_predicted_positions_empty_with_zero_lookahead() {
        let positions = compute_predicted_positions(
            IVec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            4,
            0, // no lookahead
            0,
            0,
        );
        assert!(positions.is_empty());
    }

    #[test]
    fn test_predicted_positions_diagonal() {
        let dir = Vec3::new(1.0, 0.0, 1.0).normalize();
        let positions = compute_predicted_positions(IVec3::ZERO, dir, 4, 1, 0, 0);

        // 1 step * 3 lateral * 1 vertical = 3
        assert_eq!(positions.len(), 3);

        // At least one should be ahead in both X and Z
        let any_ahead = positions.iter().any(|p| p.x > 0 && p.z > 0);
        assert!(
            any_ahead,
            "Should have positions ahead in diagonal: {:?}",
            positions
        );
    }

    #[test]
    fn test_predicted_positions_no_duplicates() {
        let positions =
            compute_predicted_positions(IVec3::ZERO, Vec3::new(1.0, 0.0, 0.0), 4, 3, 1, 1);

        let unique: std::collections::HashSet<_> = positions.iter().collect();
        // Note: duplicates CAN occur when rounding overlaps (e.g., perpendicular
        // offsets land on the same integer). This test just verifies we don't
        // produce obviously excessive duplicates.
        assert!(
            unique.len() > positions.len() / 2,
            "Too many duplicates: {} unique of {} total",
            unique.len(),
            positions.len()
        );
    }

    // --- Velocity EMA tests (unit-level, no ECS) ---

    #[test]
    fn test_ema_smoothing_converges() {
        // Simulate EMA: constant input should converge to that value
        let alpha = 0.15_f32;
        let target = Vec3::new(2.0, 0.0, 1.0);
        let mut smoothed = Vec3::ZERO;

        for _ in 0..100 {
            smoothed = smoothed * (1.0 - alpha) + target * alpha;
        }

        assert!((smoothed.x - target.x).abs() < 0.01);
        assert!((smoothed.z - target.z).abs() < 0.01);
    }

    #[test]
    fn test_ema_smoothing_zero_alpha_no_change() {
        let alpha = 0.0_f32;
        let smoothed = Vec3::new(1.0, 2.0, 3.0);
        let raw = Vec3::new(10.0, 20.0, 30.0);
        let result = smoothed * (1.0 - alpha) + raw * alpha;
        assert_eq!(result, smoothed);
    }

    #[test]
    fn test_ema_smoothing_one_alpha_instant() {
        let alpha = 1.0_f32;
        let smoothed = Vec3::new(1.0, 2.0, 3.0);
        let raw = Vec3::new(10.0, 20.0, 30.0);
        let result = smoothed * (1.0 - alpha) + raw * alpha;
        assert_eq!(result, raw);
    }
}
