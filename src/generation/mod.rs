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
use biome::{biome_at, BiomeParams, BiomeType};

pub use self::OreSpawnConfig as OreConfig;

/// Configuration for terrain generation
#[derive(Resource, Clone)]
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
    /// Whether biome boundary blending is enabled.
    ///
    /// When enabled, terrain generation parameters (amplitude, frequency)
    /// are smoothly interpolated at biome boundaries using distance-weighted
    /// sampling of nearby biomes.
    pub blend_enabled: bool,
    /// Distance in blocks over which biome parameters blend at boundaries.
    ///
    /// Larger values produce wider, smoother transitions between biomes.
    /// A value of 32.0 creates ~32-block-wide transition zones.
    pub blend_distance: f64,
    /// Noise frequency for transition zone modulation.
    ///
    /// A separate noise layer at this frequency is used to warp blend
    /// weights at biome boundaries, producing irregular, organic-looking
    /// edges rather than perfectly smooth geometric gradients.
    /// Default: 0.08. Higher values create more jagged boundaries.
    pub transition_noise_scale: f64,
    /// Amplitude of the transition noise modulation (0.0–1.0).
    ///
    /// Controls how much the noise mask can distort the blend boundary.
    /// At 0.0, transitions are purely distance-based (geometric).
    /// At 1.0, noise can shift the effective boundary by up to one full
    /// `blend_distance`. Default: 0.45.
    pub transition_noise_amplitude: f64,

    // ── Cave Generation ──

    /// Whether cave generation is enabled.
    pub caves_enabled: bool,
    /// Noise threshold for cave generation (0.0–1.0).
    ///
    /// Higher values = fewer, smaller caves.
    /// 0.5 = lots of caves, 0.8 = sparse caves.
    /// Default: 0.7 (moderate caves).
    pub cave_threshold: f64,
    /// How many blocks below the surface are protected from caves.
    ///
    /// Prevents caves from breaking through grass/dirt layer.
    /// Default: 5 blocks.
    pub cave_surface_protection: i32,
    /// Noise frequency for cave generation.
    ///
    /// Lower = larger cave systems, Higher = smaller tunnels.
    /// Default: 0.05.
    pub cave_frequency: f64,

    // ── Vegetation ──

    /// Cactus density multiplier (relative to biome defaults).
    ///
    /// Similar to tree_density but for desert cacti.
    /// Default: 1.0 (use biome defaults).
    pub cactus_density_multiplier: f64,
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
            blend_enabled: true,
            blend_distance: 32.0,
            transition_noise_scale: 0.08,
            transition_noise_amplitude: 0.45,
            caves_enabled: true,
            cave_threshold: 0.7,
            cave_surface_protection: 5,
            cave_frequency: 0.05,
            cactus_density_multiplier: 1.0,
        }
    }
}

// ============================================================================
// BIOME BOUNDARY BLENDING
// ============================================================================

/// Hermite smoothstep interpolation for smooth blending transitions.
///
/// Maps input `t` in [0, 1] to a smooth S-curve that has zero first-derivative
/// at both endpoints, eliminating visible seams in terrain transitions.
fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Sample a transition noise value at a world coordinate.
///
/// Uses a dedicated Perlin noise instance (seeded independently from terrain
/// and biome noise) to produce a modulation factor in `[-amplitude, +amplitude]`.
/// This value is added to blend weights, making biome boundaries wavy and
/// organic instead of smooth geometric bands.
fn transition_noise_at(
    world_x: i32,
    world_z: i32,
    transition_noise: &Perlin,
    config: &TerrainConfig,
) -> f64 {
    if config.transition_noise_amplitude <= 0.0 {
        return 0.0;
    }
    let raw = transition_noise.get([
        world_x as f64 * config.transition_noise_scale,
        world_z as f64 * config.transition_noise_scale,
    ]);
    // raw is in [-1, 1]; scale by amplitude
    raw * config.transition_noise_amplitude
}

/// Sample biomes around a world coordinate and return distance-weighted
/// blended terrain parameters.
///
/// When blending is disabled (or `blend_distance <= 0`), returns the raw
/// parameters of the biome at the exact query coordinate — identical to
/// the original single-biome lookup.
///
/// When enabled, nine sample points (center + 8 surrounding at `blend_distance`)
/// are evaluated.  Each sample's biome parameters are weighted by a smoothstep
/// falloff based on distance.  A Perlin noise mask modulates the weights,
/// producing irregular, organic boundary edges rather than perfectly smooth
/// geometric gradients.
///
/// Returns `(blended_amplitude, blended_frequency, primary_biome)`.
fn blended_biome_params(
    world_x: i32,
    world_z: i32,
    biome_noise: &Simplex,
    transition_noise: &Perlin,
    config: &TerrainConfig,
) -> (f64, f64, BiomeType) {
    let primary_biome = biome_at(world_x, world_z, biome_noise, config.biome_scale);

    if !config.blend_enabled || config.blend_distance <= 0.0 {
        let params = primary_biome.params();
        return (params.terrain_amplitude, params.terrain_frequency, primary_biome);
    }

    let bd = config.blend_distance;

    // Sample at center + 8 surrounding points (cardinal + diagonal) at blend_distance.
    // This gives good coverage of nearby biome boundaries without excessive cost.
    let offsets: &[(f64, f64)] = &[
        (0.0, 0.0),                         // center
        (-bd, 0.0), (bd, 0.0),              // W, E
        (0.0, -bd), (0.0, bd),              // N, S
        (-bd, -bd), (bd, -bd),              // NW, NE
        (-bd, bd),  (bd, bd),               // SW, SE
    ];

    // Maximum possible distance among samples (diagonal corner)
    let max_dist = bd * (2.0_f64).sqrt();

    // Noise modulation for this coordinate — shifts blend weights to create
    // irregular boundary edges.
    let noise_offset = transition_noise_at(world_x, world_z, transition_noise, config);

    let mut total_weight = 0.0;
    let mut blended_amplitude = 0.0;
    let mut blended_frequency = 0.0;

    for &(dx, dz) in offsets {
        let sx = world_x as f64 + dx;
        let sz = world_z as f64 + dz;
        let biome = biome_at(sx as i32, sz as i32, biome_noise, config.biome_scale);
        let params = biome.params();

        let dist = (dx * dx + dz * dz).sqrt();
        // Noise offset warps the effective distance, creating irregular edges.
        // Clamped to [0, 1] so weights stay valid.
        let t = (1.0 - (dist / max_dist) + noise_offset).clamp(0.0, 1.0);
        let weight = smoothstep(t);

        blended_amplitude += params.terrain_amplitude * weight;
        blended_frequency += params.terrain_frequency * weight;
        total_weight += weight;
    }

    if total_weight > 0.0 {
        blended_amplitude /= total_weight;
        blended_frequency /= total_weight;
    } else {
        let params = primary_biome.params();
        blended_amplitude = params.terrain_amplitude;
        blended_frequency = params.terrain_frequency;
    }

    (blended_amplitude, blended_frequency, primary_biome)
}

/// Compute the blend factor between the primary biome and its nearest
/// differing neighbor at a world (x, z) position.
///
/// Returns `(secondary_biome, blend_factor)` where `blend_factor` is 0.0
/// when deep inside the primary biome and approaches 1.0 at the boundary.
/// The transition noise mask modulates the factor for organic edges.
///
/// If all nearby samples belong to the same biome, returns `(primary, 0.0)`.
fn biome_blend_factor(
    world_x: i32,
    world_z: i32,
    primary_biome: BiomeType,
    biome_noise: &Simplex,
    transition_noise: &Perlin,
    config: &TerrainConfig,
) -> (BiomeType, f64) {
    if !config.blend_enabled || config.blend_distance <= 0.0 {
        return (primary_biome, 0.0);
    }

    let bd = config.blend_distance;

    // Sample cardinal and diagonal neighbors to find the nearest differing biome
    let sample_offsets: &[(f64, f64)] = &[
        (-bd, 0.0), (bd, 0.0),
        (0.0, -bd), (0.0, bd),
        (-bd, -bd), (bd, -bd),
        (-bd, bd),  (bd, bd),
    ];

    let max_dist = bd * (2.0_f64).sqrt();

    let mut best_secondary = primary_biome;
    let mut best_weight = 0.0_f64;

    for &(dx, dz) in sample_offsets {
        let sx = (world_x as f64 + dx) as i32;
        let sz = (world_z as f64 + dz) as i32;
        let biome = biome_at(sx, sz, biome_noise, config.biome_scale);

        if biome == primary_biome {
            continue;
        }

        let dist = (dx * dx + dz * dz).sqrt();
        // Closer neighbors get higher influence
        let raw_t = 1.0 - (dist / max_dist);
        let weight = smoothstep(raw_t.clamp(0.0, 1.0));

        if weight > best_weight {
            best_weight = weight;
            best_secondary = biome;
        }
    }

    if best_secondary == primary_biome {
        return (primary_biome, 0.0);
    }

    // Modulate the blend factor with transition noise for organic edges.
    let noise_mod = transition_noise_at(world_x, world_z, transition_noise, config);
    // best_weight is the proximity to the nearest different biome.
    // Scale it to produce a blend_factor in [0, 1].
    // The noise_mod shifts the factor, making the boundary wavy.
    let blend_factor = (best_weight + noise_mod * 0.5).clamp(0.0, 1.0);

    (best_secondary, blend_factor)
}

/// Deterministic hash for block palette blending decisions.
///
/// Given a world position and seed, returns a value in [0.0, 1.0) used to
/// probabilistically select between primary and secondary biome block palettes
/// at biome boundaries.
fn block_blend_hash(world_x: i32, world_y: i32, world_z: i32, seed: u32) -> f64 {
    let mut hasher = DefaultHasher::new();
    "block_blend".hash(&mut hasher);
    seed.hash(&mut hasher);
    world_x.hash(&mut hasher);
    world_y.hash(&mut hasher);
    world_z.hash(&mut hasher);
    let h = hasher.finish();
    (h as f64) / (u64::MAX as f64)
}

/// Select the block palette for a column, blending between biomes at boundaries.
///
/// Deep inside a biome, returns that biome's params unchanged. At boundaries,
/// probabilistically selects the secondary biome's blocks based on the blend
/// factor and a deterministic hash. This creates a scattered, natural-looking
/// mix of block types at biome edges.
///
/// Returns the `BiomeParams` to use for this column's block placement.
fn blended_block_palette(
    world_x: i32,
    world_z: i32,
    primary_biome: BiomeType,
    biome_noise: &Simplex,
    transition_noise: &Perlin,
    config: &TerrainConfig,
) -> BiomeParams {
    let (secondary_biome, blend_factor) = biome_blend_factor(
        world_x, world_z, primary_biome, biome_noise, transition_noise, config,
    );

    if blend_factor <= 0.0 || secondary_biome == primary_biome {
        return primary_biome.params();
    }

    // Use a deterministic hash to decide whether this column uses the
    // secondary biome's block palette.  The hash varies per-column so
    // adjacent columns can independently choose, creating scattered patches.
    let roll = block_blend_hash(world_x, 0, world_z, config.seed);

    if roll < blend_factor {
        secondary_biome.params()
    } else {
        primary_biome.params()
    }
}

// ============================================================================
// TERRAIN HEIGHT CALCULATION
// ============================================================================

/// Calculate the terrain height and biome at a world (x, z) column.
///
/// Shared by terrain generation and cave generation so both agree on
/// where the surface is.
///
/// When biome blending is enabled in `config`, terrain parameters
/// (amplitude, frequency) are smoothly interpolated at biome boundaries
/// with noise-modulated weights for organic-looking edges.
/// The primary biome (at the exact coordinate) is still returned for
/// block-palette selection; block types are blended separately via
/// [`blended_block_palette`].
fn terrain_column(
    world_x: i32,
    world_z: i32,
    terrain_noise: &Simplex,
    biome_noise: &Simplex,
    transition_noise: &Perlin,
    config: &TerrainConfig,
) -> (i32, BiomeType) {
    let (blended_amplitude, blended_frequency, biome) =
        blended_biome_params(world_x, world_z, biome_noise, transition_noise, config);

    let mut height = 0.0;
    let mut octave_amp = 1.0;
    let mut freq = blended_frequency;

    for _ in 0..config.octaves {
        height += terrain_noise.get([
            world_x as f64 * freq,
            world_z as f64 * freq,
        ]) * octave_amp;
        octave_amp *= 0.5;
        freq *= 2.0;
    }

    let terrain_height = (config.base_height + height * blended_amplitude) as i32;
    (terrain_height, biome)
}

/// Generates terrain for a chunk using simplex noise and biome parameters.
///
/// When biome blending is enabled, terrain shape (amplitude, frequency) is
/// smoothly interpolated at biome boundaries with noise-modulated weights,
/// and block palettes are probabilistically mixed at boundary columns for
/// natural-looking surface transitions.
pub fn generate_chunk_terrain(chunk: &mut Chunk, config: &TerrainConfig) {
    let terrain_noise = Simplex::new(config.seed);
    let biome_noise = Simplex::new(config.seed.wrapping_add(config.biome_seed_offset));
    // Transition noise uses a different seed offset to stay decorrelated
    // from both terrain and biome noise.
    let transition_noise = Perlin::new(config.seed.wrapping_add(config.biome_seed_offset + 500));
    let world_pos = chunk.world_position();

    for local_x in 0..CHUNK_SIZE {
        for local_z in 0..CHUNK_SIZE {
            let world_x = world_pos.x + local_x as i32;
            let world_z = world_pos.z + local_z as i32;

            let (terrain_height, biome) =
                terrain_column(world_x, world_z, &terrain_noise, &biome_noise, &transition_noise, config);

            // Select block palette: at biome boundaries, probabilistically
            // pick between primary and secondary biome palettes for natural
            // surface block transitions.
            let params = blended_block_palette(
                world_x, world_z, biome, &biome_noise, &transition_noise, config,
            );
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
    // Skip if caves are disabled
    if !config.caves_enabled {
        return;
    }

    let noise = Perlin::new(config.seed.wrapping_add(1000));
    let terrain_noise = Simplex::new(config.seed);
    let biome_noise = Simplex::new(config.seed.wrapping_add(config.biome_seed_offset));
    let transition_noise = Perlin::new(config.seed.wrapping_add(config.biome_seed_offset + 500));
    let world_pos = chunk.world_position();

    let cave_threshold = config.cave_threshold;
    let surface_protection = config.cave_surface_protection;
    let cave_freq = config.cave_frequency;

    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            let world_x = world_pos.x + x as i32;
            let world_z = world_pos.z + z as i32;

            // Calculate terrain height at this column (biome-aware, same as terrain generation)
            let (terrain_height, _biome) =
                terrain_column(world_x, world_z, &terrain_noise, &biome_noise, &transition_noise, config);

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
                    world_x as f64 * cave_freq,
                    world_y as f64 * cave_freq,
                    world_z as f64 * cave_freq,
                ]);

                if cave_noise > cave_threshold {
                    chunk.set_block(x, y, z, BlockType::Air);
                }
            }
        }
    }
}

/// Deterministic hash for cactus placement decisions.
/// Returns a value in [0.0, 1.0) based on world position and seed.
fn cactus_hash(world_x: i32, world_z: i32, seed: u32) -> f64 {
    let mut hasher = DefaultHasher::new();
    "cactus_placement".hash(&mut hasher);
    seed.hash(&mut hasher);
    world_x.hash(&mut hasher);
    world_z.hash(&mut hasher);
    let h = hasher.finish();
    (h as f64) / (u64::MAX as f64)
}

/// Deterministic hash to choose cactus height (2–4) for a given position.
fn cactus_height_hash(world_x: i32, world_z: i32, seed: u32) -> usize {
    let mut hasher = DefaultHasher::new();
    "cactus_height".hash(&mut hasher);
    seed.hash(&mut hasher);
    world_x.hash(&mut hasher);
    world_z.hash(&mut hasher);
    let h = hasher.finish();
    // Map to range 2..=4
    2 + (h % 3) as usize
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

/// Place cacti on desert surfaces within a chunk.
///
/// For each (x, z) column, finds the highest SandDunes block (the surface),
/// then probabilistically places a cactus: a vertical column of Cactus blocks
/// 2–4 blocks tall.
///
/// Cactus density is determined per-column by the biome at that position,
/// scaled by `config.tree_density` relative to the default density (reusing
/// the same scaling mechanism as trees for consistency).
/// Cacti are only placed on SandDunes or Sand blocks and when the entire
/// structure fits within the chunk.
pub fn generate_cacti(chunk: &mut Chunk, config: &TerrainConfig) {
    let biome_noise = Simplex::new(config.seed.wrapping_add(config.biome_seed_offset));
    let world_pos = chunk.world_position();

    for local_x in 0..CHUNK_SIZE {
        for local_z in 0..CHUNK_SIZE {
            let world_x = world_pos.x + local_x as i32;
            let world_z = world_pos.z + local_z as i32;

            // --- Find highest SandDunes or Sand block (scan top-down) ---
            let mut surface_y: Option<usize> = None;
            for local_y in (0..CHUNK_SIZE).rev() {
                let block = chunk.get_block(local_x, local_y, local_z);
                if block == BlockType::SandDunes || block == BlockType::Sand {
                    surface_y = Some(local_y);
                    break;
                }
            }

            let surface_y = match surface_y {
                Some(y) => y,
                None => continue, // No sand surface in this column
            };

            // --- Biome-aware cactus density ---
            let biome = biome_at(world_x, world_z, &biome_noise, config.biome_scale);
            let effective_density = biome.params().cactus_density * config.cactus_density_multiplier;

            if effective_density <= 0.0 {
                continue;
            }

            // --- Deterministic spawn decision ---
            let hash_val = cactus_hash(world_x, world_z, config.seed);
            if hash_val >= effective_density {
                continue;
            }

            // --- Choose cactus height (2–4 blocks) ---
            let cactus_height = cactus_height_hash(world_x, world_z, config.seed);

            let cactus_base = surface_y + 1;
            let cactus_top = cactus_base + cactus_height - 1;

            // Check that the whole cactus fits vertically in the chunk
            if cactus_top >= CHUNK_SIZE {
                continue;
            }

            // --- Place cactus (vertical column of Cactus blocks) ---
            for y in cactus_base..=cactus_top {
                // Only place in air (don't overwrite existing blocks)
                if chunk.get_block(local_x, y, local_z) == BlockType::Air {
                    chunk.set_block(local_x, y, local_z, BlockType::Cactus);
                }
            }
        }
    }
}

// ============================================================================
// ORE GENERATION
// ============================================================================

/// Ore spawn configuration - matches data from OreRegistry
/// 
/// This is a lightweight struct that can be cloned and passed
/// to async chunk generation tasks.
#[derive(Debug, Clone)]
pub struct OreSpawnConfig {
    /// Unique identifier
    pub id: String,
    /// Which block type to place
    pub block_type: BlockType,
    /// Minimum Y level for spawning
    pub min_y: i32,
    /// Maximum Y level for spawning
    pub max_y: i32,
    /// Average blocks per vein
    pub vein_size: u32,
    /// Spawn frequency (0.0-1.0, typical: 0.001-0.05)
    pub frequency: f64,
}

/// Generate ores in a chunk based on ore spawn configurations.
///
/// For each ore type, uses 3D noise to determine placement locations,
/// then grows veins by replacing Stone blocks with ore blocks.
pub fn generate_ores(chunk: &mut Chunk, config: &TerrainConfig, ore_configs: &[OreSpawnConfig]) {
    let world_pos = chunk.world_position();
    
    for (ore_index, ore) in ore_configs.iter().enumerate() {
        // Each ore type gets its own noise with a unique seed offset
        let ore_seed = config.seed.wrapping_add(5000 + ore_index as u32 * 100);
        let noise = Perlin::new(ore_seed);
        
        // Vein center noise (determines where veins start)
        let vein_noise = Simplex::new(ore_seed.wrapping_add(1));
        
        // Frequency for finding vein centers
        let vein_freq = 0.08; // Controls vein spacing
        
        for x in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                let world_x = world_pos.x + x as i32;
                let world_z = world_pos.z + z as i32;
                
                for y in 0..CHUNK_SIZE {
                    let world_y = world_pos.y + y as i32;
                    
                    // Skip if outside Y range
                    if world_y < ore.min_y || world_y > ore.max_y {
                        continue;
                    }
                    
                    // Skip if not stone (ores only replace stone)
                    if chunk.get_block(x, y, z) != BlockType::Stone {
                        continue;
                    }
                    
                    // 3D noise for vein center detection
                    let center_noise = vein_noise.get([
                        world_x as f64 * vein_freq,
                        world_y as f64 * vein_freq,
                        world_z as f64 * vein_freq,
                    ]);
                    
                    // Only consider as vein center if noise is above threshold
                    // Threshold based on ore frequency
                    let center_threshold = 1.0 - (ore.frequency * 10.0).min(0.8);
                    if center_noise < center_threshold {
                        continue;
                    }
                    
                    // This is a vein center - grow the vein
                    grow_ore_vein(chunk, x, y, z, ore.block_type, ore.vein_size, &noise, ore_seed);
                }
            }
        }
    }
}

/// Grow an ore vein from a center point, replacing stone with ore.
fn grow_ore_vein(
    chunk: &mut Chunk,
    center_x: usize,
    center_y: usize,
    center_z: usize,
    ore_type: BlockType,
    target_size: u32,
    noise: &Perlin,
    seed: u32,
) {
    let mut placed = 0u32;
    let max_radius = (target_size as f64).sqrt().ceil() as i32 + 1;
    
    // Place center block
    chunk.set_block(center_x, center_y, center_z, ore_type);
    placed += 1;
    
    // Grow outward in a roughly spherical pattern
    for dx in -max_radius..=max_radius {
        for dy in -max_radius..=max_radius {
            for dz in -max_radius..=max_radius {
                if placed >= target_size {
                    return;
                }
                
                let nx = center_x as i32 + dx;
                let ny = center_y as i32 + dy;
                let nz = center_z as i32 + dz;
                
                // Skip if out of chunk bounds
                if nx < 0 || nx >= CHUNK_SIZE as i32 ||
                   ny < 0 || ny >= CHUNK_SIZE as i32 ||
                   nz < 0 || nz >= CHUNK_SIZE as i32 {
                    continue;
                }
                
                // Skip center (already placed)
                if dx == 0 && dy == 0 && dz == 0 {
                    continue;
                }
                
                // Calculate distance-based probability
                let dist_sq = (dx * dx + dy * dy + dz * dz) as f64;
                let max_dist_sq = (max_radius * max_radius) as f64;
                let dist_factor = 1.0 - (dist_sq / max_dist_sq).sqrt();
                
                // Add noise variation
                let noise_val = noise.get([
                    (center_x as i32 + dx) as f64 * 0.5,
                    (center_y as i32 + dy) as f64 * 0.5,
                    (center_z as i32 + dz) as f64 * 0.5,
                ]) * 0.5 + 0.5;
                
                // Probability based on distance and noise
                let prob = dist_factor * noise_val;
                
                // Deterministic check using position hash
                let hash = ore_placement_hash(
                    nx, ny, nz, seed
                );
                let hash_prob = hash as f64 / u64::MAX as f64;
                
                if hash_prob < prob {
                    let ux = nx as usize;
                    let uy = ny as usize;
                    let uz = nz as usize;
                    
                    // Only replace stone
                    if chunk.get_block(ux, uy, uz) == BlockType::Stone {
                        chunk.set_block(ux, uy, uz, ore_type);
                        placed += 1;
                    }
                }
            }
        }
    }
}

/// Deterministic hash for ore placement decisions
fn ore_placement_hash(x: i32, y: i32, z: i32, seed: u32) -> u64 {
    let mut hasher = DefaultHasher::new();
    "ore_vein".hash(&mut hasher);
    seed.hash(&mut hasher);
    x.hash(&mut hasher);
    y.hash(&mut hasher);
    z.hash(&mut hasher);
    hasher.finish()
}

/// Map ore ID to BlockType.
/// 
/// Note: This is a temporary bridge until BlockType becomes data-driven.
/// Currently only the 4 built-in ores are supported. Custom ores created
/// in the editor won't spawn until we refactor BlockType to be dynamic.
pub fn ore_id_to_block_type(id: &str) -> Option<BlockType> {
    match id {
        "copper_ore" => Some(BlockType::CopperOre),
        "iron_ore" => Some(BlockType::IronOre),
        "silver_ore" => Some(BlockType::SilverOre),
        "gold_ore" => Some(BlockType::GoldOre),
        _ => None, // Custom ores not yet supported
    }
}

/// Convert OreDefinitions from the content system into spawn configs.
/// 
/// This reads from the OreRegistry so changes made in the editor
/// affect world generation (after chunk regeneration).
pub fn ore_configs_from_definitions(definitions: &[crate::content::OreDefinition]) -> Vec<OreSpawnConfig> {
    definitions
        .iter()
        .filter_map(|def| {
            ore_id_to_block_type(&def.id).map(|block_type| OreSpawnConfig {
                id: def.id.clone(),
                block_type,
                min_y: def.generation.min_y,
                max_y: def.generation.max_y,
                vein_size: def.generation.vein_size,
                frequency: def.generation.frequency,
            })
        })
        .collect()
}

/// Get default ore spawn configurations.
/// 
/// DEPRECATED: Use `ore_configs_from_definitions()` with OreRegistry instead.
/// This exists as a fallback when the registry isn't available.
pub fn default_ore_configs() -> Vec<OreSpawnConfig> {
    vec![
        OreSpawnConfig {
            id: "copper_ore".to_string(),
            block_type: BlockType::CopperOre,
            min_y: 16,
            max_y: 96,
            vein_size: 12,
            frequency: 0.035,
        },
        OreSpawnConfig {
            id: "iron_ore".to_string(),
            block_type: BlockType::IronOre,
            min_y: 0,
            max_y: 64,
            vein_size: 8,
            frequency: 0.025,
        },
        OreSpawnConfig {
            id: "silver_ore".to_string(),
            block_type: BlockType::SilverOre,
            min_y: 0,
            max_y: 40,
            vein_size: 5,
            frequency: 0.012,
        },
        OreSpawnConfig {
            id: "gold_ore".to_string(),
            block_type: BlockType::GoldOre,
            min_y: 0,
            max_y: 32,
            vein_size: 4,
            frequency: 0.006,
        },
    ]
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

    /// Helper: generate a surface chunk with terrain + caves + trees + cacti.
    fn make_surface_chunk_with_trees(config: &TerrainConfig, chunk_pos: IVec3) -> Chunk {
        let mut chunk = Chunk::new(chunk_pos);
        generate_chunk_terrain(&mut chunk, config);
        generate_caves(&mut chunk, config);
        generate_trees(&mut chunk, config);
        generate_cacti(&mut chunk, config);
        chunk
    }

    #[test]
    fn test_trees_appear_on_surface_chunks() {
        let config = TerrainConfig {
            tree_density: 0.15,
            ..Default::default()
        };
        let mut has_wood = false;
        let mut has_leaves = false;
        // Try multiple chunk positions since biome selection affects tree density
        for cx in -3i32..4 {
            for cz in -3i32..4 {
                let chunk = make_surface_chunk_with_trees(&config, IVec3::new(cx, 2, cz));
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
                if has_wood && has_leaves { break; }
            }
            if has_wood && has_leaves { break; }
        }
        assert!(has_wood, "Surface chunks should have Wood blocks");
        assert!(has_leaves, "Surface chunks should have Leaves blocks");
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
        let config = TerrainConfig {
            tree_density: 0.15,
            ..Default::default()
        };
        // Try multiple chunk positions since biome selection affects tree density
        for cx in -3i32..4 {
            for cz in -3i32..4 {
                let chunk = make_surface_chunk_with_trees(&config, IVec3::new(cx, 2, cz));
                for x in 0..CHUNK_SIZE {
                    for z in 0..CHUNK_SIZE {
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
                        if trunk_base > 0 {
                            let below = chunk.get_block(x, trunk_base - 1, z);
                            // Surface block may vary by biome (Grass, Mud, PackedDirt, etc.)
                            assert!(
                                below != BlockType::Air && below != BlockType::Water,
                                "Block below trunk at ({}, {}, {}) should be a surface block, found {:?}",
                                x, trunk_base - 1, z, below
                            );
                        }
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
            }
        }
        panic!("Expected to find at least one tree trunk across tested chunks");
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

    // ========================================================================
    // Cactus generation tests
    // ========================================================================

    /// Helper: generate a full pipeline chunk including cacti.
    fn make_surface_chunk_with_cacti(config: &TerrainConfig, chunk_pos: IVec3) -> Chunk {
        let mut chunk = Chunk::new(chunk_pos);
        generate_chunk_terrain(&mut chunk, config);
        generate_caves(&mut chunk, config);
        generate_trees(&mut chunk, config);
        generate_cacti(&mut chunk, config);
        chunk
    }

    /// Find a chunk position that's in a Desert biome (SandDunes surface).
    /// Scans a range of chunk positions to find one where the biome produces
    /// SandDunes on the surface.
    fn find_desert_chunk_pos(config: &TerrainConfig) -> Option<IVec3> {
        let biome_noise = Simplex::new(config.seed.wrapping_add(config.biome_seed_offset));

        for cx in -30..30 {
            for cz in -30..30 {
                let world_x = cx * CHUNK_SIZE as i32;
                let world_z = cz * CHUNK_SIZE as i32;
                let biome = biome_at(world_x, world_z, &biome_noise, config.biome_scale);
                if biome == BiomeType::Desert {
                    return Some(IVec3::new(cx, 2, cz));
                }
            }
        }
        None
    }

    #[test]
    fn test_cacti_appear_in_desert() {
        let config = TerrainConfig::default();
        let chunk_pos = find_desert_chunk_pos(&config)
            .expect("Should find a desert chunk within scan range");

        let chunk = make_surface_chunk_with_cacti(&config, chunk_pos);

        let mut has_cactus = false;
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    if chunk.get_block(x, y, z) == BlockType::Cactus {
                        has_cactus = true;
                        break;
                    }
                }
                if has_cactus { break; }
            }
            if has_cactus { break; }
        }

        // With default density (0.008), it's possible a single chunk has no cacti.
        // Scan multiple desert chunks to be sure.
        if !has_cactus {
            let biome_noise = Simplex::new(config.seed.wrapping_add(config.biome_seed_offset));
            for cx in -30..30 {
                for cz in -30..30 {
                    let world_x = cx * CHUNK_SIZE as i32;
                    let world_z = cz * CHUNK_SIZE as i32;
                    let biome = biome_at(world_x, world_z, &biome_noise, config.biome_scale);
                    if biome == BiomeType::Desert {
                        let c = make_surface_chunk_with_cacti(&config, IVec3::new(cx, 2, cz));
                        for x in 0..CHUNK_SIZE {
                            for y in 0..CHUNK_SIZE {
                                for z in 0..CHUNK_SIZE {
                                    if c.get_block(x, y, z) == BlockType::Cactus {
                                        has_cactus = true;
                                    }
                                }
                            }
                        }
                        if has_cactus { break; }
                    }
                }
                if has_cactus { break; }
            }
        }

        assert!(has_cactus, "Desert biome should produce at least some cacti");
    }

    #[test]
    fn test_cactus_generation_deterministic() {
        let config = TerrainConfig::default();
        let chunk_pos = find_desert_chunk_pos(&config)
            .expect("Should find a desert chunk within scan range");

        let chunk1 = make_surface_chunk_with_cacti(&config, chunk_pos);
        let chunk2 = make_surface_chunk_with_cacti(&config, chunk_pos);

        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(
                        chunk1.get_block(x, y, z),
                        chunk2.get_block(x, y, z),
                        "Cactus generation must be deterministic at ({}, {}, {})",
                        x, y, z
                    );
                }
            }
        }
    }

    #[test]
    fn test_cactus_height_is_2_to_4() {
        let config = TerrainConfig::default();
        let _chunk_pos = find_desert_chunk_pos(&config)
            .expect("Should find a desert chunk within scan range");

        // Scan multiple desert chunks to find a cactus and verify its height
        let biome_noise = Simplex::new(config.seed.wrapping_add(config.biome_seed_offset));

        for cx in -30..30 {
            for cz in -30..30 {
                let world_x = cx * CHUNK_SIZE as i32;
                let world_z = cz * CHUNK_SIZE as i32;
                let biome = biome_at(world_x, world_z, &biome_noise, config.biome_scale);
                if biome != BiomeType::Desert {
                    continue;
                }

                let chunk = make_surface_chunk_with_cacti(&config, IVec3::new(cx, 2, cz));

                for x in 0..CHUNK_SIZE {
                    for z in 0..CHUNK_SIZE {
                        // Find lowest Cactus in this column
                        let mut cactus_base: Option<usize> = None;
                        for y in 0..CHUNK_SIZE {
                            if chunk.get_block(x, y, z) == BlockType::Cactus {
                                cactus_base = Some(y);
                                break;
                            }
                        }

                        let cactus_base = match cactus_base {
                            Some(y) => y,
                            None => continue,
                        };

                        // Walk up to find height
                        let mut y = cactus_base;
                        while y < CHUNK_SIZE && chunk.get_block(x, y, z) == BlockType::Cactus {
                            y += 1;
                        }
                        let cactus_height = y - cactus_base;

                        assert!(
                            (2..=4).contains(&cactus_height),
                            "Cactus height should be 2-4, got {} at column ({}, {})",
                            cactus_height, x, z
                        );

                        // Block below cactus should be SandDunes or Sand
                        if cactus_base > 0 {
                            let below = chunk.get_block(x, cactus_base - 1, z);
                            assert!(
                                below == BlockType::SandDunes || below == BlockType::Sand,
                                "Block below cactus at ({}, {}, {}) should be SandDunes or Sand, found {:?}",
                                x, cactus_base - 1, z, below
                            );
                        }

                        return; // Verified one cactus, that's enough
                    }
                }
            }
        }

        panic!("Expected to find at least one cactus in desert chunks");
    }

    #[test]
    fn test_cacti_do_not_appear_in_non_desert_biomes() {
        // Generate a chunk in a plains area — should have no cacti
        let config = TerrainConfig::default();
        let biome_noise = Simplex::new(config.seed.wrapping_add(config.biome_seed_offset));

        // Find a Plains chunk
        for cx in -30..30 {
            for cz in -30..30 {
                let world_x = cx * CHUNK_SIZE as i32;
                let world_z = cz * CHUNK_SIZE as i32;
                let biome = biome_at(world_x, world_z, &biome_noise, config.biome_scale);
                if biome != BiomeType::Plains {
                    continue;
                }

                let chunk = make_surface_chunk_with_cacti(&config, IVec3::new(cx, 2, cz));

                for x in 0..CHUNK_SIZE {
                    for y in 0..CHUNK_SIZE {
                        for z in 0..CHUNK_SIZE {
                            assert_ne!(
                                chunk.get_block(x, y, z),
                                BlockType::Cactus,
                                "Plains biome should not have cacti at ({}, {}, {})",
                                x, y, z
                            );
                        }
                    }
                }

                return; // Verified one plains chunk
            }
        }

        panic!("Could not find a Plains chunk to test");
    }

    // ========================================================================
    // Biome boundary blending tests
    // ========================================================================

    #[test]
    fn test_smoothstep_boundaries() {
        // Endpoints
        assert!(smoothstep(0.0).abs() < f64::EPSILON);
        assert!((smoothstep(1.0) - 1.0).abs() < f64::EPSILON);
        // Midpoint
        assert!((smoothstep(0.5) - 0.5).abs() < f64::EPSILON);
        // Monotonically increasing
        assert!(smoothstep(0.25) < smoothstep(0.5));
        assert!(smoothstep(0.5) < smoothstep(0.75));
        // Clamping beyond [0, 1]
        assert!(smoothstep(-1.0).abs() < f64::EPSILON);
        assert!((smoothstep(2.0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_blend_config_defaults() {
        let config = TerrainConfig::default();
        assert!(config.blend_enabled, "Blending should be enabled by default");
        assert!(
            (config.blend_distance - 32.0).abs() < f64::EPSILON,
            "Default blend_distance should be 32.0"
        );
    }

    #[test]
    fn test_blending_disabled_matches_original() {
        // With blending disabled, terrain_column should produce identical results
        // to the original algorithm (biome_at → params → noise octaves).
        let config = TerrainConfig {
            blend_enabled: false,
            ..Default::default()
        };

        let terrain_noise = Simplex::new(config.seed);
        let biome_noise = Simplex::new(config.seed.wrapping_add(config.biome_seed_offset));
        let transition_noise = Perlin::new(config.seed.wrapping_add(config.biome_seed_offset + 500));

        for x in -20..20 {
            for z in -20..20 {
                let (h, b) = terrain_column(x, z, &terrain_noise, &biome_noise, &transition_noise, &config);

                // Replicate the original algorithm manually
                let expected_biome = biome_at(x, z, &biome_noise, config.biome_scale);
                let params = expected_biome.params();

                let mut height = 0.0;
                let mut amplitude = 1.0;
                let mut frequency = params.terrain_frequency;

                for _ in 0..config.octaves {
                    height += terrain_noise.get([
                        x as f64 * frequency,
                        z as f64 * frequency,
                    ]) * amplitude;
                    amplitude *= 0.5;
                    frequency *= 2.0;
                }

                let expected_height = (config.base_height + height * params.terrain_amplitude) as i32;

                assert_eq!(b, expected_biome, "Biome mismatch at ({}, {})", x, z);
                assert_eq!(h, expected_height, "Height mismatch at ({}, {})", x, z);
            }
        }
    }

    #[test]
    fn test_blending_is_deterministic() {
        let config = TerrainConfig {
            blend_enabled: true,
            blend_distance: 32.0,
            ..Default::default()
        };

        let terrain_noise = Simplex::new(config.seed);
        let biome_noise = Simplex::new(config.seed.wrapping_add(config.biome_seed_offset));
        let transition_noise = Perlin::new(config.seed.wrapping_add(config.biome_seed_offset + 500));

        for x in -50..50 {
            for z in -50..50 {
                let (h1, b1) = terrain_column(x, z, &terrain_noise, &biome_noise, &transition_noise, &config);
                let (h2, b2) = terrain_column(x, z, &terrain_noise, &biome_noise, &transition_noise, &config);
                assert_eq!(h1, h2, "Blended height not deterministic at ({}, {})", x, z);
                assert_eq!(b1, b2, "Blended biome not deterministic at ({}, {})", x, z);
            }
        }
    }

    #[test]
    fn test_blended_params_in_uniform_biome_match_raw() {
        // Deep inside a single biome (far from boundaries), blended params
        // should closely match the raw biome params since all 9 samples
        // return the same biome.
        let config = TerrainConfig {
            blend_enabled: true,
            blend_distance: 16.0, // small so we can find uniform regions easily
            ..Default::default()
        };

        let biome_noise = Simplex::new(config.seed.wrapping_add(config.biome_seed_offset));
        let transition_noise = Perlin::new(config.seed.wrapping_add(config.biome_seed_offset + 500));
        let bd = config.blend_distance as i32;

        // Find a position where ALL sample points return the same biome
        for x in -200..200 {
            for z in -200..200 {
                let center_biome = biome_at(x, z, &biome_noise, config.biome_scale);

                // Check all 9 sample offsets
                let all_same = [
                    (0, 0), (-bd, 0), (bd, 0), (0, -bd), (0, bd),
                    (-bd, -bd), (bd, -bd), (-bd, bd), (bd, bd),
                ].iter().all(|&(dx, dz)| {
                    biome_at(x + dx, z + dz, &biome_noise, config.biome_scale) == center_biome
                });

                if all_same {
                    let (amp, freq, biome) = blended_biome_params(x, z, &biome_noise, &transition_noise, &config);
                    let raw = center_biome.params();

                    assert_eq!(biome, center_biome);
                    // With noise modulation, uniform biomes may have tiny deviations
                    // because noise offsets shift weights slightly, but all samples
                    // return the same biome so the result should still be very close.
                    assert!(
                        (amp - raw.terrain_amplitude).abs() < 0.01,
                        "Uniform biome: blended amplitude {:.4} should match raw {:.4}",
                        amp, raw.terrain_amplitude
                    );
                    assert!(
                        (freq - raw.terrain_frequency).abs() < 0.001,
                        "Uniform biome: blended frequency {:.6} should match raw {:.6}",
                        freq, raw.terrain_frequency
                    );
                    return; // Found and verified
                }
            }
        }

        panic!("Could not find a position deep inside a uniform biome");
    }

    #[test]
    fn test_blending_produces_intermediate_values_at_boundary() {
        // At a biome boundary, blended amplitude should fall between the
        // two biomes' raw amplitudes (when they differ).
        let config = TerrainConfig {
            blend_enabled: true,
            blend_distance: 32.0,
            ..Default::default()
        };

        let biome_noise = Simplex::new(config.seed.wrapping_add(config.biome_seed_offset));
        let transition_noise = Perlin::new(config.seed.wrapping_add(config.biome_seed_offset + 500));
        let bd = config.blend_distance as i32;

        // Find a position where the center biome differs from at least one sample
        for x in -300..300 {
            for z in -300..300 {
                let center_biome = biome_at(x, z, &biome_noise, config.biome_scale);

                // Find a neighboring biome that has different amplitude
                let neighbor_biome = [
                    (-bd, 0), (bd, 0), (0, -bd), (0, bd),
                ].iter().find_map(|&(dx, dz)| {
                    let b = biome_at(x + dx, z + dz, &biome_noise, config.biome_scale);
                    if b != center_biome {
                        let diff = (b.params().terrain_amplitude - center_biome.params().terrain_amplitude).abs();
                        if diff > 2.0 { Some(b) } else { None }
                    } else {
                        None
                    }
                });

                if let Some(other) = neighbor_biome {
                    let (amp, _freq, _biome) = blended_biome_params(x, z, &biome_noise, &transition_noise, &config);
                    let amp_a = center_biome.params().terrain_amplitude;
                    let amp_b = other.params().terrain_amplitude;
                    let lo = amp_a.min(amp_b);
                    let hi = amp_a.max(amp_b);

                    // Blended amplitude should be in [lo, hi] range
                    // (with some tolerance for noise-modulated weighted averaging)
                    assert!(
                        amp >= lo - 1.0 && amp <= hi + 1.0,
                        "Blended amplitude {:.2} should be between {:.2} and {:.2} \
                         (biomes {:?} and {:?}) at ({}, {})",
                        amp, lo, hi, center_biome, other, x, z
                    );

                    // And it should NOT exactly equal the center biome's raw value
                    // (blending should have changed it)
                    assert!(
                        (amp - amp_a).abs() > 0.01,
                        "Blended amplitude {:.4} should differ from raw {:.4} at boundary ({}, {})",
                        amp, amp_a, x, z
                    );
                    return; // Found and verified
                }
            }
        }

        panic!("Could not find a biome boundary with differing amplitudes");
    }

    #[test]
    fn test_blending_with_zero_distance_equals_disabled() {
        // blend_distance = 0 should behave identically to blend_enabled = false
        let config_zero = TerrainConfig {
            blend_enabled: true,
            blend_distance: 0.0,
            ..Default::default()
        };
        let config_off = TerrainConfig {
            blend_enabled: false,
            ..Default::default()
        };

        let terrain_noise = Simplex::new(config_zero.seed);
        let biome_noise = Simplex::new(config_zero.seed.wrapping_add(config_zero.biome_seed_offset));
        let transition_noise = Perlin::new(config_zero.seed.wrapping_add(config_zero.biome_seed_offset + 500));

        for x in -20..20 {
            for z in -20..20 {
                let (h1, b1) = terrain_column(x, z, &terrain_noise, &biome_noise, &transition_noise, &config_zero);
                let (h2, b2) = terrain_column(x, z, &terrain_noise, &biome_noise, &transition_noise, &config_off);
                assert_eq!(h1, h2, "Zero distance should match disabled at ({}, {})", x, z);
                assert_eq!(b1, b2, "Biome should match at ({}, {})", x, z);
            }
        }
    }

    #[test]
    fn test_terrain_generation_with_blending_produces_surface() {
        // Full chunk generation with blending enabled should still produce
        // valid terrain (air above, solid below, grass layer).
        let config = TerrainConfig {
            blend_enabled: true,
            blend_distance: 32.0,
            ..Default::default()
        };
        let mut chunk = Chunk::new(IVec3::new(0, 2, 0));
        generate_chunk_terrain(&mut chunk, &config);

        let mut has_air = false;
        let mut has_solid = false;
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    match chunk.get_block(x, y, z) {
                        BlockType::Air => has_air = true,
                        _ => has_solid = true,
                    }
                }
            }
        }

        assert!(has_air, "Blended surface chunk should have air");
        assert!(has_solid, "Blended surface chunk should have solid blocks");
    }

    // ========================================================================
    // Transition noise fade tests
    // ========================================================================

    #[test]
    fn test_transition_noise_config_defaults() {
        let config = TerrainConfig::default();
        assert!(
            (config.transition_noise_scale - 0.08).abs() < f64::EPSILON,
            "Default transition_noise_scale should be 0.08"
        );
        assert!(
            (config.transition_noise_amplitude - 0.45).abs() < f64::EPSILON,
            "Default transition_noise_amplitude should be 0.45"
        );
    }

    #[test]
    fn test_transition_noise_at_deterministic() {
        let config = TerrainConfig::default();
        let tn = Perlin::new(config.seed.wrapping_add(config.biome_seed_offset + 500));

        for x in -50..50 {
            for z in -50..50 {
                let a = transition_noise_at(x, z, &tn, &config);
                let b = transition_noise_at(x, z, &tn, &config);
                assert_eq!(a, b, "Transition noise must be deterministic at ({}, {})", x, z);
            }
        }
    }

    #[test]
    fn test_transition_noise_zero_amplitude_returns_zero() {
        let config = TerrainConfig {
            transition_noise_amplitude: 0.0,
            ..Default::default()
        };
        let tn = Perlin::new(config.seed.wrapping_add(config.biome_seed_offset + 500));

        for x in -20..20 {
            for z in -20..20 {
                let val = transition_noise_at(x, z, &tn, &config);
                assert!(
                    val.abs() < f64::EPSILON,
                    "Zero amplitude should produce zero noise at ({}, {}), got {}",
                    x, z, val
                );
            }
        }
    }

    #[test]
    fn test_transition_noise_bounded_by_amplitude() {
        let config = TerrainConfig {
            transition_noise_amplitude: 0.45,
            ..Default::default()
        };
        let tn = Perlin::new(config.seed.wrapping_add(config.biome_seed_offset + 500));

        for x in -100..100 {
            for z in -100..100 {
                let val = transition_noise_at(x, z, &tn, &config);
                assert!(
                    val.abs() <= config.transition_noise_amplitude + f64::EPSILON,
                    "Transition noise {} should be bounded by amplitude {} at ({}, {})",
                    val, config.transition_noise_amplitude, x, z
                );
            }
        }
    }

    #[test]
    fn test_noise_modulated_blending_differs_from_geometric() {
        // With noise amplitude > 0, blended params should sometimes differ
        // from pure geometric blending (noise_amplitude=0).
        let config_noisy = TerrainConfig {
            blend_enabled: true,
            blend_distance: 32.0,
            transition_noise_amplitude: 0.45,
            ..Default::default()
        };
        let config_geometric = TerrainConfig {
            blend_enabled: true,
            blend_distance: 32.0,
            transition_noise_amplitude: 0.0,
            ..Default::default()
        };

        let biome_noise = Simplex::new(config_noisy.seed.wrapping_add(config_noisy.biome_seed_offset));
        let tn_noisy = Perlin::new(config_noisy.seed.wrapping_add(config_noisy.biome_seed_offset + 500));
        let tn_geo = Perlin::new(config_geometric.seed.wrapping_add(config_geometric.biome_seed_offset + 500));

        let mut differences = 0;
        for x in -100..100 {
            for z in -100..100 {
                let (amp_n, _, _) = blended_biome_params(x, z, &biome_noise, &tn_noisy, &config_noisy);
                let (amp_g, _, _) = blended_biome_params(x, z, &biome_noise, &tn_geo, &config_geometric);
                if (amp_n - amp_g).abs() > 0.001 {
                    differences += 1;
                }
            }
        }

        assert!(
            differences > 0,
            "Noise-modulated blending should differ from geometric blending at some positions"
        );
    }

    #[test]
    fn test_block_blend_hash_deterministic() {
        let seed = 12345_u32;
        for x in -20..20 {
            for z in -20..20 {
                let a = block_blend_hash(x, 0, z, seed);
                let b = block_blend_hash(x, 0, z, seed);
                assert_eq!(a, b, "block_blend_hash must be deterministic at ({}, {}, {})", x, 0, z);
                assert!((0.0..1.0).contains(&a), "Hash should be in [0, 1) at ({}, {}, {})", x, 0, z);
            }
        }
    }

    #[test]
    fn test_block_blend_hash_varies_with_position() {
        let seed = 12345_u32;
        let mut values = std::collections::HashSet::new();
        for x in 0..20 {
            for z in 0..20 {
                // Quantize to avoid floating-point dedup issues
                let v = (block_blend_hash(x, 0, z, seed) * 10000.0) as u64;
                values.insert(v);
            }
        }
        // 400 positions should produce many distinct values
        assert!(
            values.len() > 100,
            "block_blend_hash should produce varied outputs, got {} unique values from 400 inputs",
            values.len()
        );
    }

    #[test]
    fn test_blended_block_palette_returns_primary_deep_in_biome() {
        // Deep inside a uniform biome, blended_block_palette should
        // always return the primary biome's params.
        let config = TerrainConfig {
            blend_enabled: true,
            blend_distance: 16.0,
            ..Default::default()
        };

        let biome_noise = Simplex::new(config.seed.wrapping_add(config.biome_seed_offset));
        let transition_noise = Perlin::new(config.seed.wrapping_add(config.biome_seed_offset + 500));
        let bd = config.blend_distance as i32;

        // Find a position where ALL sample points return the same biome
        for x in -200..200 {
            for z in -200..200 {
                let center_biome = biome_at(x, z, &biome_noise, config.biome_scale);

                let all_same = [
                    (-bd, 0), (bd, 0), (0, -bd), (0, bd),
                    (-bd, -bd), (bd, -bd), (-bd, bd), (bd, bd),
                ].iter().all(|&(dx, dz)| {
                    biome_at(x + dx, z + dz, &biome_noise, config.biome_scale) == center_biome
                });

                if all_same {
                    let palette = blended_block_palette(
                        x, z, center_biome, &biome_noise, &transition_noise, &config,
                    );
                    let expected = center_biome.params();
                    assert_eq!(
                        palette.surface_block, expected.surface_block,
                        "Deep inside {:?}, surface should match at ({}, {})",
                        center_biome, x, z
                    );
                    assert_eq!(
                        palette.subsurface_block, expected.subsurface_block,
                        "Deep inside {:?}, subsurface should match at ({}, {})",
                        center_biome, x, z
                    );
                    return;
                }
            }
        }

        panic!("Could not find a position deep inside a uniform biome");
    }

    #[test]
    fn test_blended_block_palette_mixes_at_boundary() {
        // At biome boundaries, some positions should use the secondary
        // biome's block palette due to probabilistic blending.
        let config = TerrainConfig {
            blend_enabled: true,
            blend_distance: 32.0,
            ..Default::default()
        };

        let biome_noise = Simplex::new(config.seed.wrapping_add(config.biome_seed_offset));
        let transition_noise = Perlin::new(config.seed.wrapping_add(config.biome_seed_offset + 500));
        let bd = config.blend_distance as i32;

        let mut found_mixed = false;

        // Scan for a boundary between biomes with different surface blocks
        'outer: for x in -300..300 {
            for z in -300..300 {
                let center_biome = biome_at(x, z, &biome_noise, config.biome_scale);

                // Check if any nearby sample has a different biome with different surface
                let has_different_neighbor = [
                    (-bd, 0), (bd, 0), (0, -bd), (0, bd),
                ].iter().any(|&(dx, dz)| {
                    let b = biome_at(x + dx, z + dz, &biome_noise, config.biome_scale);
                    b != center_biome && b.params().surface_block != center_biome.params().surface_block
                });

                if !has_different_neighbor {
                    continue;
                }

                // Found a boundary — check a cluster of nearby positions
                // to see if block blending produces mixed surface types.
                let primary_surface = center_biome.params().surface_block;
                for dx in -8..8 {
                    for dz in -8..8 {
                        let bx = x + dx;
                        let bz = z + dz;
                        let local_biome = biome_at(bx, bz, &biome_noise, config.biome_scale);
                        if local_biome != center_biome { continue; }

                        let palette = blended_block_palette(
                            bx, bz, local_biome, &biome_noise, &transition_noise, &config,
                        );
                        if palette.surface_block != primary_surface {
                            found_mixed = true;
                            break 'outer;
                        }
                    }
                }
            }
        }

        assert!(
            found_mixed,
            "Block palette blending should produce mixed surface types at biome boundaries"
        );
    }

    #[test]
    fn test_blended_block_palette_disabled_returns_primary() {
        // With blending disabled, blended_block_palette should always
        // return the primary biome's params.
        let config = TerrainConfig {
            blend_enabled: false,
            ..Default::default()
        };

        let biome_noise = Simplex::new(config.seed.wrapping_add(config.biome_seed_offset));
        let transition_noise = Perlin::new(config.seed.wrapping_add(config.biome_seed_offset + 500));

        for x in -50..50 {
            for z in -50..50 {
                let biome = biome_at(x, z, &biome_noise, config.biome_scale);
                let palette = blended_block_palette(
                    x, z, biome, &biome_noise, &transition_noise, &config,
                );
                let expected = biome.params();
                assert_eq!(
                    palette.surface_block, expected.surface_block,
                    "Disabled blending should return primary palette at ({}, {})",
                    x, z
                );
            }
        }
    }

    #[test]
    fn test_terrain_generation_with_noise_fade_deterministic() {
        // Full terrain generation with noise fade must be deterministic.
        let config = TerrainConfig {
            blend_enabled: true,
            blend_distance: 32.0,
            transition_noise_amplitude: 0.45,
            ..Default::default()
        };

        let mut chunk1 = Chunk::new(IVec3::new(3, 2, 5));
        let mut chunk2 = Chunk::new(IVec3::new(3, 2, 5));

        generate_chunk_terrain(&mut chunk1, &config);
        generate_chunk_terrain(&mut chunk2, &config);

        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(
                        chunk1.get_block(x, y, z),
                        chunk2.get_block(x, y, z),
                        "Noise-fade terrain must be deterministic at ({}, {}, {})",
                        x, y, z
                    );
                }
            }
        }
    }
}
