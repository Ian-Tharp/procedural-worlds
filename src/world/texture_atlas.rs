//! Texture atlas system for block face textures.
//!
//! Generates a procedural texture atlas at startup where each block type has
//! per-face textures (e.g., grass top differs from grass sides). The atlas is
//! a grid of 16×16-pixel tiles packed into a power-of-2 image.
//!
//! Architecture is designed so that procedural generation can be swapped for
//! PNG loading later.

use bevy::prelude::*;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::BlockType;
use super::meshing::Face;

// ============================================================================
// BLOCK TEXTURE MAPPING
// ============================================================================

/// Per-face atlas tile indices for a block type.
#[derive(Clone, Copy, Debug)]
pub struct BlockTextures {
    /// Atlas tile index for the top (+Y) face
    pub top: u32,
    /// Atlas tile index for the bottom (-Y) face
    pub bottom: u32,
    /// Atlas tile index for all four side faces
    pub side: u32,
}

/// Return the atlas tile index for a given block type and face direction.
///
/// Air returns 0 but should never actually be rendered.
pub fn block_face_texture(block: BlockType, face: Face) -> u32 {
    let tex = block_textures(block);
    match face {
        Face::Top => tex.top,
        Face::Bottom => tex.bottom,
        Face::North | Face::South | Face::East | Face::West => tex.side,
    }
}

/// Return the full `BlockTextures` for a block type.
///
/// Tile indices are assigned sequentially. Each unique texture variant gets its
/// own tile slot. The assignment must stay in sync with `generate_tile_rgba`.
pub fn block_textures(block: BlockType) -> BlockTextures {
    match block {
        // Air — never rendered; placeholder index 0
        BlockType::Air => BlockTextures { top: 0, bottom: 0, side: 0 },

        // Stone — all faces: tile 0 (grey with noise/cracks)
        BlockType::Stone => BlockTextures { top: 0, bottom: 0, side: 0 },

        // Dirt — all faces: tile 1 (brown with speckles)
        BlockType::Dirt => BlockTextures { top: 1, bottom: 1, side: 1 },

        // Grass — top: tile 2 (green), bottom: tile 1 (dirt), sides: tile 3 (gradient)
        BlockType::Grass => BlockTextures { top: 2, bottom: 1, side: 3 },

        // Sand — all faces: tile 4 (tan/yellow grain)
        BlockType::Sand => BlockTextures { top: 4, bottom: 4, side: 4 },

        // Water — all faces: tile 5 (blue semi-transparent)
        BlockType::Water => BlockTextures { top: 5, bottom: 5, side: 5 },

        // Wood — top/bottom: tile 6 (rings), sides: tile 7 (bark)
        BlockType::Wood => BlockTextures { top: 6, bottom: 6, side: 7 },

        // Leaves — all faces: tile 8 (varied green with holes)
        BlockType::Leaves => BlockTextures { top: 8, bottom: 8, side: 8 },

        // Sandstone — all faces: tile 9 (layered tan)
        BlockType::Sandstone => BlockTextures { top: 9, bottom: 9, side: 9 },

        // Snow — all faces: tile 10 (white with subtle blue)
        BlockType::Snow => BlockTextures { top: 10, bottom: 10, side: 10 },

        // Ice — all faces: tile 11 (light blue)
        BlockType::Ice => BlockTextures { top: 11, bottom: 11, side: 11 },

        // Obsidian — all faces: tile 12 (dark purple/black)
        BlockType::Obsidian => BlockTextures { top: 12, bottom: 12, side: 12 },

        // VolcanicRock — all faces: tile 13 (dark grey + orange veins)
        BlockType::VolcanicRock => BlockTextures { top: 13, bottom: 13, side: 13 },

        // Cactus — top: tile 14 (cactus top), sides: tile 15 (green stripes)
        BlockType::Cactus => BlockTextures { top: 14, bottom: 14, side: 15 },

        // SandDunes — all faces: tile 16 (golden wave pattern)
        BlockType::SandDunes => BlockTextures { top: 16, bottom: 16, side: 16 },
    }
}

/// The highest tile index used by any block face.
pub const MAX_TILE_INDEX: u32 = 16;

// ============================================================================
// UV COMPUTATION
// ============================================================================

/// Compute the UV rectangle for a tile in the atlas.
///
/// Returns `(u_min, v_min, u_size, v_size)` where a single 1×1 block face maps
/// to `[u_min .. u_min + u_size, v_min .. v_min + v_size]`.
///
/// Note on greedy meshing + atlases:
/// - With a classic packed atlas and Bevy's `StandardMaterial`, you **cannot**
///   "repeat within a single tile" just by scaling UVs — scaling makes the UVs
///   walk into neighboring tiles.
/// - To truly tile per-block while still greedy-merging geometry, you'd need a
///   custom shader (or texture arrays) that applies `fract()` *within the tile*
///   region.
/// - This engine therefore keeps atlas UVs **inside the tile bounds** and
///   stretches the tile across greedy-merged quads (still visually coherent,
///   and avoids sampling unrelated tiles).
#[inline]
pub fn atlas_uv(tile_index: u32, tiles_per_row: u32, tile_size: u32, atlas_size: u32) -> (f32, f32, f32, f32) {
    let tile_x = tile_index % tiles_per_row;
    let tile_y = tile_index / tiles_per_row;
    let u_min = (tile_x * tile_size) as f32 / atlas_size as f32;
    let v_min = (tile_y * tile_size) as f32 / atlas_size as f32;
    let u_size = tile_size as f32 / atlas_size as f32;
    let v_size = tile_size as f32 / atlas_size as f32;
    (u_min, v_min, u_size, v_size)
}

/// Build the 4-vertex UV array for a face quad.
///
/// `quad_w` and `quad_h` are currently **not used** for atlas UVs (see note in
/// [`atlas_uv`]). We keep them in the signature for forward compatibility if
/// we later add a custom material/shader that supports per-tile repeating.
///
/// ## Edge Bleeding Prevention
///
/// UVs are inset by a full texel (not half) from each tile edge. While
/// half-texel is the theoretical minimum for nearest filtering, a full-texel
/// inset provides robustness against:
/// - Bilinear filtering bleed if the sampler mode is changed
/// - Mip-map level sampling that may straddle tile boundaries
/// - GPU driver rounding differences across hardware
/// - Floating-point accumulation errors in interpolated UVs
///
/// The inset is clamped to at most 25% of the tile UV size to ensure the
/// UV rectangle never collapses or inverts, even for very small tiles.
pub fn face_uvs_atlas(
    tile_index: u32,
    tiles_per_row: u32,
    tile_size: u32,
    atlas_size: u32,
    quad_w: f32,
    quad_h: f32,
) -> [[f32; 2]; 4] {
    let _ = (quad_w, quad_h);

    let (u_min, v_min, u_size, v_size) =
        atlas_uv(tile_index, tiles_per_row, tile_size, atlas_size);

    // Full-texel inset from tile edges to prevent edge bleeding.
    // This matches the shader's `remap_atlas_uv` inset strategy for
    // consistency between CPU-mapped and shader-mapped UV paths.
    let texel = 1.0 / atlas_size.max(1) as f32;
    let mut inset = texel; // full texel, not half
    // Ensure inset can't invert the UV rectangle even for tiny tiles/configs.
    inset = inset.min(u_size * 0.25).min(v_size * 0.25);

    let u0 = u_min + inset;
    let v0 = v_min + inset;
    let u1 = (u_min + u_size) - inset;
    let v1 = (v_min + v_size) - inset;

    [
        [u0, v0],
        [u1, v0],
        [u1, v1],
        [u0, v1],
    ]
}

// ============================================================================
// ATLAS RESOURCE
// ============================================================================

/// Bevy resource holding the generated atlas texture handle and layout metadata.
#[derive(Resource)]
pub struct BlockTextureAtlas {
    /// Handle to the atlas `Image` asset.
    pub texture: Handle<Image>,
    /// Number of tiles per row in the atlas grid.
    pub tiles_per_row: u32,
    /// Pixel size of each tile (width = height).
    pub tile_size: u32,
    /// Total atlas image size in pixels (width = height).
    pub atlas_size: u32,
}

// ============================================================================
// PROCEDURAL ATLAS GENERATION
// ============================================================================

/// Build the procedural atlas `Image` and return it along with layout metadata.
///
/// The image is `atlas_size × atlas_size` pixels, RGBA8, with `Nearest` filtering
/// and `Repeat` address mode (required for greedy-mesh UV tiling).
pub fn build_atlas_image(tile_size: u32, grid_size: u32) -> Image {
    let atlas_size = tile_size * grid_size;
    let total_tiles = grid_size * grid_size;
    let pixel_count = (atlas_size * atlas_size) as usize;
    let mut data = vec![0u8; pixel_count * 4]; // RGBA

    for tile_index in 0..total_tiles {
        let tile_x = tile_index % grid_size;
        let tile_y = tile_index / grid_size;
        let base_px = tile_x * tile_size;
        let base_py = tile_y * tile_size;

        let tile_rgba = generate_tile_rgba(tile_index, tile_size);

        for py in 0..tile_size {
            for px in 0..tile_size {
                let img_x = base_px + px;
                let img_y = base_py + py;
                let img_idx = ((img_y * atlas_size + img_x) * 4) as usize;
                let tile_idx = ((py * tile_size + px) * 4) as usize;
                data[img_idx..img_idx + 4].copy_from_slice(&tile_rgba[tile_idx..tile_idx + 4]);
            }
        }
    }

    let mut image = Image::new(
        Extent3d {
            width: atlas_size,
            height: atlas_size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );

    // Nearest filtering for crisp pixel-art look.
    //
    // NOTE: We use ClampToEdge because we keep atlas UVs within each tile's
    // rectangle; Repeat would wrap across the *entire atlas*, which is not
    // what we want for a packed tile atlas.
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Nearest,
        min_filter: ImageFilterMode::Nearest,
        mipmap_filter: ImageFilterMode::Nearest,
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        address_mode_w: ImageAddressMode::ClampToEdge,
        ..default()
    });

    image
}

/// Startup system: generate the atlas image, add it to `Assets<Image>`, and
/// insert the `BlockTextureAtlas` resource.
pub fn setup_block_texture_atlas(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    config: Res<crate::config::EngineConfig>,
) {
    let tile_size = config.render.atlas_tile_size;
    let grid_size = config.render.atlas_grid_size;
    let atlas_size = tile_size * grid_size;

    let total_tiles = grid_size * grid_size;
    // MAX_TILE_INDEX is inclusive, so we need at least MAX_TILE_INDEX + 1 tiles.
    if total_tiles <= MAX_TILE_INDEX {
        warn!(
            "Texture atlas grid too small: {}×{} = {} tiles, but code uses tile indices up to {}. \
             Increase render.atlas_grid_size or reduce MAX_TILE_INDEX/texture mappings.",
            grid_size,
            grid_size,
            total_tiles,
            MAX_TILE_INDEX
        );
    }

    let image = build_atlas_image(tile_size, grid_size);
    let handle = images.add(image);

    commands.insert_resource(BlockTextureAtlas {
        texture: handle,
        tiles_per_row: grid_size,
        tile_size,
        atlas_size,
    });

    info!(
        "Block texture atlas created: {}×{} pixels, {} tiles ({}×{}px each)",
        atlas_size, atlas_size, grid_size * grid_size, tile_size, tile_size,
    );
}

// ============================================================================
// PROCEDURAL TILE GENERATION
// ============================================================================

/// Simple deterministic hash for pseudo-random noise.
#[inline]
fn noise_hash(x: u32, y: u32, seed: u32) -> u8 {
    let n = x.wrapping_mul(73).wrapping_add(y.wrapping_mul(37)).wrapping_add(seed);
    let n = n ^ (n >> 13);
    let n = n.wrapping_mul(1274126177);
    (n >> 24) as u8
}

/// Multi-octave noise helper: layers fine, medium, and coarse noise.
#[inline]
fn multi_noise(x: u32, y: u32, seed: u32) -> f32 {
    let n1 = noise_hash(x, y, seed) as f32 / 255.0;
    let n2 = noise_hash(x / 2, y / 2, seed.wrapping_add(100)) as f32 / 255.0;
    let n3 = noise_hash(x / 4, y / 4, seed.wrapping_add(200)) as f32 / 255.0;
    n1 * 0.5 + n2 * 0.3 + n3 * 0.2
}

/// Generate the RGBA pixel data for a single tile.
///
/// Returns a `Vec<u8>` with `tile_size * tile_size * 4` bytes.
///
/// Tile index assignments (must match `block_textures`):
///   0  = Stone (grey + noise/cracks)
///   1  = Dirt (brown + speckles)
///   2  = Grass top (green + variation)
///   3  = Grass side (green-top / dirt-bottom gradient)
///   4  = Sand (tan + grain)
///   5  = Water (blue, semi-transparent)
///   6  = Wood top/bottom (rings)
///   7  = Wood side / bark (vertical lines)
///   8  = Leaves (green + holes)
///   9  = Sandstone (layered tan)
///  10  = Snow (white + blue tint)
///  11  = Ice (light blue)
///  12  = Obsidian (dark purple/black)
///  13  = VolcanicRock (dark grey + orange veins)
///  14  = Cactus top
///  15  = Cactus side (green + vertical stripes)
///  16  = SandDunes (golden wave)
///  17+ = magenta debug fill
fn generate_tile_rgba(tile_index: u32, tile_size: u32) -> Vec<u8> {
    let count = (tile_size * tile_size * 4) as usize;
    let mut data = vec![255u8; count]; // default opaque white

    let set = |data: &mut Vec<u8>, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8, ts: u32| {
        let idx = ((y * ts + x) * 4) as usize;
        data[idx] = r;
        data[idx + 1] = g;
        data[idx + 2] = b;
        data[idx + 3] = a;
    };

    let ts = tile_size;

    match tile_index {
        // ── 0: Stone — multi-octave grey with crack lines and mineral speckles ──
        0 => {
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 42);
                    let base: i16 = 128;
                    let v = (base as f32 + (mn - 0.5) * 40.0).clamp(0.0, 255.0) as u8;

                    // Diagonal cracks: 2-3 cracks across the tile
                    let crack1 = ((x as i32 + y as i32 * 2 - (ts as i32 / 3)).unsigned_abs() % ts) as u32;
                    let crack2 = ((x as i32 * 2 - y as i32 + (ts as i32 * 2 / 3)).unsigned_abs() % ts) as u32;
                    let crack_noise = noise_hash(x, y, 99) as u32;
                    let on_crack1 = crack1 < 2 + (crack_noise % 2);
                    let on_crack2 = crack2 < 2 + ((crack_noise / 4) % 2);
                    let v = if on_crack1 || on_crack2 {
                        v.saturating_sub(35 + (noise_hash(x, y, 101) % 15))
                    } else {
                        v
                    };

                    // Mineral speckles: occasional brighter pixels
                    let speck = noise_hash(x.wrapping_add(7), y.wrapping_add(3), 105);
                    let v = if speck < 8 { v.saturating_add(30) } else { v };

                    set(&mut data, x, y, v, v, v, 255, ts);
                }
            }
        }

        // ── 1: Dirt — multi-scale noise with pebbles and organic patches ──
        1 => {
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 77);
                    let variation = (mn - 0.5) * 30.0;
                    let mut r = (115.0 + variation).clamp(0.0, 255.0);
                    let mut g = (82.0 + variation * 0.8).clamp(0.0, 255.0);
                    let mut b = (56.0 + variation * 0.6).clamp(0.0, 255.0);

                    // Subtle horizontal layering
                    let layer_noise = noise_hash(x / 2, y, 210) as f32 / 255.0;
                    let layer_shift = ((y as f32 * 8.0 / ts as f32).sin() * 3.0) * layer_noise;
                    r += layer_shift;
                    g += layer_shift * 0.7;

                    // Pebble dots (3-4px clusters, lighter)
                    let peb = noise_hash(x / 3, y / 3, 200);
                    let peb_fine = noise_hash(x, y, 201);
                    if peb < 25 && peb_fine < 180 {
                        r += 20.0;
                        g += 14.0;
                        b += 8.0;
                    }

                    // Dark organic patches
                    let org = noise_hash(x / 4, y / 4, 215);
                    if org < 20 {
                        r -= 12.0;
                        g -= 8.0;
                        b -= 5.0;
                    }

                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                }
            }
        }

        // ── 2: Grass top — multi-octave green with blade streaks and spots ──
        2 => {
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 123);
                    let mut r = (90.0 + (mn - 0.5) * 25.0).clamp(0.0, 255.0);
                    let mut g = (155.0 + (mn - 0.5) * 35.0).clamp(0.0, 255.0);
                    let mut b = (65.0 + (mn - 0.5) * 18.0).clamp(0.0, 255.0);

                    // Darker blade-like streaks (thin diagonal lines)
                    let blade_hash = noise_hash(x, y / 2, 130);
                    let blade_dir = noise_hash(x / 3, y / 3, 131);
                    if blade_hash < 22 {
                        let dark = if blade_dir < 128 { 15.0 } else { 20.0 };
                        r -= dark;
                        g -= dark * 0.5;
                        b -= dark;
                    }

                    // Occasional yellow/brown spots
                    let spot = noise_hash(x / 4, y / 4, 140);
                    let spot_fine = noise_hash(x, y, 141);
                    if spot < 12 && spot_fine < 100 {
                        r += 30.0;
                        g -= 15.0;
                        b -= 20.0;
                    }

                    // Bright green highlights for freshness
                    let highlight = noise_hash(x.wrapping_add(5), y.wrapping_add(9), 145);
                    if highlight < 10 {
                        g += 20.0;
                        r -= 5.0;
                    }

                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                }
            }
        }

        // ── 3: Grass side — sharp green-top / dirt-bottom, Minecraft-style ──
        3 => {
            let green_height = ts * 3 / 16; // ~12 rows at 64px
            let transition_width = ts * 3 / 32; // ~6 rows transition zone

            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 155);

                    // Per-column jagged transition: ±3 pixel variation
                    let edge_hash = noise_hash(x, 0, 175) as i32;
                    let jag = (edge_hash % 7) - 3; // -3 to +3
                    let transition_row = (green_height as i32 + jag).max(1) as u32;

                    if y < transition_row {
                        // Green section with blade-like vertical streaks
                        let mut r = (90.0 + (mn - 0.5) * 20.0).clamp(0.0, 255.0);
                        let mut g = (155.0 + (mn - 0.5) * 30.0).clamp(0.0, 255.0);
                        let b = (65.0 + (mn - 0.5) * 15.0).clamp(0.0, 255.0);

                        // Vertical blade streaks hanging down
                        let blade = noise_hash(x, 0, 180);
                        if blade < 60 && y > transition_row / 2 {
                            g -= 12.0;
                            r -= 5.0;
                        }

                        set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                    } else if y < transition_row + transition_width {
                        // Transition zone: blend from grass to dirt
                        let t = (y - transition_row) as f32 / transition_width as f32;
                        let gr = 90.0 + (mn - 0.5) * 20.0;
                        let gg = 155.0 + (mn - 0.5) * 30.0;
                        let gb = 65.0 + (mn - 0.5) * 15.0;
                        let dr = 115.0 + (mn - 0.5) * 25.0;
                        let dg = 82.0 + (mn - 0.5) * 20.0;
                        let db = 56.0 + (mn - 0.5) * 15.0;
                        let r = (gr * (1.0 - t) + dr * t).clamp(0.0, 255.0) as u8;
                        let g = (gg * (1.0 - t) + dg * t).clamp(0.0, 255.0) as u8;
                        let b = (gb * (1.0 - t) + db * t).clamp(0.0, 255.0) as u8;
                        set(&mut data, x, y, r, g, b, 255, ts);
                    } else {
                        // Dirt section (matching tile 1 look)
                        let variation = (mn - 0.5) * 30.0;
                        let mut r = 115.0 + variation;
                        let mut g = 82.0 + variation * 0.8;
                        let mut b = 56.0 + variation * 0.6;

                        // Pebble dots
                        let peb = noise_hash(x / 3, y / 3, 200);
                        let peb_fine = noise_hash(x, y, 201);
                        if peb < 25 && peb_fine < 180 {
                            r += 18.0;
                            g += 12.0;
                            b += 7.0;
                        }

                        set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                    }
                }
            }
        }

        // ── 4: Sand — multi-scale grain with diagonal ripple pattern ──
        4 => {
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 210);

                    // Diagonal ripple pattern
                    let ripple = ((x as f32 * 0.7 + y as f32 * 0.3) * 6.2832 / ts as f32 * 3.0).sin() * 0.5 + 0.5;

                    let r = (230.0 + (mn - 0.5) * 20.0 + ripple * 8.0).clamp(0.0, 255.0);
                    let g = (217.0 + (mn - 0.5) * 18.0 + ripple * 6.0).clamp(0.0, 255.0);
                    let b = (153.0 + (mn - 0.5) * 14.0 + ripple * 4.0).clamp(0.0, 255.0);

                    // Granules: scattered lighter and darker grains
                    let grain = noise_hash(x, y, 212);
                    let (r, g, b) = if grain < 15 {
                        ((r + 12.0).min(255.0), (g + 10.0).min(255.0), (b + 7.0).min(255.0))
                    } else if grain > 240 {
                        ((r - 10.0).max(0.0), (g - 8.0).max(0.0), (b - 6.0).max(0.0))
                    } else {
                        (r, g, b)
                    };

                    set(&mut data, x, y, r as u8, g as u8, b as u8, 255, ts);
                }
            }
        }

        // ── 5: Water — caustic-like pattern with depth variation ──
        5 => {
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 55);

                    // Caustic-like overlapping circles
                    let cx1 = ts as f32 * 0.3;
                    let cy1 = ts as f32 * 0.4;
                    let cx2 = ts as f32 * 0.7;
                    let cy2 = ts as f32 * 0.6;
                    let d1 = ((x as f32 - cx1).powi(2) + (y as f32 - cy1).powi(2)).sqrt();
                    let d2 = ((x as f32 - cx2).powi(2) + (y as f32 - cy2).powi(2)).sqrt();
                    let wave1 = (d1 * 6.2832 / (ts as f32 * 0.4)).sin() * 0.5 + 0.5;
                    let wave2 = (d2 * 6.2832 / (ts as f32 * 0.35)).sin() * 0.5 + 0.5;
                    let caustic = (wave1 + wave2) * 0.5;

                    let r = (51.0 + (mn - 0.5) * 15.0 + caustic * 12.0).clamp(0.0, 255.0) as u8;
                    let g = (102.0 + (mn - 0.5) * 20.0 + caustic * 15.0).clamp(0.0, 255.0) as u8;
                    let b = (204.0 + (mn - 0.5) * 18.0 + caustic * 8.0).clamp(0.0, 255.0) as u8;
                    set(&mut data, x, y, r, g, b, 204, ts);
                }
            }
        }

        // ── 6: Wood top/bottom — detailed rings with off-center and grain ──
        6 => {
            let cx = ts as f32 * 0.45; // slightly off-center
            let cy = ts as f32 * 0.52;
            for y in 0..ts {
                for x in 0..ts {
                    let dx = x as f32 - cx;
                    let dy = y as f32 - cy;
                    let dist = (dx * dx + dy * dy).sqrt();
                    let mn = multi_noise(x, y, 170);

                    // Ring spacing with variation
                    let ring_freq = 0.8 + mn * 0.4; // varying ring density
                    let ring_val = ((dist * ring_freq) * 3.14159 / 3.0).sin() * 0.5 + 0.5;

                    let r = (130.0 + ring_val * 35.0 + (mn - 0.5) * 15.0).clamp(0.0, 255.0) as u8;
                    let g = (90.0 + ring_val * 30.0 + (mn - 0.5) * 12.0).clamp(0.0, 255.0) as u8;
                    let b = (50.0 + ring_val * 20.0 + (mn - 0.5) * 8.0).clamp(0.0, 255.0) as u8;

                    set(&mut data, x, y, r, g, b, 255, ts);
                }
            }
        }

        // ── 7: Wood bark — vertical strips with horizontal cracks and grain ──
        7 => {
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 180);

                    // Vertical bark strips (5-6 strips)
                    let strip_freq = 5.0;
                    let strip_val = (x as f32 * strip_freq * 6.2832 / ts as f32).sin() * 0.5 + 0.5;
                    let depth = strip_val * 20.0;

                    let mut r = (128.0 + depth + (mn - 0.5) * 18.0).clamp(0.0, 255.0);
                    let mut g = (89.0 + depth * 0.7 + (mn - 0.5) * 14.0).clamp(0.0, 255.0);
                    let mut b = (51.0 + depth * 0.4 + (mn - 0.5) * 10.0).clamp(0.0, 255.0);

                    // Horizontal cracks between strips
                    let crack_h = noise_hash(x / 4, y, 185);
                    if crack_h < 12 && strip_val < 0.3 {
                        r -= 20.0;
                        g -= 15.0;
                        b -= 10.0;
                    }

                    // Fine vertical grain detail
                    let grain = noise_hash(x, y, 188) as f32 / 255.0;
                    r += (grain - 0.5) * 6.0;
                    g += (grain - 0.5) * 4.0;

                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                }
            }
        }

        // ── 8: Leaves — varied shapes with gaps and color clusters ──
        8 => {
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 88);

                    // Gaps/holes at medium scale
                    let hole = noise_hash(x / 3, y / 3, 33);
                    let hole_fine = noise_hash(x, y, 34);

                    if hole < 20 && hole_fine < 120 {
                        // Dark gap / see-through
                        set(&mut data, x, y, 30, 60, 20, 180, ts);
                    } else {
                        // Color clusters for visual interest
                        let cluster = noise_hash(x / 4, y / 4, 90) as f32 / 255.0;
                        let brightness = if cluster < 0.3 { -12.0 } else if cluster > 0.7 { 12.0 } else { 0.0 };

                        let r = (51.0 + (mn - 0.5) * 28.0 + brightness).clamp(0.0, 255.0) as u8;
                        let g = (128.0 + (mn - 0.5) * 40.0 + brightness).clamp(0.0, 255.0) as u8;
                        let b = (38.0 + (mn - 0.5) * 18.0 + brightness * 0.5).clamp(0.0, 255.0) as u8;
                        set(&mut data, x, y, r, g, b, 225, ts);
                    }
                }
            }
        }

        // ── 9: Sandstone — horizontal stratification layers ──
        9 => {
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 250);

                    // Stratification layers of varying thickness (2-6px)
                    // Use cumulative layer boundaries generated from noise
                    let layer_seed = noise_hash(0, y / 3, 255) as f32 / 255.0;
                    let layer_band = ((y as f32 * 8.0 / ts as f32 + layer_seed * 2.0) as u32) % 3;

                    let (base_r, base_g, base_b): (f32, f32, f32) = match layer_band {
                        0 => (210.0, 186.0, 135.0), // cream
                        1 => (200.0, 178.0, 130.0), // tan
                        _ => (195.0, 172.0, 125.0), // darker tan
                    };

                    // Fine noise within each layer
                    let r = (base_r + (mn - 0.5) * 16.0).clamp(0.0, 255.0) as u8;
                    let g = (base_g + (mn - 0.5) * 14.0).clamp(0.0, 255.0) as u8;
                    let b = (base_b + (mn - 0.5) * 12.0).clamp(0.0, 255.0) as u8;
                    set(&mut data, x, y, r, g, b, 255, ts);
                }
            }
        }

        // ── 10: Snow — subtle blue shadows, sparkle pixels, crystalline ──
        10 => {
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 300);

                    // Subtle blue shadow pattern
                    let shadow = noise_hash(x / 4, y / 4, 305) as f32 / 255.0;
                    let blue_shift = shadow * 4.0;

                    let mut r = (242.0 + (mn - 0.5) * 8.0 - blue_shift).clamp(0.0, 255.0);
                    let mut g = (242.0 + (mn - 0.5) * 8.0 - blue_shift * 0.5).clamp(0.0, 255.0);
                    let mut b = (248.0 + (mn - 0.5) * 6.0).clamp(0.0, 255.0);

                    // Sparkle pixels (very bright)
                    let sparkle = noise_hash(x.wrapping_add(13), y.wrapping_add(29), 310);
                    if sparkle < 4 {
                        r = 255.0;
                        g = 255.0;
                        b = 255.0;
                    }

                    // Crystalline pattern (barely visible)
                    let crystal = ((x as f32 * 2.0 + y as f32).sin() * 0.5 + 0.5) * 2.0;
                    b = (b + crystal).min(255.0);

                    set(&mut data, x, y, r as u8, g as u8, b as u8, 255, ts);
                }
            }
        }

        // ── 11: Ice — crack lines, semi-transparent, blue variation ──
        11 => {
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 310);

                    let mut r = (178.0 + (mn - 0.5) * 20.0).clamp(0.0, 255.0);
                    let mut g = (217.0 + (mn - 0.5) * 18.0).clamp(0.0, 255.0);
                    let mut b = (242.0 + (mn - 0.5) * 10.0).clamp(0.0, 255.0);

                    // Crack lines
                    let crack_a = ((x as i32 * 3 + y as i32 - (ts as i32 / 2)).unsigned_abs() % ts) as u32;
                    let crack_b = ((x as i32 - y as i32 * 2 + (ts as i32 / 3)).unsigned_abs() % ts) as u32;
                    let crack_n = noise_hash(x, y, 315);
                    if crack_a < 2 + (crack_n as u32 % 2) || crack_b < 2 {
                        r -= 25.0;
                        g -= 15.0;
                        b -= 8.0;
                    }

                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 220, ts);
                }
            }
        }

        // ── 12: Obsidian — deep purple-black with glossy streaks ──
        12 => {
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 320);

                    let mut r = (25.0 + (mn - 0.5) * 12.0).clamp(0.0, 255.0);
                    let mut g = (20.0 + (mn - 0.5) * 10.0).clamp(0.0, 255.0);
                    let mut b = (31.0 + (mn - 0.5) * 14.0).clamp(0.0, 255.0);

                    // Glossy diagonal streaks
                    let streak = ((x as f32 * 0.8 + y as f32 * 0.6) * 4.0 / ts as f32 * 6.2832).sin();
                    if streak > 0.85 {
                        r += 8.0;
                        g += 5.0;
                        b += 12.0;
                    }

                    // Subtle purple highlights
                    let highlight = noise_hash(x / 3, y / 3, 325);
                    if highlight < 10 {
                        r += 6.0;
                        b += 10.0;
                    }

                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                }
            }
        }

        // ── 13: VolcanicRock — dark base with connected orange/red vein network ──
        13 => {
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 330);

                    // Connected vein network using distance field approach
                    // Multiple vein lines that branch
                    let vein_d1 = ((x as i32 * 2 + y as i32 - (ts as i32)).unsigned_abs() % ts) as f32;
                    let vein_d2 = ((x as i32 - y as i32 * 2 + (ts as i32 * 3 / 4)).unsigned_abs() % ts) as f32;
                    let vein_d3 = ((x as i32 + y as i32 * 3 / 2 - (ts as i32 / 2)).unsigned_abs() % ts) as f32;
                    let vein_noise = noise_hash(x / 2, y / 2, 335) as f32 / 255.0;
                    let vein_threshold = 3.0 + vein_noise * 2.0;
                    let is_vein = vein_d1 < vein_threshold || vein_d2 < vein_threshold || vein_d3 < vein_threshold;

                    let (r, g, b) = if is_vein {
                        // Orange/red vein
                        let glow = 1.0 - (vein_d1.min(vein_d2).min(vein_d3) / vein_threshold).min(1.0);
                        (
                            (180.0 + glow * 40.0 + (mn - 0.5) * 15.0).clamp(0.0, 255.0) as u8,
                            (65.0 + glow * 25.0 + (mn - 0.5) * 10.0).clamp(0.0, 255.0) as u8,
                            (15.0 + glow * 10.0 + (mn - 0.5) * 8.0).clamp(0.0, 255.0) as u8,
                        )
                    } else {
                        // Ashy dark base
                        (
                            (77.0 + (mn - 0.5) * 18.0).clamp(0.0, 255.0) as u8,
                            (46.0 + (mn - 0.5) * 14.0).clamp(0.0, 255.0) as u8,
                            (38.0 + (mn - 0.5) * 12.0).clamp(0.0, 255.0) as u8,
                        )
                    };
                    set(&mut data, x, y, r, g, b, 255, ts);
                }
            }
        }

        // ── 14: Cactus top — star/cross pattern with rim and thorns ──
        14 => {
            let cx = ts as f32 / 2.0;
            let cy = ts as f32 / 2.0;
            let rim_dist = ts as f32 / 2.0 - (ts as f32 * 3.0 / 32.0);

            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 340);
                    let dx = x as f32 - cx;
                    let dy = y as f32 - cy;
                    let dist = (dx * dx + dy * dy).sqrt();
                    let chebyshev = dx.abs().max(dy.abs());

                    // Star/cross pattern: brighter along axes
                    let cross_dist = dx.abs().min(dy.abs());
                    let on_cross = cross_dist < ts as f32 / 10.0;

                    // Rim detection
                    let on_rim = chebyshev > rim_dist;

                    let (mut r, mut g, mut b) = if on_rim {
                        // Darker rim
                        (
                            50.0 + (mn - 0.5) * 12.0,
                            110.0 + (mn - 0.5) * 16.0,
                            40.0 + (mn - 0.5) * 10.0,
                        )
                    } else if on_cross {
                        // Cross/star pattern — slightly lighter center line
                        (
                            82.0 + (mn - 0.5) * 14.0,
                            155.0 + (mn - 0.5) * 18.0,
                            65.0 + (mn - 0.5) * 12.0,
                        )
                    } else {
                        // Standard cactus green
                        (
                            75.0 + (mn - 0.5) * 14.0,
                            145.0 + (mn - 0.5) * 20.0,
                            60.0 + (mn - 0.5) * 12.0,
                        )
                    };

                    // Thorn dots as bright specks
                    let thorn = noise_hash(x.wrapping_mul(7), y.wrapping_mul(11), 345);
                    if thorn < 5 && dist < rim_dist && dist > ts as f32 * 0.15 {
                        r = 200.0;
                        g = 210.0;
                        b = 170.0;
                    }

                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                }
            }
        }

        // ── 15: Cactus side — vertical ribs with thorn dots ──
        15 => {
            let num_ribs = 4u32;
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 350);

                    // Vertical ribs: sinusoidal pattern for raised sections
                    let rib_phase = (x as f32 * num_ribs as f32 * 6.2832 / ts as f32).sin();
                    let on_rib_edge = rib_phase.abs() < 0.3;
                    let rib_bright = (rib_phase * 0.5 + 0.5) * 12.0;

                    let mut r = (60.0 + rib_bright + (mn - 0.5) * 16.0).clamp(0.0, 255.0);
                    let mut g = (132.0 + rib_bright * 1.5 + (mn - 0.5) * 22.0).clamp(0.0, 255.0);
                    let mut b = (48.0 + rib_bright * 0.6 + (mn - 0.5) * 12.0).clamp(0.0, 255.0);

                    // Thorn dots along rib edges
                    let thorn = noise_hash(x, y.wrapping_mul(5), 355);
                    if on_rib_edge && thorn < 8 && (y % (ts / 8)) < 2 {
                        r = 195.0;
                        g = 200.0;
                        b = 160.0;
                    }

                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                }
            }
        }

        // ── 16: SandDunes — diagonal wind ripple pattern ──
        16 => {
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 360);

                    // Wind ripple waves (diagonal, period ~6px at 64px tile)
                    let period = ts as f32 / 10.0;
                    let wave = ((x as f32 * 0.8 + y as f32 * 0.6) * 6.2832 / period).sin() * 0.5 + 0.5;

                    let r = (217.0 + wave * 20.0 + (mn - 0.5) * 16.0).clamp(0.0, 255.0) as u8;
                    let g = (199.0 + wave * 16.0 + (mn - 0.5) * 14.0).clamp(0.0, 255.0) as u8;
                    let b = (140.0 + wave * 10.0 + (mn - 0.5) * 12.0).clamp(0.0, 255.0) as u8;
                    set(&mut data, x, y, r, g, b, 255, ts);
                }
            }
        }

        // ── Unused tiles: magenta debug fill ────────────
        _ => {
            for y in 0..ts {
                for x in 0..ts {
                    set(&mut data, x, y, 255, 0, 255, 255, ts);
                }
            }
        }
    }

    data
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atlas_uv_computation() {
        // Tile 0 in a 16-tiles-per-row atlas with 16px tiles → 256px atlas
        let (u_min, v_min, u_size, v_size) = atlas_uv(0, 16, 16, 256);
        assert!((u_min - 0.0).abs() < 1e-6);
        assert!((v_min - 0.0).abs() < 1e-6);
        assert!((u_size - 1.0 / 16.0).abs() < 1e-6);
        assert!((v_size - 1.0 / 16.0).abs() < 1e-6);

        // Tile 1 → second column
        let (u_min, v_min, _, _) = atlas_uv(1, 16, 16, 256);
        assert!((u_min - 1.0 / 16.0).abs() < 1e-6);
        assert!((v_min - 0.0).abs() < 1e-6);

        // Tile 16 → first column, second row
        let (u_min, v_min, _, _) = atlas_uv(16, 16, 16, 256);
        assert!((u_min - 0.0).abs() < 1e-6);
        assert!((v_min - 1.0 / 16.0).abs() < 1e-6);

        // Tile 17 → second column, second row
        let (u_min, v_min, _, _) = atlas_uv(17, 16, 16, 256);
        assert!((u_min - 1.0 / 16.0).abs() < 1e-6);
        assert!((v_min - 1.0 / 16.0).abs() < 1e-6);
    }

    #[test]
    fn test_block_face_texture_mapping() {
        // Every non-Air block type should have valid tile indices
        let block_types = [
            BlockType::Stone,
            BlockType::Dirt,
            BlockType::Grass,
            BlockType::Sand,
            BlockType::Water,
            BlockType::Wood,
            BlockType::Leaves,
            BlockType::Sandstone,
            BlockType::Snow,
            BlockType::Ice,
            BlockType::Obsidian,
            BlockType::VolcanicRock,
            BlockType::Cactus,
            BlockType::SandDunes,
        ];

        let faces = [Face::Top, Face::Bottom, Face::North, Face::South, Face::East, Face::West];

        for block in &block_types {
            for face in &faces {
                let tile = block_face_texture(*block, *face);
                assert!(
                    tile <= MAX_TILE_INDEX,
                    "Block {:?} face {:?} has tile index {} > MAX_TILE_INDEX {}",
                    block,
                    face,
                    tile,
                    MAX_TILE_INDEX,
                );
            }
        }
    }

    #[test]
    fn test_atlas_dimensions() {
        let image = build_atlas_image(64, 16);
        // 16 tiles * 64 pixels = 1024px per side
        assert_eq!(image.width(), 1024);
        assert_eq!(image.height(), 1024);
        assert_eq!(image.data.len(), 1024 * 1024 * 4);
    }

    #[test]
    fn test_all_textures_within_atlas_bounds() {
        // With grid_size=16, max valid tile index = 16*16 - 1 = 255
        let grid_size: u32 = 16;
        let max_valid = grid_size * grid_size - 1;

        let block_types = [
            BlockType::Air,
            BlockType::Stone,
            BlockType::Dirt,
            BlockType::Grass,
            BlockType::Sand,
            BlockType::Water,
            BlockType::Wood,
            BlockType::Leaves,
            BlockType::Sandstone,
            BlockType::Snow,
            BlockType::Ice,
            BlockType::Obsidian,
            BlockType::VolcanicRock,
            BlockType::Cactus,
            BlockType::SandDunes,
        ];

        for block in &block_types {
            let tex = block_textures(*block);
            assert!(tex.top <= max_valid, "{:?} top index {} exceeds atlas capacity", block, tex.top);
            assert!(tex.bottom <= max_valid, "{:?} bottom index {} exceeds atlas capacity", block, tex.bottom);
            assert!(tex.side <= max_valid, "{:?} side index {} exceeds atlas capacity", block, tex.side);
        }
    }

    #[test]
    fn test_greedy_face_uv_tiling() {
        // Packed atlas UVs must stay within a tile rectangle. We deliberately do NOT
        // scale UVs with greedy quad size here (StandardMaterial can't repeat within
        // a tile). We inset by a full texel to robustly prevent edge sampling bleed.
        let uvs = face_uvs_atlas(0, 16, 64, 1024, 3.0, 2.0);
        let u_tile = 1.0 / 16.0_f32;
        let v_tile = 1.0 / 16.0_f32;

        // Full-texel inset (matches shader strategy)
        let texel = 1.0 / 1024.0_f32;
        let mut inset = texel; // full texel
        inset = inset.min(u_tile * 0.25).min(v_tile * 0.25);

        let u0 = inset;
        let v0 = inset;
        let u1 = u_tile - inset;
        let v1 = v_tile - inset;

        // Bottom-left
        assert!((uvs[0][0] - u0).abs() < 1e-6);
        assert!((uvs[0][1] - v0).abs() < 1e-6);
        // Bottom-right
        assert!((uvs[1][0] - u1).abs() < 1e-6);
        assert!((uvs[1][1] - v0).abs() < 1e-6);
        // Top-right
        assert!((uvs[2][0] - u1).abs() < 1e-6);
        assert!((uvs[2][1] - v1).abs() < 1e-6);
        // Top-left
        assert!((uvs[3][0] - u0).abs() < 1e-6);
        assert!((uvs[3][1] - v1).abs() < 1e-6);
    }

    #[test]
    fn test_grass_has_distinct_faces() {
        let tex = block_textures(BlockType::Grass);
        // Grass top should differ from side
        assert_ne!(tex.top, tex.side, "Grass top and side should be different tiles");
        // Grass bottom should be dirt
        assert_eq!(tex.bottom, block_textures(BlockType::Dirt).top, "Grass bottom should be dirt");
    }

    #[test]
    fn test_wood_has_distinct_faces() {
        let tex = block_textures(BlockType::Wood);
        assert_ne!(tex.top, tex.side, "Wood top and side should be different tiles");
        assert_eq!(tex.top, tex.bottom, "Wood top and bottom should be the same");
    }

    #[test]
    fn test_noise_hash_deterministic() {
        // Same inputs should always produce the same output
        let a = noise_hash(5, 10, 42);
        let b = noise_hash(5, 10, 42);
        assert_eq!(a, b);

        // Different inputs should (usually) differ
        let c = noise_hash(5, 10, 43);
        assert_ne!(a, c);
    }

    #[test]
    fn test_tile_rgba_correct_size() {
        for tile_idx in 0..=MAX_TILE_INDEX + 1 {
            let data = generate_tile_rgba(tile_idx, 64);
            assert_eq!(data.len(), 64 * 64 * 4, "Tile {} has wrong data size", tile_idx);
        }
    }
}
