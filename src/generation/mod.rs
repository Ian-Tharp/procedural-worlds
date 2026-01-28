//! Procedural generation systems - terrain, vegetation, structures
//!
//! This module will contain:
//! - Terrain generation using noise functions
//! - Biome distribution
//! - Vegetation placement
//! - Structure generation

use bevy::prelude::*;
use noise::{NoiseFn, Perlin, Simplex};

use crate::world::{BlockType, Chunk, CHUNK_SIZE};

/// Configuration for terrain generation
#[derive(Resource)]
pub struct TerrainConfig {
    /// World seed
    pub seed: u32,
    /// Base terrain height
    pub base_height: f64,
    /// Terrain height variation
    pub height_scale: f64,
    /// Noise frequency (lower = smoother terrain)
    pub frequency: f64,
    /// Number of noise octaves
    pub octaves: usize,
}

impl Default for TerrainConfig {
    fn default() -> Self {
        Self {
            seed: 12345,
            base_height: 32.0,
            height_scale: 16.0,
            frequency: 0.02,
            octaves: 4,
        }
    }
}

/// Generates terrain for a chunk using simplex noise
pub fn generate_chunk_terrain(chunk: &mut Chunk, config: &TerrainConfig) {
    let noise = Simplex::new(config.seed);
    let world_pos = chunk.world_position();

    for local_x in 0..CHUNK_SIZE {
        for local_z in 0..CHUNK_SIZE {
            let world_x = world_pos.x + local_x as i32;
            let world_z = world_pos.z + local_z as i32;

            // Generate height using fractal noise (multiple octaves)
            let mut height = 0.0;
            let mut amplitude = 1.0;
            let mut frequency = config.frequency;

            for _ in 0..config.octaves {
                height += noise.get([
                    world_x as f64 * frequency,
                    world_z as f64 * frequency,
                ]) * amplitude;
                amplitude *= 0.5;
                frequency *= 2.0;
            }

            // Convert to block height
            let terrain_height = (config.base_height + height * config.height_scale) as i32;

            // Fill column with appropriate blocks
            for local_y in 0..CHUNK_SIZE {
                let world_y = world_pos.y + local_y as i32;

                let block = if world_y > terrain_height {
                    BlockType::Air
                } else if world_y == terrain_height {
                    BlockType::Grass
                } else if world_y > terrain_height - 4 {
                    BlockType::Dirt
                } else {
                    BlockType::Stone
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
    let world_pos = chunk.world_position();

    // Higher threshold = fewer caves (0.7 means only top 15% of noise creates caves)
    let cave_threshold = 0.7;
    // Protect this many blocks below the surface from caves
    let surface_protection = 5;

    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            let world_x = world_pos.x + x as i32;
            let world_z = world_pos.z + z as i32;

            // Calculate terrain height at this column (same as terrain generation)
            let mut height = 0.0;
            let mut amplitude = 1.0;
            let mut frequency = config.frequency;
            for _ in 0..config.octaves {
                height += terrain_noise.get([
                    world_x as f64 * frequency,
                    world_z as f64 * frequency,
                ]) * amplitude;
                amplitude *= 0.5;
                frequency *= 2.0;
            }
            let terrain_height = (config.base_height + height * config.height_scale) as i32;

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
}
