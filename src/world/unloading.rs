//! Chunk unloading system — distance-based memory management
//!
//! Automatically unloads chunks that are too far from the player,
//! saving modified chunks to disk before removal. Includes memory
//! pressure monitoring to trigger more aggressive unloading when
//! the process approaches configurable memory thresholds.
//!
//! # Architecture
//!
//! ```text
//! chunk_unloading_system
//!   ├── Calculate effective unload distance (memory pressure)
//!   ├── For each chunk beyond distance:
//!   │   ├── Modified? → spawn async save task (PendingSave)
//!   │   └── Not modified? → despawn immediately (regenerable)
//!   └── Also despawn far pending chunk tasks
//!
//! poll_pending_saves
//!   ├── Save complete + still out of range → despawn entity
//!   └── Save complete + back in range → keep entity, remove PendingSave
//! ```

use bevy::prelude::*;
use bevy::tasks::{block_on, futures_lite::future, AsyncComputeTaskPool, Task};

use super::persistence::{self, ChunkStorage};
use super::{Chunk, ChunkManager, PendingChunk};
use crate::engine::memory::get_process_memory;

// ============================================================================
// CONFIGURATION
// ============================================================================

/// Configuration for the chunk unloading system.
///
/// Controls when and how chunks are removed from memory. Insert as a
/// Bevy resource — the defaults are sensible for most setups.
///
/// # Memory Pressure
///
/// When the process RSS exceeds `memory_threshold_bytes`, the effective
/// unload distance is reduced by `memory_pressure_reduction` chunks,
/// causing more aggressive cleanup without dropping below render distance.
#[derive(Resource, Clone, Debug)]
pub struct UnloadConfig {
    /// Distance in chunks beyond which chunks are unloaded.
    /// If `None`, defaults to `render_distance + 2`.
    pub unload_distance: Option<i32>,

    /// Whether to save modified chunks to disk before unloading.
    /// Default: `true`. Disabling this **discards** unsaved player edits!
    pub save_on_unload: bool,

    /// Memory threshold in bytes. When process RSS exceeds this value,
    /// the effective unload distance shrinks to free memory faster.
    /// Default: 2 GB.
    pub memory_threshold_bytes: usize,

    /// How many chunks to shrink the unload distance when memory
    /// pressure is detected. Default: 2.
    pub memory_pressure_reduction: i32,

    /// Maximum async save tasks to spawn per frame (prevents I/O storms).
    /// Default: 4.
    pub max_saves_per_frame: u32,
}

impl Default for UnloadConfig {
    fn default() -> Self {
        Self {
            unload_distance: None,
            save_on_unload: true,
            memory_threshold_bytes: 2 * 1024 * 1024 * 1024, // 2 GB
            memory_pressure_reduction: 2,
            max_saves_per_frame: 4,
        }
    }
}

impl UnloadConfig {
    /// Calculate the effective unload distance, factoring in memory pressure.
    ///
    /// Returns the base unload distance (explicit or `render_distance + 2`),
    /// reduced by `memory_pressure_reduction` if the process RSS exceeds
    /// the configured threshold. The result is clamped so it never drops
    /// below `render_distance` (we never unload chunks the player can see).
    pub fn effective_unload_distance(&self, render_distance: i32) -> i32 {
        let base = self.unload_distance.unwrap_or(render_distance + 2);

        let under_pressure = get_process_memory()
            .is_some_and(|mem| mem.rss_bytes > self.memory_threshold_bytes);

        if under_pressure {
            (base - self.memory_pressure_reduction).max(render_distance)
        } else {
            base
        }
    }
}

// ============================================================================
// COMPONENTS
// ============================================================================

/// Component attached to a chunk entity while its data is being saved
/// to disk on a background thread via `AsyncComputeTaskPool`.
///
/// The entity will be despawned (or kept, if the player moved back)
/// once the save task completes.
#[derive(Component)]
pub struct PendingSave {
    /// The async task performing the disk write.
    task: Task<Result<(), String>>,
    /// Chunk position (for bookkeeping cleanup on completion).
    position: IVec3,
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// Unload chunks that are too far from the player.
///
/// Modified chunks are saved to disk asynchronously before removal.
/// Unmodified chunks are despawned immediately — they can be regenerated
/// from the same seed. Also cleans up out-of-range pending chunk tasks.
pub fn chunk_unloading_system(
    mut commands: Commands,
    mut chunk_manager: ResMut<ChunkManager>,
    unload_config: Res<UnloadConfig>,
    storage: Res<ChunkStorage>,
    chunk_query: Query<(Entity, &Chunk), Without<PendingSave>>,
    pending_query: Query<(Entity, &PendingChunk)>,
) {
    let center = chunk_manager.player_chunk;
    let max_dist = unload_config.effective_unload_distance(chunk_manager.render_distance);
    let mut saves_this_frame: u32 = 0;

    let task_pool = AsyncComputeTaskPool::get();

    // --- Unload loaded chunks beyond distance ---
    let mut to_remove = Vec::new();
    for (entity, chunk) in &chunk_query {
        let diff = chunk.position - center;
        let dist = diff.x.abs().max(diff.y.abs()).max(diff.z.abs());

        if dist > max_dist {
            if chunk.modified && unload_config.save_on_unload {
                // Rate-limit save tasks per frame
                if saves_this_frame >= unload_config.max_saves_per_frame {
                    continue; // Will be picked up next frame
                }

                // Clone chunk data for the background save task
                let chunk_clone = chunk.clone();
                let save_storage = ChunkStorage::new(storage.save_dir.clone());
                let pos = chunk.position;

                let task = task_pool.spawn(async move {
                    persistence::save_chunk(&chunk_clone, &save_storage)
                        .map_err(|e| e.to_string())
                });

                commands.entity(entity).insert(PendingSave {
                    task,
                    position: pos,
                });

                saves_this_frame += 1;
            } else {
                // Not modified or save disabled — despawn immediately
                commands.entity(entity).despawn_recursive();
                to_remove.push(chunk.position);
            }
        }
    }
    for pos in to_remove {
        chunk_manager.chunks.remove(&pos);
    }

    // --- Despawn pending-chunk tasks that are out of range ---
    let mut pending_to_remove = Vec::new();
    for (entity, pending) in &pending_query {
        let diff = pending.position - center;
        let dist = diff.x.abs().max(diff.y.abs()).max(diff.z.abs());

        if dist > max_dist {
            commands.entity(entity).despawn_recursive();
            pending_to_remove.push(pending.position);
        }
    }
    for pos in pending_to_remove {
        chunk_manager.pending.remove(&pos);
    }
}

/// Poll completed save tasks and finalize chunk unloading.
///
/// If the player has moved back into range while the save was in flight,
/// the chunk is kept loaded (PendingSave removed, entity preserved).
/// Otherwise the entity is despawned and removed from the chunk map.
pub fn poll_pending_saves(
    mut commands: Commands,
    mut chunk_manager: ResMut<ChunkManager>,
    mut save_query: Query<(Entity, &mut PendingSave)>,
) {
    for (entity, mut pending) in &mut save_query {
        if let Some(result) = block_on(future::poll_once(&mut pending.task)) {
            match &result {
                Ok(()) => {
                    info!("Saved chunk at {:?} to disk", pending.position);
                }
                Err(e) => {
                    warn!(
                        "Failed to save chunk at {:?}: {}. Unloading anyway.",
                        pending.position, e
                    );
                }
            }

            let pos = pending.position;

            // Check if the player moved back into range while we were saving
            let diff = pos - chunk_manager.player_chunk;
            let dist = diff.x.abs().max(diff.y.abs()).max(diff.z.abs());

            if dist <= chunk_manager.render_distance {
                // Player came back — keep the chunk loaded
                commands.entity(entity).remove::<PendingSave>();
                info!("Chunk at {:?} back in range, keeping loaded", pos);
            } else {
                // Still out of range — despawn
                commands.entity(entity).despawn_recursive();
                chunk_manager.chunks.remove(&pos);
            }
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unload_config_default() {
        let config = UnloadConfig::default();
        assert!(config.save_on_unload);
        assert_eq!(config.unload_distance, None);
        assert_eq!(config.memory_threshold_bytes, 2 * 1024 * 1024 * 1024);
        assert_eq!(config.memory_pressure_reduction, 2);
        assert_eq!(config.max_saves_per_frame, 4);
    }

    #[test]
    fn test_effective_unload_distance_defaults_to_render_plus_2() {
        let config = UnloadConfig {
            // Set threshold very high so memory pressure never triggers
            memory_threshold_bytes: usize::MAX,
            ..Default::default()
        };
        assert_eq!(config.effective_unload_distance(4), 6);
        assert_eq!(config.effective_unload_distance(8), 10);
    }

    #[test]
    fn test_effective_unload_distance_explicit() {
        let config = UnloadConfig {
            unload_distance: Some(10),
            memory_threshold_bytes: usize::MAX,
            ..Default::default()
        };
        assert_eq!(config.effective_unload_distance(4), 10);
    }

    #[test]
    fn test_effective_unload_distance_memory_pressure() {
        // Set threshold to 0 so we're always under pressure
        let config = UnloadConfig {
            unload_distance: Some(10),
            memory_threshold_bytes: 0,
            memory_pressure_reduction: 3,
            ..Default::default()
        };
        // Under pressure: 10 - 3 = 7
        assert_eq!(config.effective_unload_distance(4), 7);
    }

    #[test]
    fn test_effective_unload_distance_clamped_to_render_distance() {
        let config = UnloadConfig {
            unload_distance: Some(5),
            memory_threshold_bytes: 0,
            memory_pressure_reduction: 10,
            ..Default::default()
        };
        // 5 - 10 = -5, clamped to render_distance (4)
        assert_eq!(config.effective_unload_distance(4), 4);
    }

    #[test]
    fn test_effective_unload_distance_pressure_never_below_render() {
        let config = UnloadConfig {
            unload_distance: None, // defaults to render_distance + 2
            memory_threshold_bytes: 0,
            memory_pressure_reduction: 100,
            ..Default::default()
        };
        // (4+2) - 100 = -94, clamped to render_distance (4)
        assert_eq!(config.effective_unload_distance(4), 4);
    }

    #[test]
    fn test_unload_config_clone() {
        let config = UnloadConfig {
            unload_distance: Some(8),
            save_on_unload: false,
            memory_threshold_bytes: 1_000_000,
            memory_pressure_reduction: 1,
            max_saves_per_frame: 2,
        };
        let cloned = config.clone();
        assert_eq!(cloned.unload_distance, Some(8));
        assert!(!cloned.save_on_unload);
        assert_eq!(cloned.memory_threshold_bytes, 1_000_000);
        assert_eq!(cloned.memory_pressure_reduction, 1);
        assert_eq!(cloned.max_saves_per_frame, 2);
    }
}
