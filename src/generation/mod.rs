//! Procedural generation systems - terrain, vegetation, structures
//!
//! This module contains:
//! - Terrain generation using noise functions
//! - Biome distribution (see [`biome`] submodule)
//! - Vegetation placement
//! - Structure generation

pub mod biome;

use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

use bevy::prelude::*;
use noise::{NoiseFn, Perlin, Simplex};

use crate::world::{BlockType, Chunk, CHUNK_SIZE};
use biome::{biome_at, BiomeType};

/// Configuration for terrain generation
#[derive(Resource)]
pub struct TerrainConfig {
    /// World seed
    pub seed: u32,
    /// Base terrain height
    pub base_height: f64,
    /// Terrain height variation (used as fallback when biomes are disabled)
    pub height_scale: f64,
    /// Noise frequency (used as fallback when biomes are disabled)
    pub frequency: f64,
    /// Number of noise octaves
    pub octaves: usize,
    /// Tree density — probability (0.0–1.0) that a valid surface position gets a tree.
    ///
    /// With biomes enabled this acts as a scaling factor relative to the
    /// default value (0.02).  For example, setting this to 0.04 doubles
    /// biome tree densities, and 0.0 disables trees entirely.
    pub tree_density: f64,
    /// Sea level — air blocks at or below this Y coordinate become water
    pub sea_level: i32,
    /// Noise frequency for biome selection (lower = larger biomes).
    ///
    /// A value of 0.005 produces biomes roughly 200 blocks across.
    pub biome_scale: f64,
    /// Seed offset added to `seed` for the biome noise instance,
    /// ensuring biome boundaries are independent of terrain shape.
    pub biome_seed_offset: u32,
}

/// Default tree density value used as the scaling reference.
const BASE_TREE_DENSITY: f64 = 0.02;

impl Default for TerrainConfig {
    fn default() -> Self {
        Self {
            seed: 12345,
            base_height: 32.0,
            height_scale: 16.0,
            frequency: 0.02,
            octaves: 4,
            tree_density: BASE_TREE_DENSITY,
            sea_level: 28,
            biome_scale: 0.005,
            biome_seed_offset: 999,
        }
    }
}

/// Calculate the terrain height and biome at a world (x, z) column.
///
/// Shared by terrain generation and cave generation so both agree on
/// where the surface is.
fn terrain_column(
    world_x: i32,
    world_z: i32,
    terrain_noise: &Simplex,
    biome_noise: &Simplex,
    config: &TerrainConfig,
) -> (i32, BiomeType) {
    let biome = biome_at(world_x, world_z, biome_noise, config.biome_scale);
    let params = biome.params();

    let mut height = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = params.terrain_frequency;

    for _ in 0..config.octaves {
        height += terrain_noise.get([
            world_x as f64 * frequency,
            world_z as f64 * frequency,
        ]) * amplitude;
        amplitude *= 0.5;
        frequency *= 2.0;
    }

    let terrain_height = (config.base_height + height * params.terrain_amplitude) as i32;
    (terrain_height, biome)
}

/// Generates terrain for a chunk using simplex noise and biome parameters.
pub fn generate_chunk_terrain(chunk: &mut Chunk, config: &TerrainConfig) {
    let terrain_noise = Simplex::new(config.seed);
    let biome_noise = Simplex::new(config.seed.wrapping_add(config.biome_seed_offset));
    let world_pos = chunk.world_position();

    for local_x in 0..CHUNK_SIZE {
        for local_z in 0..CHUNK_SIZE {
            let world_x = world_pos.x + local_x as i32;
            let world_z = world_pos.z + local_z as i32;

            let (terrain_height, biome) =
                terrain_column(world_x, world_z, &terrain_noise, &biome_noise, config);
            let params = biome.params();
            let effective_sea_level = config.sea_level + params.sea_level_offset;

            // Fill column with biome-appropriate blocks
            for local_y in 0..CHUNK_SIZE {
                let world_y = world_pos.y + local_y as i32;

                let block = if world_y > terrain_height {
                    BlockType::Air
                } else if world_y == terrain_height {
                    // Snow cap override for mountains
                    if let Some(cap) = params.snow_cap_height {
                        if world_y >= cap {
                            BlockType::Snow
                        } else {
                            params.surface_block
                        }
                    } else {
                        params.surface_block
                    }
                } else if world_y > terrain_height - 4 {
                    params.subsurface_block
                } else {
                    params.deep_block
                };

                // Fill air at or below sea level with water
                let block = if block == BlockType::Air && world_y <= effective_sea_level {
                    BlockType::Water
                } else {
                    block
                };

                chunk.set_block(local_x, local_y, local_z, block);
            }
        }
    }
}

/// Simple 3D noise for cave generation
/// Protects the surface layer (grass and top dirt) from being carved
pub fn generate_caves(chunk: &mut Chunk, config: &TerrainConfig) {
    let noise = Perlin::new(config.seed.wrapping_add(1000));
    let terrain_noise = Simplex::new(config.seed);
    let biome_noise = Simplex::new(config.seed.wrapping_add(config.biome_seed_offset));
    let world_pos = chunk.world_position();

    // Higher threshold = fewer caves (0.7 means only top 15% of noise creates caves)
    let cave_threshold = 0.7;
    // Protect this many blocks below the surface from caves
    let surface_protection = 5;

    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            let world_x = world_pos.x + x as i32;
            let world_z = world_pos.z + z as i32;

            // Calculate terrain height at this column (biome-aware, same as terrain generation)
            let (terrain_height, _biome) =
                terrain_column(world_x, world_z, &terrain_noise, &biome_noise, config);

            for y in 0..CHUNK_SIZE {
                let world_y = world_pos.y + y as i32;

                // Skip if this is air or within surface protection zone
                let block = chunk.get_block(x, y, z);
                if block == BlockType::Air {
                    continue;
                }

                // Protect surface: don't carve within N blocks of terrain height
                if world_y > terrain_height - surface_protection {
                    continue;
                }

                // 3D cave noise
                let cave_noise = noise.get([
                    world_x as f64 * 0.05,
                    world_y as f64 * 0.05,
                    world_z as f64 * 0.05,
                ]);

                if cave_noise > cave_threshold {
                    chunk.set_block(x, y, z, BlockType::Air);
                }
            }
        }
    }
}

/// Deterministic hash for tree placement decisions.
/// Returns a value in [0.0, 1.0) based on world position and seed.
fn tree_hash(world_x: i32, world_z: i32, seed: u32) -> f64 {
    let mut hasher = DefaultHasher::new();
    // Use a domain tag so this hash doesn't collide with other uses
    "tree_placement".hash(&mut hasher);
    seed.hash(&mut hasher);
    world_x.hash(&mut hasher);
    world_z.hash(&mut hasher);
    let h = hasher.finish();
    // Map u64 to [0.0, 1.0)
    (h as f64) / (u64::MAX as f64)
}

/// Deterministic hash to choose trunk height (4–6) for a given tree position.
fn trunk_height_hash(world_x: i32, world_z: i32, seed: u32) -> usize {
    let mut hasher = DefaultHasher::new();
    "trunk_height".hash(&mut hasher);
    seed.hash(&mut hasher);
    world_x.hash(&mut hasher);
    world_z.hash(&mut hasher);
    let h = hasher.finish();
    // Map to range 4..=6
    4 + (h % 3) as usize
}

/// Place simple trees on the surface of a chunk.
///
/// For each (x, z) column, finds the highest Grass block (the surface),
/// then probabilistically places a tree: a Wood trunk 4-6 blocks tall
/// topped with a diamond-shaped Leaves canopy (radius 2).
///
/// Tree density is determined per-column by the biome at that position,
/// scaled by `config.tree_density` relative to the default density.
/// Trees are only placed on Grass blocks and when the entire structure
/// fits within the chunk.
pub fn generate_trees(chunk: &mut Chunk, config: &TerrainConfig) {
    let biome_noise = Simplex::new(config.seed.wrapping_add(config.biome_seed_offset));
    let world_pos = chunk.world_position();
    let canopy_radius: i32 = 2;

    // Global scaling factor: config.tree_density / default allows tests
    // and config to control density while biomes provide the base rate.
    let density_scale = if BASE_TREE_DENSITY > 0.0 {
        config.tree_density / BASE_TREE_DENSITY
    } else {
        0.0
    };

    for local_x in 0..CHUNK_SIZE {
        for local_z in 0..CHUNK_SIZE {
            let world_x = world_pos.x + local_x as i32;
            let world_z = world_pos.z + local_z as i32;

            // --- Boundary check for canopy in x/z ---
            // The canopy extends ±canopy_radius from the trunk.
            // Reject positions where canopy would leave chunk bounds.
            if (local_x as i32) < canopy_radius
                || (local_x as i32) + canopy_radius >= CHUNK_SIZE as i32
            {
                continue;
            }
            if (local_z as i32) < canopy_radius
                || (local_z as i32) + canopy_radius >= CHUNK_SIZE as i32
            {
                continue;
            }

            // --- Find highest Grass block (scan top-down) ---
            // Trees only grow on Grass; biomes without Grass surfaces
            // (Desert, Tundra, Mountains) naturally produce no trees.
            let mut surface_y: Option<usize> = None;
            for local_y in (0..CHUNK_SIZE).rev() {
                if chunk.get_block(local_x, local_y, local_z) == BlockType::Grass {
                    surface_y = Some(local_y);
                    break;
                }
            }

            let surface_y = match surface_y {
                Some(y) => y,
                None => continue, // No grass in this column
            };

            // --- Biome-aware tree density ---
            let biome = biome_at(world_x, world_z, &biome_noise, config.biome_scale);
            let effective_density = biome.params().tree_density * density_scale;

            // --- Deterministic spawn decision ---
            let hash_val = tree_hash(world_x, world_z, config.seed);
            if hash_val >= effective_density {
                continue;
            }

            // --- Choose trunk height ---
            let trunk_height = trunk_height_hash(world_x, world_z, config.seed);

            // The trunk starts at surface_y + 1 and goes up trunk_height blocks.
            // The canopy center is at the top of the trunk.
            // Canopy extends ±canopy_radius vertically from center (diamond shape).
            let trunk_base = surface_y + 1;
            let trunk_top = trunk_base + trunk_height - 1; // inclusive
            let canopy_center_y = trunk_top;
            let canopy_top = canopy_center_y as i32 + canopy_radius;

            // Check that the whole tree fits vertically in the chunk
            if canopy_top >= CHUNK_SIZE as i32 {
                continue;
            }

            // --- Place trunk (Wood) ---
            for y in trunk_base..=trunk_top {
                chunk.set_block(local_x, y, local_z, BlockType::Wood);
            }

            // --- Place canopy (diamond / taxicab-distance sphere) ---
            // The canopy is centred on the trunk top block.
            let cx = local_x as i32;
            let cy = canopy_center_y as i32;
            let cz = local_z as i32;

            for dy in -canopy_radius..=canopy_radius {
                for dx in -canopy_radius..=canopy_radius {
                    for dz in -canopy_radius..=canopy_radius {
                        // Diamond (taxicab) distance
                        if dx.abs() + dy.abs() + dz.abs() > canopy_radius {
                            continue;
                        }

                        let bx = (cx + dx) as usize;
                        let by = (cy + dy) as usize;
                        let bz = (cz + dz) as usize;

                        // Don't overwrite the trunk
                        if bx == local_x && bz == local_z && by >= trunk_base && by <= trunk_top {
                            continue;
                        }

                        // Only place leaves in air (don't overwrite terrain)
                        if chunk.get_block(bx, by, bz) == BlockType::Air {
                            chunk.set_block(bx, by, bz, BlockType::Leaves);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::CHUNK_VOLUME;

    #[test]
    fn test_terrain_config_default() {
        let config = TerrainConfig::default();
        assert_eq!(config.seed, 12345);
        assert_eq!(config.base_height, 32.0);
        assert_eq!(config.height_scale, 16.0);
        assert_eq!(config.frequency, 0.02);
        assert_eq!(config.octaves, 4);
        assert!((config.tree_density - 0.02).abs() < f64::EPSILON);
    }

    #[test]
    fn test_terrain_generation_creates_surface() {
        let config = TerrainConfig::default();
        let mut chunk = Chunk::new(IVec3::new(0, 2, 0));  // Chunk at y=32 (surface level)

        generate_chunk_terrain(&mut chunk, &config);

        // At surface level, we should have a mix of air above and blocks below
        let mut has_air = false;
        let mut has_blocks = false;

        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    let block = chunk.get_block(x, y, z);
                    if block == BlockType::Air {
                        has_air = true;
                    } else {
                        has_blocks = true;
                    }
                }
            }
        }

        assert!(has_air, "Surface chunk should have air above terrain");
        assert!(has_blocks, "Surface chunk should have solid blocks");
    }

    #[test]
    fn test_terrain_generation_underground_chunk() {
        let config = TerrainConfig::default();
        let mut chunk = Chunk::new(IVec3::new(0, 0, 0));  // Chunk at y=0 (underground)

        generate_chunk_terrain(&mut chunk, &config);

        // Underground chunks should be mostly solid
        let mut solid_count = 0;
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    if chunk.get_block(x, y, z) != BlockType::Air {
                        solid_count += 1;
                    }
                }
            }
        }

        // Should be mostly stone
        assert!(
            solid_count > CHUNK_VOLUME * 95 / 100,
            "Underground chunk should be >95% solid, was {}%",
            solid_count * 100 / CHUNK_VOLUME
        );
    }

    #[test]
    fn test_terrain_generation_sky_chunk() {
        let config = TerrainConfig::default();
        let mut chunk = Chunk::new(IVec3::new(0, 5, 0));  // Chunk at y=80 (high in sky)

        generate_chunk_terrain(&mut chunk, &config);

        // Sky chunks should be all air
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(
                        chunk.get_block(x, y, z),
                        BlockType::Air,
                        "Sky chunk should be all air at ({}, {}, {})",
                        x, y, z
                    );
                }
            }
        }
    }

    #[test]
    fn test_terrain_has_grass_layer() {
        let config = TerrainConfig::default();
        let mut chunk = Chunk::new(IVec3::new(0, 2, 0));  // Surface level

        generate_chunk_terrain(&mut chunk, &config);

        // Should have at least some grass blocks
        let mut has_grass = false;
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    if chunk.get_block(x, y, z) == BlockType::Grass {
                        has_grass = true;
                        break;
                    }
                }
            }
        }

        assert!(has_grass, "Surface chunk should have grass blocks");
    }

    #[test]
    fn test_terrain_has_dirt_below_grass() {
        let config = TerrainConfig::default();
        let mut chunk = Chunk::new(IVec3::new(0, 2, 0));  // Surface level

        generate_chunk_terrain(&mut chunk, &config);

        // Find a grass block and verify dirt is below it
        for x in 0..CHUNK_SIZE {
            for y in 1..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    if chunk.get_block(x, y, z) == BlockType::Grass {
                        // Block below should be dirt (or stone if at layer boundary)
                        let below = chunk.get_block(x, y - 1, z);
                        assert!(
                            below == BlockType::Dirt || below == BlockType::Stone,
                            "Block below grass should be dirt or stone, was {:?}",
                            below
                        );
                        return;  // Found and verified
                    }
                }
            }
        }
    }

    #[test]
    fn test_terrain_deterministic() {
        let config = TerrainConfig::default();

        // Generate same chunk twice with same config
        let mut chunk1 = Chunk::new(IVec3::new(5, 2, 3));
        let mut chunk2 = Chunk::new(IVec3::new(5, 2, 3));

        generate_chunk_terrain(&mut chunk1, &config);
        generate_chunk_terrain(&mut chunk2, &config);

        // Should be identical
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(
                        chunk1.get_block(x, y, z),
                        chunk2.get_block(x, y, z),
                        "Same seed should produce same terrain at ({}, {}, {})",
                        x, y, z
                    );
                }
            }
        }
    }

    #[test]
    fn test_different_seeds_produce_different_terrain() {
        let config1 = TerrainConfig { seed: 12345, ..Default::default() };
        let config2 = TerrainConfig { seed: 54321, ..Default::default() };

        let mut chunk1 = Chunk::new(IVec3::new(0, 2, 0));
        let mut chunk2 = Chunk::new(IVec3::new(0, 2, 0));

        generate_chunk_terrain(&mut chunk1, &config1);
        generate_chunk_terrain(&mut chunk2, &config2);

        // Should be different (at least some blocks)
        let mut differences = 0;
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    if chunk1.get_block(x, y, z) != chunk2.get_block(x, y, z) {
                        differences += 1;
                    }
                }
            }
        }

        assert!(
            differences > 0,
            "Different seeds should produce different terrain"
        );
    }

    #[test]
    fn test_sea_level_config() {
        let config = TerrainConfig::default();
        assert_eq!(config.sea_level, 28, "Default sea_level should be 28");

        let custom = TerrainConfig {
            sea_level: 50,
            ..Default::default()
        };
        assert_eq!(custom.sea_level, 50, "Custom sea_level should be respected");
    }

    #[test]
    fn test_water_fills_below_sea_level() {
        // Use a high sea_level so air blocks at the surface become water
        let config = TerrainConfig {
            sea_level: 60,
            ..Default::default()
        };
        // Chunk at y=2 covers world_y 32..47, which is below sea_level=60
        let mut chunk = Chunk::new(IVec3::new(0, 2, 0));
        generate_chunk_terrain(&mut chunk, &config);

        let mut has_water = false;
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    let block = chunk.get_block(x, y, z);
                    if block == BlockType::Water {
                        has_water = true;
                        // Water should only appear where terrain is absent
                        let world_y = chunk.world_position().y + y as i32;
                        assert!(
                            world_y <= config.sea_level,
                            "Water found above sea_level at world_y={}",
                            world_y
                        );
                    }
                }
            }
        }

        assert!(has_water, "Should have water blocks where air is below sea_level");
    }

    #[test]
    fn test_no_water_above_sea_level() {
        let config = TerrainConfig {
            sea_level: 28,
            ..Default::default()
        };
        // Chunk at y=5 covers world_y 80..95, well above sea_level=28
        let mut chunk = Chunk::new(IVec3::new(0, 5, 0));
        generate_chunk_terrain(&mut chunk, &config);

        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_ne!(
                        chunk.get_block(x, y, z),
                        BlockType::Water,
                        "No water should exist above sea_level at local ({}, {}, {})",
                        x, y, z
                    );
                }
            }
        }
    }

    #[test]
    fn test_water_does_not_replace_solid() {
        // Set sea_level very high so everything below it could become water
        let config = TerrainConfig {
            sea_level: 200,
            ..Default::default()
        };
        // Underground chunk: world_y 0..15, should be all solid (stone)
        let mut chunk = Chunk::new(IVec3::new(0, 0, 0));
        generate_chunk_terrain(&mut chunk, &config);

        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    let block = chunk.get_block(x, y, z);
                    // Underground blocks should remain solid — water only replaces air
                    assert_ne!(
                        block,
                        BlockType::Water,
                        "Water should not replace solid blocks at ({}, {}, {}), found {:?}",
                        x, y, z, block
                    );
                }
            }
        }
    }

    #[test]
    fn test_caves_dont_break_surface() {
        let config = TerrainConfig::default();
        let mut chunk = Chunk::new(IVec3::new(0, 2, 0));  // Surface level

        generate_chunk_terrain(&mut chunk, &config);

        // Find grass blocks and remember their positions
        let mut grass_positions = Vec::new();
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    if chunk.get_block(x, y, z) == BlockType::Grass {
                        grass_positions.push((x, y, z));
                    }
                }
            }
        }

        // Generate caves
        generate_caves(&mut chunk, &config);

        // Grass blocks should still be grass (surface protection)
        for (x, y, z) in &grass_positions {
            assert_eq!(
                chunk.get_block(*x, *y, *z),
                BlockType::Grass,
                "Cave generation should not remove grass at ({}, {}, {})",
                x, y, z
            );
        }
    }

    // ========================================================================
    // Tree generation tests
    // ========================================================================

    /// Helper: generate a surface chunk with terrain + caves + trees.
    fn make_surface_chunk_with_trees(config: &TerrainConfig, chunk_pos: IVec3) -> Chunk {
        let mut chunk = Chunk::new(chunk_pos);
        generate_chunk_terrain(&mut chunk, config);
        generate_caves(&mut chunk, config);
        generate_trees(&mut chunk, config);
        chunk
    }

    #[test]
    fn test_trees_appear_on_surface_chunks() {
        // Use a higher density so we're very likely to get at least one tree
        let config = TerrainConfig {
            tree_density: 0.15,
            ..Default::default()
        };
        let chunk = make_surface_chunk_with_trees(&config, IVec3::new(0, 2, 0));

        let mut has_wood = false;
        let mut has_leaves = false;
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    match chunk.get_block(x, y, z) {
                        BlockType::Wood => has_wood = true,
                        BlockType::Leaves => has_leaves = true,
                        _ => {}
                    }
                }
            }
        }

        assert!(has_wood, "Surface chunk with trees should have Wood blocks");
        assert!(has_leaves, "Surface chunk with trees should have Leaves blocks");
    }

    #[test]
    fn test_tree_generation_deterministic() {
        let config = TerrainConfig {
            tree_density: 0.10,
            ..Default::default()
        };

        let chunk1 = make_surface_chunk_with_trees(&config, IVec3::new(3, 2, 5));
        let chunk2 = make_surface_chunk_with_trees(&config, IVec3::new(3, 2, 5));

        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(
                        chunk1.get_block(x, y, z),
                        chunk2.get_block(x, y, z),
                        "Tree generation must be deterministic at ({}, {}, {})",
                        x, y, z
                    );
                }
            }
        }
    }

    #[test]
    fn test_trees_do_not_generate_in_sky_chunks() {
        let config = TerrainConfig {
            tree_density: 1.0, // max density — should still produce nothing in the sky
            ..Default::default()
        };
        let chunk = make_surface_chunk_with_trees(&config, IVec3::new(0, 5, 0));

        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    let block = chunk.get_block(x, y, z);
                    assert_eq!(
                        block,
                        BlockType::Air,
                        "Sky chunk should have no tree blocks at ({}, {}, {}), found {:?}",
                        x, y, z, block
                    );
                }
            }
        }
    }

    #[test]
    fn test_tree_block_types_are_correct() {
        // Generate with high density to guarantee trees
        let config = TerrainConfig {
            tree_density: 0.15,
            ..Default::default()
        };
        let chunk = make_surface_chunk_with_trees(&config, IVec3::new(0, 2, 0));

        // Find a Wood block — it should have either Wood or Leaves above it,
        // and the column below should eventually reach Grass.
        for x in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                // Find lowest Wood in this column (trunk base)
                let mut trunk_base: Option<usize> = None;
                for y in 0..CHUNK_SIZE {
                    if chunk.get_block(x, y, z) == BlockType::Wood {
                        trunk_base = Some(y);
                        break;
                    }
                }

                let trunk_base = match trunk_base {
                    Some(y) => y,
                    None => continue,
                };

                // Block directly below trunk base should be Grass (surface)
                if trunk_base > 0 {
                    let below = chunk.get_block(x, trunk_base - 1, z);
                    assert_eq!(
                        below,
                        BlockType::Grass,
                        "Block below trunk at ({}, {}, {}) should be Grass, found {:?}",
                        x, trunk_base - 1, z, below
                    );
                }

                // Walk up — expect continuous Wood then the column may end
                let mut y = trunk_base;
                while y < CHUNK_SIZE && chunk.get_block(x, y, z) == BlockType::Wood {
                    y += 1;
                }
                let trunk_top = y - 1;
                let trunk_height = trunk_top - trunk_base + 1;

                assert!(
                    (4..=6).contains(&trunk_height),
                    "Trunk height should be 4-6, got {} at column ({}, {})",
                    trunk_height, x, z
                );

                return; // Verified one tree, that's enough
            }
        }

        panic!("Expected to find at least one tree trunk in the chunk");
    }

    #[test]
    fn test_trees_do_not_generate_with_zero_density() {
        let config = TerrainConfig {
            tree_density: 0.0,
            ..Default::default()
        };
        let mut chunk_before = Chunk::new(IVec3::new(0, 2, 0));
        generate_chunk_terrain(&mut chunk_before, &config);
        generate_caves(&mut chunk_before, &config);

        let mut chunk_after = Chunk::new(IVec3::new(0, 2, 0));
        generate_chunk_terrain(&mut chunk_after, &config);
        generate_caves(&mut chunk_after, &config);
        generate_trees(&mut chunk_after, &config);

        // With density=0, generate_trees should change nothing
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(
                        chunk_before.get_block(x, y, z),
                        chunk_after.get_block(x, y, z),
                        "Zero density should produce no trees at ({}, {}, {})",
                        x, y, z
                    );
                }
            }
        }
    }
}
