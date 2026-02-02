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

    // Guard against sampling outside the tile due to float precision.
    // With nearest filtering, sampling exactly on tile borders can still pick
    // a neighboring texel; insetting by half a texel avoids seams/bleed.
    let texel = 1.0 / atlas_size.max(1) as f32;
    let mut inset = 0.5 * texel;
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

    match tile_index {
        // ── 0: Stone ────────────────────────────────────
        0 => {
            for y in 0..tile_size {
                for x in 0..tile_size {
                    let n = noise_hash(x, y, 42) as i16;
                    let base: i16 = 128;
                    let v = (base + (n - 128) / 6).clamp(0, 255) as u8;
                    // Subtle cracks: darken where hash mod is small
                    let crack = noise_hash(x.wrapping_add(5), y.wrapping_add(3), 99);
                    let v = if crack < 18 { v.saturating_sub(40) } else { v };
                    set(&mut data, x, y, v, v, v, 255, tile_size);
                }
            }
        }

        // ── 1: Dirt ─────────────────────────────────────
        1 => {
            for y in 0..tile_size {
                for x in 0..tile_size {
                    let n = noise_hash(x, y, 77) as i16;
                    let r = (115 + (n - 128) / 8).clamp(0, 255) as u8;
                    let g = (82 + (n - 128) / 10).clamp(0, 255) as u8;
                    let b = (56 + (n - 128) / 12).clamp(0, 255) as u8;
                    // Speckles
                    let speck = noise_hash(x, y, 200);
                    let (r, g, b) = if speck < 20 {
                        (r.saturating_add(25), g.saturating_add(15), b.saturating_add(10))
                    } else {
                        (r, g, b)
                    };
                    set(&mut data, x, y, r, g, b, 255, tile_size);
                }
            }
        }

        // ── 2: Grass top ────────────────────────────────
        2 => {
            for y in 0..tile_size {
                for x in 0..tile_size {
                    let n = noise_hash(x, y, 123) as i16;
                    let r = (90 + (n - 128) / 8).clamp(0, 255) as u8;
                    let g = (155 + (n - 128) / 5).clamp(0, 255) as u8;
                    let b = (65 + (n - 128) / 10).clamp(0, 255) as u8;
                    set(&mut data, x, y, r, g, b, 255, tile_size);
                }
            }
        }

        // ── 3: Grass side (green-top / dirt-bottom gradient) ─
        3 => {
            for y in 0..tile_size {
                for x in 0..tile_size {
                    let n = noise_hash(x, y, 150) as i16;
                    let t = y as f32 / tile_size as f32; // 0 = top, 1 = bottom
                    // Blend from grass green to dirt brown
                    let r = ((90.0 * (1.0 - t) + 115.0 * t) as i16 + (n - 128) / 10).clamp(0, 255) as u8;
                    let g = ((155.0 * (1.0 - t) + 82.0 * t) as i16 + (n - 128) / 8).clamp(0, 255) as u8;
                    let b = ((65.0 * (1.0 - t) + 56.0 * t) as i16 + (n - 128) / 12).clamp(0, 255) as u8;
                    set(&mut data, x, y, r, g, b, 255, tile_size);
                }
            }
        }

        // ── 4: Sand ─────────────────────────────────────
        4 => {
            for y in 0..tile_size {
                for x in 0..tile_size {
                    let n = noise_hash(x, y, 210) as i16;
                    let r = (230 + (n - 128) / 8).clamp(0, 255) as u8;
                    let g = (217 + (n - 128) / 8).clamp(0, 255) as u8;
                    let b = (153 + (n - 128) / 8).clamp(0, 255) as u8;
                    set(&mut data, x, y, r, g, b, 255, tile_size);
                }
            }
        }

        // ── 5: Water ────────────────────────────────────
        5 => {
            for y in 0..tile_size {
                for x in 0..tile_size {
                    let n = noise_hash(x, y, 55) as i16;
                    let r = (51 + (n - 128) / 12).clamp(0, 255) as u8;
                    let g = (102 + (n - 128) / 10).clamp(0, 255) as u8;
                    let b = (204 + (n - 128) / 8).clamp(0, 255) as u8;
                    set(&mut data, x, y, r, g, b, 204, tile_size); // semi-transparent
                }
            }
        }

        // ── 6: Wood top/bottom (rings) ──────────────────
        6 => {
            let cx = tile_size as f32 / 2.0;
            let cy = tile_size as f32 / 2.0;
            for y in 0..tile_size {
                for x in 0..tile_size {
                    let dx = x as f32 - cx;
                    let dy = y as f32 - cy;
                    let dist = (dx * dx + dy * dy).sqrt();
                    let ring = ((dist * 1.5) as u32) % 2;
                    let n = noise_hash(x, y, 170) as i16;
                    let (r, g, b) = if ring == 0 {
                        (
                            (160 + (n - 128) / 10).clamp(0, 255) as u8,
                            (120 + (n - 128) / 12).clamp(0, 255) as u8,
                            (70 + (n - 128) / 14).clamp(0, 255) as u8,
                        )
                    } else {
                        (
                            (130 + (n - 128) / 10).clamp(0, 255) as u8,
                            (90 + (n - 128) / 12).clamp(0, 255) as u8,
                            (50 + (n - 128) / 14).clamp(0, 255) as u8,
                        )
                    };
                    set(&mut data, x, y, r, g, b, 255, tile_size);
                }
            }
        }

        // ── 7: Wood side / bark ─────────────────────────
        7 => {
            for y in 0..tile_size {
                for x in 0..tile_size {
                    let n = noise_hash(x, y, 180) as i16;
                    // Vertical line pattern
                    let stripe = ((x as i16 * 3 / tile_size as i16) % 2) as i16;
                    let r = (128 + stripe * 15 + (n - 128) / 10).clamp(0, 255) as u8;
                    let g = (89 + stripe * 10 + (n - 128) / 12).clamp(0, 255) as u8;
                    let b = (51 + stripe * 5 + (n - 128) / 14).clamp(0, 255) as u8;
                    set(&mut data, x, y, r, g, b, 255, tile_size);
                }
            }
        }

        // ── 8: Leaves ───────────────────────────────────
        8 => {
            for y in 0..tile_size {
                for x in 0..tile_size {
                    let n = noise_hash(x, y, 88);
                    // "Holes" — make some pixels much darker (simulating see-through)
                    let hole = noise_hash(x.wrapping_add(11), y.wrapping_add(7), 33);
                    if hole < 35 {
                        set(&mut data, x, y, 30, 60, 20, 200, tile_size);
                    } else {
                        let n = n as i16;
                        let r = (51 + (n - 128) / 8).clamp(0, 255) as u8;
                        let g = (128 + (n - 128) / 5).clamp(0, 255) as u8;
                        let b = (38 + (n - 128) / 10).clamp(0, 255) as u8;
                        set(&mut data, x, y, r, g, b, 230, tile_size);
                    }
                }
            }
        }

        // ── 9: Sandstone ────────────────────────────────
        9 => {
            for y in 0..tile_size {
                for x in 0..tile_size {
                    let n = noise_hash(x, y, 250) as i16;
                    // Horizontal layers
                    let layer = (y * 4 / tile_size) % 2;
                    let base_r: i16 = if layer == 0 { 210 } else { 195 };
                    let base_g: i16 = if layer == 0 { 186 } else { 175 };
                    let base_b: i16 = if layer == 0 { 135 } else { 128 };
                    let r = (base_r + (n - 128) / 10).clamp(0, 255) as u8;
                    let g = (base_g + (n - 128) / 10).clamp(0, 255) as u8;
                    let b = (base_b + (n - 128) / 10).clamp(0, 255) as u8;
                    set(&mut data, x, y, r, g, b, 255, tile_size);
                }
            }
        }

        // ── 10: Snow ────────────────────────────────────
        10 => {
            for y in 0..tile_size {
                for x in 0..tile_size {
                    let n = noise_hash(x, y, 300) as i16;
                    let r = (242 + (n - 128) / 20).clamp(0, 255) as u8;
                    let g = (242 + (n - 128) / 20).clamp(0, 255) as u8;
                    let b = (248 + (n - 128) / 25).clamp(0, 255) as u8;
                    set(&mut data, x, y, r, g, b, 255, tile_size);
                }
            }
        }

        // ── 11: Ice ─────────────────────────────────────
        11 => {
            for y in 0..tile_size {
                for x in 0..tile_size {
                    let n = noise_hash(x, y, 310) as i16;
                    let r = (178 + (n - 128) / 10).clamp(0, 255) as u8;
                    let g = (217 + (n - 128) / 10).clamp(0, 255) as u8;
                    let b = (242 + (n - 128) / 12).clamp(0, 255) as u8;
                    set(&mut data, x, y, r, g, b, 230, tile_size);
                }
            }
        }

        // ── 12: Obsidian ────────────────────────────────
        12 => {
            for y in 0..tile_size {
                for x in 0..tile_size {
                    let n = noise_hash(x, y, 320) as i16;
                    let r = (25 + (n - 128) / 14).clamp(0, 255) as u8;
                    let g = (20 + (n - 128) / 16).clamp(0, 255) as u8;
                    let b = (31 + (n - 128) / 12).clamp(0, 255) as u8;
                    set(&mut data, x, y, r, g, b, 255, tile_size);
                }
            }
        }

        // ── 13: VolcanicRock ────────────────────────────
        13 => {
            for y in 0..tile_size {
                for x in 0..tile_size {
                    let n = noise_hash(x, y, 330) as i16;
                    let base_r: i16 = 77;
                    let base_g: i16 = 46;
                    let base_b: i16 = 38;
                    // Orange/red veins
                    let vein = noise_hash(x.wrapping_add(3), y.wrapping_add(9), 335);
                    let (r, g, b) = if vein < 22 {
                        (
                            (200 + (n - 128) / 10).clamp(0, 255) as u8,
                            (80 + (n - 128) / 12).clamp(0, 255) as u8,
                            (20 + (n - 128) / 14).clamp(0, 255) as u8,
                        )
                    } else {
                        (
                            (base_r + (n - 128) / 10).clamp(0, 255) as u8,
                            (base_g + (n - 128) / 12).clamp(0, 255) as u8,
                            (base_b + (n - 128) / 14).clamp(0, 255) as u8,
                        )
                    };
                    set(&mut data, x, y, r, g, b, 255, tile_size);
                }
            }
        }

        // ── 14: Cactus top ──────────────────────────────
        14 => {
            let cx = tile_size as f32 / 2.0;
            let cy = tile_size as f32 / 2.0;
            for y in 0..tile_size {
                for x in 0..tile_size {
                    let n = noise_hash(x, y, 340) as i16;
                    let dx = (x as f32 - cx).abs();
                    let dy = (y as f32 - cy).abs();
                    let dist = dx.max(dy);
                    let rim = dist > (tile_size as f32 / 2.0 - 2.0);
                    let (r, g, b) = if rim {
                        (
                            (50 + (n - 128) / 12).clamp(0, 255) as u8,
                            (110 + (n - 128) / 10).clamp(0, 255) as u8,
                            (40 + (n - 128) / 14).clamp(0, 255) as u8,
                        )
                    } else {
                        (
                            (75 + (n - 128) / 10).clamp(0, 255) as u8,
                            (145 + (n - 128) / 8).clamp(0, 255) as u8,
                            (60 + (n - 128) / 12).clamp(0, 255) as u8,
                        )
                    };
                    set(&mut data, x, y, r, g, b, 255, tile_size);
                }
            }
        }

        // ── 15: Cactus side ─────────────────────────────
        15 => {
            for y in 0..tile_size {
                for x in 0..tile_size {
                    let n = noise_hash(x, y, 350) as i16;
                    // Vertical stripes
                    let stripe = (x * 4 / tile_size) % 2;
                    let (r, g, b) = if stripe == 0 {
                        (
                            (64 + (n - 128) / 10).clamp(0, 255) as u8,
                            (140 + (n - 128) / 8).clamp(0, 255) as u8,
                            (51 + (n - 128) / 12).clamp(0, 255) as u8,
                        )
                    } else {
                        (
                            (55 + (n - 128) / 10).clamp(0, 255) as u8,
                            (120 + (n - 128) / 8).clamp(0, 255) as u8,
                            (45 + (n - 128) / 12).clamp(0, 255) as u8,
                        )
                    };
                    set(&mut data, x, y, r, g, b, 255, tile_size);
                }
            }
        }

        // ── 16: SandDunes ───────────────────────────────
        16 => {
            for y in 0..tile_size {
                for x in 0..tile_size {
                    let n = noise_hash(x, y, 360) as i16;
                    // Wave pattern via sin-like approximation
                    let wave = ((x as f32 + y as f32 * 0.5).sin() * 0.5 + 0.5) * 20.0;
                    let r = (217 + wave as i16 + (n - 128) / 12).clamp(0, 255) as u8;
                    let g = (199 + wave as i16 + (n - 128) / 12).clamp(0, 255) as u8;
                    let b = (140 + wave as i16 / 2 + (n - 128) / 14).clamp(0, 255) as u8;
                    set(&mut data, x, y, r, g, b, 255, tile_size);
                }
            }
        }

        // ── Unused tiles: magenta debug fill ────────────
        _ => {
            for y in 0..tile_size {
                for x in 0..tile_size {
                    set(&mut data, x, y, 255, 0, 255, 255, tile_size);
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
        let image = build_atlas_image(16, 16);
        // 16 tiles * 16 pixels = 256px per side
        assert_eq!(image.width(), 256);
        assert_eq!(image.height(), 256);
        assert_eq!(image.data.len(), 256 * 256 * 4);
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
        // a tile). We also inset by half a texel to avoid edge sampling bleed.
        let uvs = face_uvs_atlas(0, 16, 16, 256, 3.0, 2.0);
        let u_tile = 1.0 / 16.0_f32;
        let v_tile = 1.0 / 16.0_f32;

        let texel = 1.0 / 256.0_f32;
        let mut inset = 0.5 * texel;
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
            let data = generate_tile_rgba(tile_idx, 16);
            assert_eq!(data.len(), 16 * 16 * 4, "Tile {} has wrong data size", tile_idx);
        }
    }
}
