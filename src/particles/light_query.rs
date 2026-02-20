//! Block light level queries for the particle system.
//!
//! Provides a [`BlockLightQuery`] resource that particles use to sample
//! light levels at arbitrary world positions. When the block light
//! propagation system (`feature/block-light-propagation`) is integrated,
//! this module will read from [`WorldLightMap`]. Until then, it provides
//! estimated light levels based on vertical position and sky exposure.
//!
//! # Light Scale
//!
//! Light levels follow the 0–15 integer scale (matching Minecraft convention):
//! - 0 = total darkness
//! - 15 = maximum brightness (direct sunlight or adjacent to a max emitter)
//!
//! The [`LightSample`] struct normalizes this to a 0.0–1.0 float for
//! convenient use in shaders and color modulation.

use bevy::prelude::*;

use crate::world::{world_to_chunk_pos, Chunk, ChunkManager, CHUNK_SIZE};

// ============================================================================
// LIGHT SAMPLE
// ============================================================================

/// A sampled light level at a specific world position.
#[derive(Debug, Clone, Copy)]
pub struct LightSample {
    /// Raw block light level (0–15).
    pub block_light: u8,
    /// Normalized brightness factor (0.0–1.0).
    /// Computed as `block_light / 15.0`.
    pub brightness: f32,
}

impl LightSample {
    /// Create a light sample from a raw block light level.
    pub fn from_level(level: u8) -> Self {
        let clamped = level.min(15);
        Self {
            block_light: clamped,
            brightness: clamped as f32 / 15.0,
        }
    }

    /// Maximum brightness sample (level 15).
    pub fn max() -> Self {
        Self::from_level(15)
    }

    /// Minimum brightness sample (level 0).
    pub fn min() -> Self {
        Self::from_level(0)
    }

    /// Default fallback when light data is unavailable.
    /// Returns a moderate ambient level (7) to avoid pitch-black particles.
    pub fn fallback() -> Self {
        Self::from_level(7)
    }
}

impl Default for LightSample {
    fn default() -> Self {
        Self::fallback()
    }
}

// ============================================================================
// BLOCK LIGHT QUERY
// ============================================================================

/// Configuration for the block light query system.
#[derive(Resource, Debug, Clone)]
pub struct BlockLightQueryConfig {
    /// Minimum light level applied to all queries (prevents pitch-black particles).
    /// Range: 0–15. Default: 1.
    pub minimum_light: u8,
    /// Whether to use height-based sky light estimation when block light
    /// propagation data is unavailable. Default: true.
    pub use_sky_estimation: bool,
    /// Y-level above which full sky light is assumed. Default: 64.0.
    pub sky_light_height: f32,
    /// Y-level below which no sky light penetrates. Default: 0.0.
    pub underground_height: f32,
}

impl Default for BlockLightQueryConfig {
    fn default() -> Self {
        Self {
            minimum_light: 1,
            use_sky_estimation: true,
            sky_light_height: 64.0,
            underground_height: 0.0,
        }
    }
}

/// Resource that provides block light level queries at world positions.
///
/// Currently uses height-based sky light estimation as a placeholder.
/// When `feature/block-light-propagation` is merged, this will read
/// directly from `WorldLightMap` for accurate per-block light levels.
#[derive(Resource, Default)]
pub struct BlockLightQuery {
    /// Configuration for light queries.
    pub config: BlockLightQueryConfig,
}

impl BlockLightQuery {
    /// Sample the block light level at a world position.
    ///
    /// This is the primary API for particles to query light. Returns a
    /// [`LightSample`] with both the raw 0–15 level and a normalized
    /// 0.0–1.0 brightness factor.
    ///
    /// # Current implementation (placeholder)
    ///
    /// Estimates light based on:
    /// 1. Height-based sky light (higher = brighter, linear interpolation)
    /// 2. Minimum light floor from config
    ///
    /// # Future implementation (with block light propagation)
    ///
    /// Will query `WorldLightMap` for the chunk containing `world_pos`,
    /// convert to local coordinates, and read the stored light level.
    pub fn sample_at(&self, world_pos: Vec3) -> LightSample {
        if self.config.use_sky_estimation {
            let sky_light = self.estimate_sky_light(world_pos.y);
            let final_level = sky_light.max(self.config.minimum_light);
            LightSample::from_level(final_level)
        } else {
            LightSample::from_level(self.config.minimum_light)
        }
    }

    /// Sample block light using chunk data for occlusion estimation.
    ///
    /// Checks whether the block at the particle's position is inside a
    /// solid block (occluded) and reduces light accordingly. This gives
    /// a rough approximation of shadow until the full light propagation
    /// system is integrated.
    pub fn sample_with_chunks(
        &self,
        world_pos: Vec3,
        chunk_manager: &ChunkManager,
        chunks: &Query<&Chunk>,
    ) -> LightSample {
        let chunk_pos = world_to_chunk_pos(world_pos);

        // If we can look up the chunk, check for occlusion
        if let Some(&entity) = chunk_manager.chunks.get(&chunk_pos)
            && let Ok(chunk) = chunks.get(entity)
        {
            let local_x =
                ((world_pos.x - (chunk_pos.x * CHUNK_SIZE as i32) as f32) as usize)
                    .min(CHUNK_SIZE - 1);
            let local_y =
                ((world_pos.y - (chunk_pos.y * CHUNK_SIZE as i32) as f32) as usize)
                    .min(CHUNK_SIZE - 1);
            let local_z =
                ((world_pos.z - (chunk_pos.z * CHUNK_SIZE as i32) as f32) as usize)
                    .min(CHUNK_SIZE - 1);

            let block = chunk.get_block(local_x, local_y, local_z);
            if block.is_solid() {
                // Particle is inside a solid block — very dark
                return LightSample::from_level(self.config.minimum_light);
            }

            // Check if there's a solid block directly above (simple shadow)
            let above_y = local_y + 1;
            if above_y < CHUNK_SIZE {
                let above_block = chunk.get_block(local_x, above_y, local_z);
                if above_block.is_solid() {
                    // Under a solid block — reduced sky light
                    let reduced = self.estimate_sky_light(world_pos.y).saturating_sub(4);
                    return LightSample::from_level(
                        reduced.max(self.config.minimum_light),
                    );
                }
            }
        }

        // Fallback to basic sky estimation
        self.sample_at(world_pos)
    }

    /// Estimate sky light level based on Y height.
    ///
    /// Returns 0–15 with linear interpolation between `underground_height`
    /// and `sky_light_height`.
    fn estimate_sky_light(&self, y: f32) -> u8 {
        let range = self.config.sky_light_height - self.config.underground_height;
        if range <= 0.0 {
            return 15;
        }

        let t = ((y - self.config.underground_height) / range).clamp(0.0, 1.0);
        (t * 15.0).round() as u8
    }
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that registers the block light query resource.
pub struct BlockLightQueryPlugin;

impl Plugin for BlockLightQueryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BlockLightQuery>()
            .init_resource::<BlockLightQueryConfig>();
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_light_sample_from_level() {
        let sample = LightSample::from_level(15);
        assert_eq!(sample.block_light, 15);
        assert!((sample.brightness - 1.0).abs() < f32::EPSILON);

        let sample = LightSample::from_level(0);
        assert_eq!(sample.block_light, 0);
        assert!((sample.brightness - 0.0).abs() < f32::EPSILON);

        let sample = LightSample::from_level(7);
        assert_eq!(sample.block_light, 7);
        assert!((sample.brightness - 7.0 / 15.0).abs() < 0.001);
    }

    #[test]
    fn test_light_sample_clamps_to_15() {
        let sample = LightSample::from_level(255);
        assert_eq!(sample.block_light, 15);
        assert!((sample.brightness - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_light_sample_max_min() {
        let max = LightSample::max();
        assert_eq!(max.block_light, 15);

        let min = LightSample::min();
        assert_eq!(min.block_light, 0);
    }

    #[test]
    fn test_light_sample_fallback() {
        let fb = LightSample::fallback();
        assert_eq!(fb.block_light, 7);
    }

    #[test]
    fn test_sky_light_estimation_at_ground() {
        let query = BlockLightQuery::default();
        // At underground_height (0.0), light should be near minimum
        let level = query.estimate_sky_light(0.0);
        assert_eq!(level, 0);
    }

    #[test]
    fn test_sky_light_estimation_at_sky() {
        let query = BlockLightQuery::default();
        // At sky_light_height (64.0), light should be maximum
        let level = query.estimate_sky_light(64.0);
        assert_eq!(level, 15);
    }

    #[test]
    fn test_sky_light_estimation_midpoint() {
        let query = BlockLightQuery::default();
        // At midpoint (32.0), light should be ~7-8
        let level = query.estimate_sky_light(32.0);
        assert!(level >= 7 && level <= 8, "Expected ~7-8, got {level}");
    }

    #[test]
    fn test_sky_light_estimation_above_sky() {
        let query = BlockLightQuery::default();
        // Above sky_light_height, should clamp to 15
        let level = query.estimate_sky_light(200.0);
        assert_eq!(level, 15);
    }

    #[test]
    fn test_sky_light_estimation_below_ground() {
        let query = BlockLightQuery::default();
        // Below underground_height, should clamp to 0
        let level = query.estimate_sky_light(-50.0);
        assert_eq!(level, 0);
    }

    #[test]
    fn test_sample_at_applies_minimum() {
        let mut query = BlockLightQuery::default();
        query.config.minimum_light = 3;
        // Underground should still return at least 3
        let sample = query.sample_at(Vec3::new(0.0, -50.0, 0.0));
        assert!(sample.block_light >= 3);
    }

    #[test]
    fn test_sample_at_sky_estimation_disabled() {
        let mut query = BlockLightQuery::default();
        query.config.use_sky_estimation = false;
        query.config.minimum_light = 5;
        let sample = query.sample_at(Vec3::new(0.0, 100.0, 0.0));
        assert_eq!(sample.block_light, 5);
    }

    #[test]
    fn test_query_config_defaults() {
        let config = BlockLightQueryConfig::default();
        assert_eq!(config.minimum_light, 1);
        assert!(config.use_sky_estimation);
        assert!((config.sky_light_height - 64.0).abs() < f32::EPSILON);
        assert!((config.underground_height - 0.0).abs() < f32::EPSILON);
    }
}
