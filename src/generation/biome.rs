//! Biome system — noise-based biome selection and terrain parameterization
//!
//! Biomes control terrain shape, vegetation density, and block palettes.
//! A separate 2D simplex noise instance (with a different seed from terrain
//! noise) produces smooth, deterministic biome boundaries across the world.
//!
//! # Biome Selection
//!
//! Two noise channels — *temperature* and *moisture* — are sampled at each
//! world (x, z) position.  The combination determines the biome:
//!
//! ```text
//!              dry ← moisture → wet
//!   cold  ┌──────────┬──────────┐
//!         │  Tundra  │  Tundra  │
//!         ├──────────┼──────────┤
//!   mild  │Mountains │  Forest  │
//!         ├──────────┼──────────┤
//!   hot   │  Desert  │  Plains  │
//!         └──────────┴──────────┘
//! ```

use noise::{NoiseFn, Simplex};

use crate::world::BlockType;

// ============================================================================
// BIOME TYPE
// ============================================================================

/// The five foundation biome types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BiomeType {
    Plains,
    Desert,
    Forest,
    Mountains,
    Tundra,
}

impl BiomeType {
    /// Return all biome variants (useful for testing / iteration).
    pub fn all() -> &'static [BiomeType] {
        &[
            BiomeType::Plains,
            BiomeType::Desert,
            BiomeType::Forest,
            BiomeType::Mountains,
            BiomeType::Tundra,
        ]
    }
}

// ============================================================================
// BIOME PARAMETERS
// ============================================================================

/// Parameters that define how a biome shapes the terrain.
#[derive(Clone, Debug)]
pub struct BiomeParams {
    /// Height variation multiplier (replaces global height_scale per-column).
    pub terrain_amplitude: f64,
    /// Base noise frequency for terrain (lower = smoother).
    pub terrain_frequency: f64,
    /// Probability (0.0–1.0) that an eligible surface position gets a tree.
    pub tree_density: f64,
    /// Offset added to the global sea level for this biome.
    pub sea_level_offset: i32,
    /// Block placed at the terrain surface.
    pub surface_block: BlockType,
    /// Block placed in the 1–4 blocks below the surface.
    pub subsurface_block: BlockType,
    /// Block used for the deep underground.
    pub deep_block: BlockType,
    /// If `Some(y)`, surface blocks at or above world-Y `y` become Snow
    /// (used for mountain snow caps).
    pub snow_cap_height: Option<i32>,
}

impl BiomeType {
    /// Get the terrain parameters for this biome.
    pub fn params(&self) -> BiomeParams {
        match self {
            // ----------------------------------------------------------
            // Plains — gentle rolling hills, standard palette
            // ----------------------------------------------------------
            BiomeType::Plains => BiomeParams {
                terrain_amplitude: 8.0,
                terrain_frequency: 0.02,
                tree_density: 0.02,
                sea_level_offset: 0,
                surface_block: BlockType::Grass,
                subsurface_block: BlockType::Dirt,
                deep_block: BlockType::Stone,
                snow_cap_height: None,
            },
            // ----------------------------------------------------------
            // Desert — medium dune-like terrain, sand palette, no trees
            // ----------------------------------------------------------
            BiomeType::Desert => BiomeParams {
                terrain_amplitude: 12.0,
                terrain_frequency: 0.015,
                tree_density: 0.0,
                sea_level_offset: -2,
                surface_block: BlockType::Sand,
                subsurface_block: BlockType::Sandstone,
                deep_block: BlockType::Stone,
                snow_cap_height: None,
            },
            // ----------------------------------------------------------
            // Forest — moderate terrain, dense trees
            // ----------------------------------------------------------
            BiomeType::Forest => BiomeParams {
                terrain_amplitude: 10.0,
                terrain_frequency: 0.02,
                tree_density: 0.08,
                sea_level_offset: 0,
                surface_block: BlockType::Grass,
                subsurface_block: BlockType::Dirt,
                deep_block: BlockType::Stone,
                snow_cap_height: None,
            },
            // ----------------------------------------------------------
            // Mountains — high amplitude, stone-heavy, snow caps
            // ----------------------------------------------------------
            BiomeType::Mountains => BiomeParams {
                terrain_amplitude: 32.0,
                terrain_frequency: 0.025,
                tree_density: 0.005,
                sea_level_offset: 4,
                surface_block: BlockType::Stone,
                subsurface_block: BlockType::Stone,
                deep_block: BlockType::Stone,
                snow_cap_height: Some(48),
            },
            // ----------------------------------------------------------
            // Tundra — flat, frozen, no trees
            // ----------------------------------------------------------
            BiomeType::Tundra => BiomeParams {
                terrain_amplitude: 6.0,
                terrain_frequency: 0.018,
                tree_density: 0.0,
                sea_level_offset: -1,
                surface_block: BlockType::Snow,
                subsurface_block: BlockType::Ice,
                deep_block: BlockType::Stone,
                snow_cap_height: None,
            },
        }
    }
}

// ============================================================================
// BIOME SELECTION
// ============================================================================

/// Determine the biome at a world (x, z) position.
///
/// Uses two orthogonal 2D simplex-noise samples (temperature and moisture)
/// to place the coordinate into one of the five biome zones.  The
/// `biome_scale` parameter controls how large biomes are — lower values
/// produce larger biomes.
///
/// # Determinism
///
/// Given the same `noise` instance (same seed) and `biome_scale`, the
/// result is perfectly deterministic for any (x, z).
pub fn biome_at(x: i32, z: i32, noise: &Simplex, biome_scale: f64) -> BiomeType {
    // Temperature channel
    let temp = noise.get([x as f64 * biome_scale, z as f64 * biome_scale]);

    // Moisture channel (offset by 1000 to decorrelate from temperature)
    let moisture = noise.get([
        x as f64 * biome_scale + 1000.0,
        z as f64 * biome_scale + 1000.0,
    ]);

    // Map (temp, moisture) → biome
    //   temp:     cold (< -0.3)  ·  mild  ·  hot (> 0.4)
    //   moisture: dry  (< -0.3)  ·  mid   ·  wet (> 0.2)
    if temp < -0.3 {
        BiomeType::Tundra
    } else if temp > 0.4 {
        if moisture < 0.0 {
            BiomeType::Desert
        } else {
            BiomeType::Plains
        }
    } else if moisture > 0.2 {
        BiomeType::Forest
    } else if moisture < -0.3 {
        BiomeType::Mountains
    } else {
        BiomeType::Plains
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a biome noise instance with the default offset seed.
    fn test_biome_noise() -> Simplex {
        Simplex::new(12345_u32.wrapping_add(999))
    }

    // ----------------------------------------------------------------
    // Determinism
    // ----------------------------------------------------------------

    #[test]
    fn test_biome_at_deterministic() {
        let noise = test_biome_noise();
        let scale = 0.005;

        // Same inputs → same output, every time
        for x in -100..100 {
            for z in -100..100 {
                let a = biome_at(x, z, &noise, scale);
                let b = biome_at(x, z, &noise, scale);
                assert_eq!(a, b, "biome_at must be deterministic at ({x}, {z})");
            }
        }
    }

    #[test]
    fn test_biome_at_different_seeds_differ() {
        let noise_a = Simplex::new(1);
        let noise_b = Simplex::new(9999);
        let scale = 0.005;

        let mut differences = 0;
        for x in -50..50 {
            for z in -50..50 {
                if biome_at(x, z, &noise_a, scale) != biome_at(x, z, &noise_b, scale) {
                    differences += 1;
                }
            }
        }
        assert!(
            differences > 0,
            "Different seeds should produce different biome maps"
        );
    }

    // ----------------------------------------------------------------
    // All biome types are reachable
    // ----------------------------------------------------------------

    #[test]
    fn test_all_biomes_reachable() {
        // Scan a large area to confirm every BiomeType appears at least once
        let noise = test_biome_noise();
        let scale = 0.005;
        let mut seen = std::collections::HashSet::new();

        for x in (-500..500).step_by(3) {
            for z in (-500..500).step_by(3) {
                seen.insert(biome_at(x, z, &noise, scale));
                if seen.len() == BiomeType::all().len() {
                    return; // All found — pass
                }
            }
        }

        let missing: Vec<_> = BiomeType::all()
            .iter()
            .filter(|b| !seen.contains(b))
            .collect();
        panic!("Not all biomes reachable. Missing: {:?}", missing);
    }

    // ----------------------------------------------------------------
    // Biome parameter correctness
    // ----------------------------------------------------------------

    #[test]
    fn test_plains_params() {
        let p = BiomeType::Plains.params();
        assert_eq!(p.surface_block, BlockType::Grass);
        assert_eq!(p.subsurface_block, BlockType::Dirt);
        assert_eq!(p.deep_block, BlockType::Stone);
        assert!(p.terrain_amplitude > 0.0);
        assert!(p.terrain_frequency > 0.0);
        assert!(p.tree_density > 0.0, "Plains should have some trees");
        assert_eq!(p.sea_level_offset, 0);
        assert!(p.snow_cap_height.is_none());
    }

    #[test]
    fn test_desert_params() {
        let p = BiomeType::Desert.params();
        assert_eq!(p.surface_block, BlockType::Sand);
        assert_eq!(p.subsurface_block, BlockType::Sandstone);
        assert_eq!(p.deep_block, BlockType::Stone);
        assert!(
            p.terrain_amplitude > BiomeType::Plains.params().terrain_amplitude,
            "Desert dunes should have more amplitude than plains"
        );
        assert!(
            (p.tree_density - 0.0).abs() < f64::EPSILON,
            "Desert should have no trees"
        );
        assert!(p.snow_cap_height.is_none());
    }

    #[test]
    fn test_forest_params() {
        let p = BiomeType::Forest.params();
        assert_eq!(p.surface_block, BlockType::Grass);
        assert_eq!(p.subsurface_block, BlockType::Dirt);
        assert_eq!(p.deep_block, BlockType::Stone);
        assert!(
            p.tree_density > BiomeType::Plains.params().tree_density,
            "Forest should have higher tree density than Plains"
        );
    }

    #[test]
    fn test_mountains_params() {
        let p = BiomeType::Mountains.params();
        assert_eq!(p.surface_block, BlockType::Stone);
        assert_eq!(p.subsurface_block, BlockType::Stone);
        assert_eq!(p.deep_block, BlockType::Stone);
        assert!(
            p.terrain_amplitude > BiomeType::Forest.params().terrain_amplitude,
            "Mountains should have highest amplitude"
        );
        assert!(
            p.tree_density < BiomeType::Plains.params().tree_density,
            "Mountains should have low tree density"
        );
        assert!(p.snow_cap_height.is_some(), "Mountains should have snow caps");
    }

    #[test]
    fn test_tundra_params() {
        let p = BiomeType::Tundra.params();
        assert_eq!(p.surface_block, BlockType::Snow);
        assert_eq!(p.subsurface_block, BlockType::Ice);
        assert_eq!(p.deep_block, BlockType::Stone);
        assert!(
            (p.tree_density - 0.0).abs() < f64::EPSILON,
            "Tundra should have no trees"
        );
        assert!(
            p.terrain_amplitude < BiomeType::Plains.params().terrain_amplitude,
            "Tundra should be flatter than plains"
        );
    }

    #[test]
    fn test_all_biomes_have_positive_amplitude_and_frequency() {
        for biome in BiomeType::all() {
            let p = biome.params();
            assert!(
                p.terrain_amplitude > 0.0,
                "{:?} terrain_amplitude should be positive",
                biome
            );
            assert!(
                p.terrain_frequency > 0.0,
                "{:?} terrain_frequency should be positive",
                biome
            );
        }
    }

    #[test]
    fn test_all_biomes_deep_block_is_stone() {
        for biome in BiomeType::all() {
            assert_eq!(
                biome.params().deep_block,
                BlockType::Stone,
                "{:?} deep_block should be Stone",
                biome,
            );
        }
    }

    #[test]
    fn test_biome_type_all_returns_five() {
        assert_eq!(BiomeType::all().len(), 5);
    }
}
