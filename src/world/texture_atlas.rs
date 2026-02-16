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

        // Ores - distinct tile indices for each
        BlockType::CopperOre => BlockTextures { top: 17, bottom: 17, side: 17 },
        BlockType::IronOre => BlockTextures { top: 18, bottom: 18, side: 18 },
        BlockType::SilverOre => BlockTextures { top: 19, bottom: 19, side: 19 },
        BlockType::GoldOre => BlockTextures { top: 20, bottom: 20, side: 20 },
        BlockType::Mud => BlockTextures { top: 1, bottom: 1, side: 1 },
        BlockType::Clay => BlockTextures { top: 1, bottom: 1, side: 1 },
        BlockType::Mycelium => BlockTextures { top: 2, bottom: 1, side: 3 },
        BlockType::TerracottaRed => BlockTextures { top: 0, bottom: 0, side: 0 },
        BlockType::TerracottaOrange => BlockTextures { top: 0, bottom: 0, side: 0 },
        BlockType::PackedDirt => BlockTextures { top: 1, bottom: 1, side: 1 },
    }
}

/// The highest tile index used by any block face.
pub const MAX_TILE_INDEX: u32 = 20;

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

/// Cell/Voronoi noise — creates organic cobblestone/cell patterns.
///
/// Returns 0.0 near cell centers and approaches 1.0 at cell boundaries.
fn cell_noise(x: u32, y: u32, tile_size: u32, seed: u32, num_cells: u32) -> f32 {
    let mut min_dist = f32::MAX;
    let ts = tile_size as f32;
    for i in 0..num_cells {
        let cx = (noise_hash(i, 0, seed) as f32 / 255.0) * ts;
        let cy = (noise_hash(i, 1, seed) as f32 / 255.0) * ts;
        let dx = (x as f32 - cx).abs().min((x as f32 - cx + ts).abs()).min((x as f32 - cx - ts).abs());
        let dy = (y as f32 - cy).abs().min((y as f32 - cy + ts).abs()).min((y as f32 - cy - ts).abs());
        min_dist = min_dist.min((dx * dx + dy * dy).sqrt());
    }
    (min_dist / ts * 4.0).min(1.0)
}

/// Cell noise returning (F1 distance, nearest cell index) for varied cell colors.
fn cell_noise_with_id(x: u32, y: u32, tile_size: u32, seed: u32, num_cells: u32) -> (f32, f32, u32) {
    let mut min_dist = f32::MAX;
    let mut second_dist = f32::MAX;
    let mut nearest = 0u32;
    let ts = tile_size as f32;
    for i in 0..num_cells {
        let cx = (noise_hash(i, 0, seed) as f32 / 255.0) * ts;
        let cy = (noise_hash(i, 1, seed) as f32 / 255.0) * ts;
        let dx = (x as f32 - cx).abs().min((x as f32 - cx + ts).abs()).min((x as f32 - cx - ts).abs());
        let dy = (y as f32 - cy).abs().min((y as f32 - cy + ts).abs()).min((y as f32 - cy - ts).abs());
        let d = (dx * dx + dy * dy).sqrt();
        if d < min_dist {
            second_dist = min_dist;
            min_dist = d;
            nearest = i;
        } else if d < second_dist {
            second_dist = d;
        }
    }
    ((min_dist / ts * 4.0).min(1.0), (second_dist / ts * 4.0).min(1.0), nearest)
}

/// Line distance — for crack/vein rendering.
fn dist_to_line(x: f32, y: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 0.001 { return ((x-x1)*(x-x1) + (y-y1)*(y-y1)).sqrt(); }
    let t = ((x - x1) * dx + (y - y1) * dy) / len_sq;
    let t = t.clamp(0.0, 1.0);
    ((x - x1 - t * dx).powi(2) + (y - y1 - t * dy).powi(2)).sqrt()
}

/// Generate the RGBA pixel data for a single tile.
///
/// Returns a `Vec<u8>` with `tile_size * tile_size * 4` bytes.
///
/// Tile index assignments (must match `block_textures`):
///   0  = Stone (grey + cell noise cobblestone)
///   1  = Dirt (brown + pebbles + organic patches)
///   2  = Grass top (green + blade streaks + patches)
///   3  = Grass side (green-top / dirt-bottom with blade tips)
///   4  = Sand (tan + diagonal ripple + grain)
///   5  = Water (blue, caustic cell noise, semi-transparent)
///   6  = Wood top/bottom (off-center concentric rings)
///   7  = Wood side / bark (vertical furrows + horizontal cracks)
///   8  = Leaves (leaf blobs + gap holes)
///   9  = Sandstone (horizontal strata layers)
///  10  = Snow (white + blue shadow + sparkle)
///  11  = Ice (light blue + crack network + bubbles)
///  12  = Obsidian (dark + glossy streaks + purple tint)
///  13  = VolcanicRock (dark grey + orange-red vein network)
///  14  = Cactus top (star pattern + rim + thorns)
///  15  = Cactus side (vertical ribs + thorns)
///  16  = SandDunes (golden wind ripple waves)
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
    let tsf = tile_size as f32;

    match tile_index {
        // ── 0: Stone — cell-noise cobblestone with cracks and mineral speckles ──
        0 => {
            let num_cells: u32 = 7;
            let cell_seed: u32 = 42;
            for y in 0..ts {
                for x in 0..ts {
                    let (f1, f2, nearest_id) = cell_noise_with_id(x, y, ts, cell_seed, num_cells);

                    // Crack at cell boundaries where F2 - F1 is small
                    let boundary = f2 - f1;
                    let crack_threshold = 0.12;
                    let on_crack = boundary < crack_threshold;

                    // Per-cell grey tone variation (120-140 range)
                    let cell_tone = noise_hash(nearest_id, 2, cell_seed) as f32 / 255.0;
                    let base_grey = 120.0 + cell_tone * 20.0;

                    // Fine per-pixel noise
                    let fine = noise_hash(x, y, 43) as f32 / 255.0;
                    let mn = multi_noise(x, y, 44);
                    let mut grey = base_grey + (fine - 0.5) * 12.0 + (mn - 0.5) * 10.0;

                    // Subtle shading within cells (darker toward edges)
                    grey -= f1 * 8.0;

                    if on_crack {
                        let crack_intensity = 1.0 - (boundary / crack_threshold);
                        grey -= 40.0 * crack_intensity + fine * 8.0;
                    }

                    // Mineral speckles: occasional brighter pixels
                    let speck = noise_hash(x.wrapping_add(7), y.wrapping_add(3), 105);
                    if speck < 6 {
                        grey += 28.0;
                    }

                    let v = grey.clamp(0.0, 255.0) as u8;
                    set(&mut data, x, y, v, v, v, 255, ts);
                }
            }
        }

        // ── 1: Dirt — warm brown with pebbles, organic patches, horizontal layering ──
        1 => {
            // Pre-generate 4 pebble positions
            let mut pebbles = [(0.0f32, 0.0f32, 0.0f32); 5];
            for i in 0..5u32 {
                pebbles[i as usize] = (
                    noise_hash(i, 0, 200) as f32 / 255.0 * tsf,
                    noise_hash(i, 1, 200) as f32 / 255.0 * tsf,
                    2.0 + (noise_hash(i, 2, 200) as f32 / 255.0) * 1.2,
                );
            }

            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 77);
                    let variation = (mn - 0.5) * 28.0;
                    let mut r = 110.0 + variation;
                    let mut g = 78.0 + variation * 0.8;
                    let mut b = 55.0 + variation * 0.6;

                    // Subtle horizontal layering
                    let layer_val = ((y as f32 * 3.0 * 3.14159 / tsf).sin() * 0.3 + 0.5) * 5.0;
                    r += layer_val;
                    g += layer_val * 0.7;
                    b += layer_val * 0.4;

                    // Pebble dots (lighter, with smooth falloff)
                    for &(px, py, radius) in &pebbles {
                        let dx = (x as f32 - px).abs().min((x as f32 - px + tsf).abs()).min((x as f32 - px - tsf).abs());
                        let dy = (y as f32 - py).abs().min((y as f32 - py + tsf).abs()).min((y as f32 - py - tsf).abs());
                        let dist = (dx * dx + dy * dy).sqrt();
                        if dist < radius {
                            let t = 1.0 - dist / radius;
                            r += 22.0 * t;
                            g += 16.0 * t;
                            b += 10.0 * t;
                        }
                    }

                    // Dark organic patches
                    let org = noise_hash(x / 5, y / 5, 215);
                    let org_fine = noise_hash(x, y, 216) as f32 / 255.0;
                    if org < 22 {
                        let dark = 14.0 * org_fine;
                        r -= dark;
                        g -= dark * 0.8;
                        b -= dark * 0.5;
                    }

                    // Fine grain noise
                    let grain = noise_hash(x, y, 78) as f32 / 255.0;
                    r += (grain - 0.5) * 8.0;
                    g += (grain - 0.5) * 6.0;
                    b += (grain - 0.5) * 4.0;

                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                }
            }
        }

        // ── 2: Grass top — rich green with blade streaks, color patches, highlights ──
        2 => {
            // Pre-generate 18 blade streaks (1px wide, 3-5px long, random angles)
            let blade_count = 18usize;
            let mut blades = [(0.0f32, 0.0f32, 0.0f32, 0.0f32); 18];
            for i in 0..blade_count {
                let bx = noise_hash(i as u32, 0, 130) as f32 / 255.0 * tsf;
                let by = noise_hash(i as u32, 1, 130) as f32 / 255.0 * tsf;
                let angle = noise_hash(i as u32, 2, 130) as f32 / 255.0 * 3.14159;
                let len = 3.0 + (noise_hash(i as u32, 3, 130) as f32 / 255.0) * 2.0;
                blades[i] = (bx, by, bx + angle.cos() * len, by + angle.sin() * len);
            }

            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 123);

                    // Large-scale color patches (yellow-green vs blue-green)
                    let patch = noise_hash(x / 8, y / 8, 125) as f32 / 255.0;
                    let yellow_shift = (patch - 0.5) * 14.0;

                    let mut r = 88.0 + (mn - 0.5) * 20.0 + yellow_shift;
                    let mut g = 158.0 + (mn - 0.5) * 28.0;
                    let mut b = 68.0 + (mn - 0.5) * 15.0 - yellow_shift * 0.6;

                    // Check blade streaks (darker)
                    let xf = x as f32;
                    let yf = y as f32;
                    for &(x1, y1, x2, y2) in &blades {
                        let d = dist_to_line(xf, yf, x1, y1, x2, y2);
                        if d < 0.9 {
                            r -= 20.0;
                            g -= 10.0;
                            b -= 16.0;
                            break;
                        }
                    }

                    // Bright highlight pixels (~2%)
                    let highlight = noise_hash(x.wrapping_add(5), y.wrapping_add(9), 145);
                    if highlight < 5 {
                        r -= 5.0;
                        g += 24.0;
                        b -= 3.0;
                    }

                    // Fine per-pixel noise
                    let grain = noise_hash(x, y, 124) as f32 / 255.0;
                    r += (grain - 0.5) * 6.0;
                    g += (grain - 0.5) * 8.0;
                    b += (grain - 0.5) * 5.0;

                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                }
            }
        }

        // ── 3: Grass side — green top ~18% with dangling blade tips, jagged transition, dirt bottom ──
        3 => {
            let green_rows = (tsf * 0.18) as u32;

            // Per-column blade tip extensions (2-5px below green boundary)
            let mut blade_tips: Vec<u32> = Vec::with_capacity(ts as usize);
            for col in 0..ts {
                let ext = 2 + (noise_hash(col, 0, 175) as u32 % 4);
                blade_tips.push(green_rows + ext);
            }

            // Root-like dark streaks from transition downward
            let mut roots = [(0u32, 0u32, 0u32); 5];
            for i in 0..5u32 {
                roots[i as usize] = (
                    (noise_hash(i, 0, 176) as u32).wrapping_mul(ts) / 255,
                    green_rows + 4 + (noise_hash(i, 1, 176) as u32 % 5),
                    6 + (noise_hash(i, 2, 176) as u32 % 10),
                );
            }

            let transition_size = ts / 16; // 3-4px at 64

            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 155);
                    let blade_tip = blade_tips[x as usize];

                    if y < green_rows {
                        // Solid green section
                        let r = (85.0 + (mn - 0.5) * 18.0).clamp(0.0, 255.0);
                        let g = (155.0 + (mn - 0.5) * 25.0).clamp(0.0, 255.0);
                        let b = (62.0 + (mn - 0.5) * 14.0).clamp(0.0, 255.0);
                        set(&mut data, x, y, r as u8, g as u8, b as u8, 255, ts);
                    } else if y < blade_tip {
                        // Dangling blade tip zone — intermittent green pixels
                        let blade_noise = noise_hash(x, y, 177);
                        if blade_noise < 150 {
                            let fade = (y - green_rows) as f32 / (blade_tip - green_rows).max(1) as f32;
                            let r = (80.0 + (mn - 0.5) * 14.0 - fade * 10.0).clamp(0.0, 255.0);
                            let g = (145.0 + (mn - 0.5) * 20.0 - fade * 18.0).clamp(0.0, 255.0);
                            let b = (56.0 + (mn - 0.5) * 12.0).clamp(0.0, 255.0);
                            set(&mut data, x, y, r as u8, g as u8, b as u8, 255, ts);
                        } else {
                            let variation = (mn - 0.5) * 25.0;
                            let r = (110.0 + variation).clamp(0.0, 255.0);
                            let g = (78.0 + variation * 0.8).clamp(0.0, 255.0);
                            let b = (55.0 + variation * 0.6).clamp(0.0, 255.0);
                            set(&mut data, x, y, r as u8, g as u8, b as u8, 255, ts);
                        }
                    } else if y < blade_tip + transition_size {
                        // Jagged transition zone
                        let t = (y - blade_tip) as f32 / transition_size.max(1) as f32;
                        let jag = noise_hash(x, y, 178) as f32 / 255.0;
                        let blend = (t + (jag - 0.5) * 0.4).clamp(0.0, 1.0);

                        let gr = 78.0 + (mn - 0.5) * 14.0;
                        let gg = 138.0 + (mn - 0.5) * 20.0;
                        let gb = 54.0 + (mn - 0.5) * 12.0;
                        let dr = 110.0 + (mn - 0.5) * 25.0;
                        let dg = 78.0 + (mn - 0.5) * 20.0;
                        let db = 55.0 + (mn - 0.5) * 14.0;

                        let r = (gr * (1.0 - blend) + dr * blend).clamp(0.0, 255.0);
                        let g = (gg * (1.0 - blend) + dg * blend).clamp(0.0, 255.0);
                        let b = (gb * (1.0 - blend) + db * blend).clamp(0.0, 255.0);
                        set(&mut data, x, y, r as u8, g as u8, b as u8, 255, ts);
                    } else {
                        // Dirt section (matching tile 1 style)
                        let variation = (mn - 0.5) * 28.0;
                        let mut r = 110.0 + variation;
                        let mut g = 78.0 + variation * 0.8;
                        let mut b = 55.0 + variation * 0.6;

                        // Root-like dark streaks
                        for &(rx, ry_start, rlen) in &roots {
                            if x.abs_diff(rx) <= 1 && y >= ry_start && y < ry_start + rlen {
                                r -= 20.0;
                                g -= 14.0;
                                b -= 9.0;
                            }
                        }

                        // Pebble dots
                        let peb = noise_hash(x / 3, y / 3, 200);
                        let peb_fine = noise_hash(x, y, 201);
                        if peb < 25 && peb_fine < 180 {
                            r += 18.0;
                            g += 12.0;
                            b += 7.0;
                        }

                        // Fine grain
                        let grain = noise_hash(x, y, 156) as f32 / 255.0;
                        r += (grain - 0.5) * 8.0;
                        g += (grain - 0.5) * 6.0;

                        set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                    }
                }
            }
        }

        // ── 4: Sand — warm tan with diagonal ripple pattern and grain ──
        4 => {
            let period = tsf / 5.3; // ~12px at 64
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 210);

                    // Diagonal ripple pattern
                    let ripple = ((x as f32 * 0.7 + y as f32 * 0.3) * 6.2832 / period).sin() * 0.5 + 0.5;

                    let mut r = 220.0 + (mn - 0.5) * 16.0 + ripple * 10.0;
                    let mut g = 210.0 + (mn - 0.5) * 14.0 + ripple * 8.0;
                    let mut b = 155.0 + (mn - 0.5) * 12.0 + ripple * 5.0;

                    // Fine grain noise
                    let grain = noise_hash(x, y, 212) as f32 / 255.0;
                    r += (grain - 0.5) * 10.0;
                    g += (grain - 0.5) * 8.0;
                    b += (grain - 0.5) * 6.0;

                    // Scattered darker and lighter grains
                    let scatter = noise_hash(x.wrapping_add(3), y.wrapping_add(7), 213);
                    if scatter < 12 {
                        r += 14.0;
                        g += 11.0;
                        b += 8.0;
                    } else if scatter > 243 {
                        r -= 12.0;
                        g -= 10.0;
                        b -= 7.0;
                    }

                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                }
            }
        }

        // ── 5: Water — deep blue with caustic cell-noise patches, semi-transparent ──
        5 => {
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 55);

                    // Caustic-like lighter patches from cell noise
                    let cn = cell_noise(x, y, ts, 55, 14);
                    // Caustics are bright network lines at cell boundaries (high cn)
                    let caustic = cn * cn; // sharpen the effect

                    let r = (50.0 + caustic * 30.0 + (mn - 0.5) * 10.0).clamp(0.0, 255.0) as u8;
                    let g = (105.0 + caustic * 35.0 + (mn - 0.5) * 14.0).clamp(0.0, 255.0) as u8;
                    let b = (195.0 + caustic * 22.0 + (mn - 0.5) * 8.0).clamp(0.0, 255.0) as u8;

                    // Alpha varies 180-210 based on caustic pattern
                    let alpha = (180.0 + caustic * 30.0).clamp(180.0, 210.0) as u8;
                    set(&mut data, x, y, r, g, b, alpha, ts);
                }
            }
        }

        // ── 6: Wood top — off-center concentric growth rings with radial grain ──
        6 => {
            // Slightly off-center ring origin
            let cx = tsf * 0.45;
            let cy = tsf * 0.52;
            let num_rings = 6.0;

            for y in 0..ts {
                for x in 0..ts {
                    let dx = x as f32 - cx;
                    let dy = y as f32 - cy;
                    let dist = (dx * dx + dy * dy).sqrt();
                    let mn = multi_noise(x, y, 170);

                    // Concentric growth rings — alternate lighter/darker bands
                    let ring_freq = num_rings * 3.14159 / (tsf * 0.55);
                    let ring_val = (dist * ring_freq).sin() * 0.5 + 0.5; // 0-1

                    // Base tan-brown with ring modulation
                    let lighter = ring_val > 0.5;
                    let ring_t = if lighter { (ring_val - 0.5) * 2.0 } else { (0.5 - ring_val) * 2.0 };

                    let base_r = if lighter { 155.0 + ring_t * 12.0 } else { 130.0 - ring_t * 10.0 };
                    let base_g = if lighter { 115.0 + ring_t * 8.0 } else { 90.0 - ring_t * 8.0 };
                    let base_b = if lighter { 70.0 + ring_t * 5.0 } else { 52.0 - ring_t * 5.0 };

                    // Fine radial grain using angle
                    let angle = dy.atan2(dx);
                    let radial_grain = noise_hash(
                        ((angle * 10.0 + 50.0) as i32).unsigned_abs(),
                        (dist * 2.0) as u32,
                        171,
                    ) as f32 / 255.0;

                    let r = (base_r + (mn - 0.5) * 12.0 + (radial_grain - 0.5) * 8.0).clamp(0.0, 255.0) as u8;
                    let g = (base_g + (mn - 0.5) * 10.0 + (radial_grain - 0.5) * 6.0).clamp(0.0, 255.0) as u8;
                    let b = (base_b + (mn - 0.5) * 8.0 + (radial_grain - 0.5) * 4.0).clamp(0.0, 255.0) as u8;

                    set(&mut data, x, y, r, g, b, 255, ts);
                }
            }
        }

        // ── 7: Wood bark — dark brown vertical furrows with horizontal cracks ──
        7 => {
            // Pre-generate 3 horizontal crack lines
            let mut cracks = [(0.0f32, 0.0f32, 0.0f32, 0.0f32); 3];
            for i in 0..3u32 {
                let y_pos = tsf * 0.15 + (noise_hash(i, 0, 186) as f32 / 255.0) * tsf * 0.7;
                let y_end = y_pos + (noise_hash(i, 1, 186) as f32 / 255.0 - 0.5) * 4.0;
                cracks[i as usize] = (0.0, y_pos, tsf, y_end);
            }

            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 180);
                    let xf = x as f32;
                    let yf = y as f32;

                    // 4-5 vertical furrow strips with non-uniform spacing
                    let phase_mod = noise_hash(x / 6, 0, 183) as f32 / 255.0 * 1.5;
                    let strip_val = ((xf * 4.5 * 6.2832 / tsf + phase_mod).cos() + 1.0) * 0.5;

                    // Dark gaps where strip_val is low
                    let (base_r, base_g, base_b) = if strip_val < 0.2 {
                        (62.0, 40.0, 26.0) // dark gap
                    } else {
                        let brightness = strip_val * 16.0;
                        (100.0 + brightness, 68.0 + brightness * 0.7, 42.0 + brightness * 0.4)
                    };

                    let mut r = base_r + (mn - 0.5) * 14.0;
                    let mut g = base_g + (mn - 0.5) * 10.0;
                    let mut b = base_b + (mn - 0.5) * 7.0;

                    // Horizontal cracks
                    for &(x1, y1, x2, y2) in &cracks {
                        let d = dist_to_line(xf, yf, x1, y1, x2, y2);
                        if d < 1.8 {
                            let intensity = 1.0 - d / 1.8;
                            r -= 28.0 * intensity;
                            g -= 20.0 * intensity;
                            b -= 14.0 * intensity;
                        }
                    }

                    // Fine vertical grain
                    let grain = noise_hash(x, y, 188) as f32 / 255.0;
                    r += (grain - 0.5) * 6.0;
                    g += (grain - 0.5) * 4.0;

                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                }
            }
        }

        // ── 8: Leaves — random leaf blobs with gap holes and color variety ──
        8 => {
            // Pre-generate 10 leaf blob centers
            let blob_count = 10usize;
            let mut blobs = [(0.0f32, 0.0f32, 0.0f32, 0u32); 10]; // (x, y, radius, color_seed)
            for i in 0..blob_count {
                blobs[i] = (
                    noise_hash(i as u32, 0, 88) as f32 / 255.0 * tsf,
                    noise_hash(i as u32, 1, 88) as f32 / 255.0 * tsf,
                    3.0 + (noise_hash(i as u32, 2, 88) as f32 / 255.0) * 2.5,
                    noise_hash(i as u32, 3, 88) as u32,
                );
            }

            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 89);

                    // Check if pixel is a dark gap (~15%)
                    let gap_coarse = noise_hash(x / 3, y / 3, 33);
                    let gap_fine = noise_hash(x, y, 34);
                    let is_gap = gap_coarse < 28 && gap_fine < 140;

                    if is_gap {
                        set(&mut data, x, y, 25, 50, 18, 170, ts);
                    } else {
                        // Find nearest leaf blob for color variety
                        let xf = x as f32;
                        let yf = y as f32;
                        let mut best_blob_seed = 0u32;
                        let mut best_dist = f32::MAX;
                        for &(bx, by, _radius, cseed) in &blobs {
                            let dx = (xf - bx).abs().min((xf - bx + tsf).abs()).min((xf - bx - tsf).abs());
                            let dy = (yf - by).abs().min((yf - by + tsf).abs()).min((yf - by - tsf).abs());
                            let d = dx * dx + dy * dy;
                            if d < best_dist {
                                best_dist = d;
                                best_blob_seed = cseed;
                            }
                        }

                        // Per-blob color variation
                        let hue_shift = (best_blob_seed as f32 / 255.0 - 0.5) * 20.0;
                        let r = (48.0 + (mn - 0.5) * 24.0 + hue_shift * 0.3).clamp(0.0, 255.0) as u8;
                        let g = (130.0 + (mn - 0.5) * 35.0 + hue_shift).clamp(0.0, 255.0) as u8;
                        let b = (36.0 + (mn - 0.5) * 16.0 - hue_shift * 0.4).clamp(0.0, 255.0) as u8;
                        set(&mut data, x, y, r, g, b, 230, ts);
                    }
                }
            }
        }

        // ── 9: Sandstone — horizontal stratification layers with wavy boundaries ──
        9 => {
            // Pre-compute 7 layer boundary y-positions (wavy)
            let num_layers = 7u32;
            // Layer colors (warm tans with varying hues)
            let layer_colors: [(f32, f32, f32); 7] = [
                (215.0, 192.0, 140.0), // cream
                (200.0, 175.0, 128.0), // tan
                (208.0, 185.0, 135.0), // light tan
                (192.0, 168.0, 122.0), // darker tan
                (210.0, 188.0, 138.0), // warm
                (196.0, 172.0, 126.0), // muted
                (205.0, 182.0, 132.0), // medium
            ];

            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 250);

                    // Determine layer with wavy boundaries
                    let wave = noise_hash(x / 4, 0, 251) as f32 / 255.0 * 3.0;
                    let effective_y = y as f32 + wave;
                    let layer_height = tsf / num_layers as f32;
                    let layer_idx = ((effective_y / layer_height) as u32).min(num_layers - 1);

                    let (base_r, base_g, base_b) = layer_colors[layer_idx as usize];

                    // Boundary darkening at layer transitions
                    let in_layer_pos = (effective_y % layer_height) / layer_height;
                    let boundary_dark = if in_layer_pos < 0.08 || in_layer_pos > 0.92 {
                        8.0
                    } else {
                        0.0
                    };

                    let r = (base_r + (mn - 0.5) * 14.0 - boundary_dark).clamp(0.0, 255.0) as u8;
                    let g = (base_g + (mn - 0.5) * 12.0 - boundary_dark).clamp(0.0, 255.0) as u8;
                    let b = (base_b + (mn - 0.5) * 10.0 - boundary_dark).clamp(0.0, 255.0) as u8;
                    set(&mut data, x, y, r, g, b, 255, ts);
                }
            }
        }

        // ── 10: Snow — near-white with subtle blue shadows and sparkle pixels ──
        10 => {
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 300);

                    // Subtle blue shadow patches
                    let shadow = noise_hash(x / 5, y / 5, 305) as f32 / 255.0;
                    let blue_shift = shadow * 5.0;

                    let mut r = (244.0 + (mn - 0.5) * 7.0 - blue_shift).clamp(0.0, 255.0);
                    let mut g = (244.0 + (mn - 0.5) * 7.0 - blue_shift * 0.4).clamp(0.0, 255.0);
                    let mut b = (250.0 + (mn - 0.5) * 5.0).clamp(0.0, 255.0);

                    // Sparkle pixels (~3%)
                    let sparkle = noise_hash(x.wrapping_add(13), y.wrapping_add(29), 310);
                    if sparkle < 8 { // 8/256 ~ 3%
                        r = 255.0;
                        g = 255.0;
                        b = 255.0;
                    }

                    // Very subtle surface texture
                    let grain = noise_hash(x, y, 301) as f32 / 255.0;
                    r += (grain - 0.5) * 3.0;
                    g += (grain - 0.5) * 3.0;

                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                }
            }
        }

        // ── 11: Ice — light blue with crack line network, bubbles, semi-transparent ──
        11 => {
            // Pre-generate 4 crack line segments forming a network
            let mut cracks = [(0.0f32, 0.0f32, 0.0f32, 0.0f32); 4];
            for i in 0..4u32 {
                cracks[i as usize] = (
                    noise_hash(i, 0, 315) as f32 / 255.0 * tsf,
                    noise_hash(i, 1, 315) as f32 / 255.0 * tsf,
                    noise_hash(i, 2, 315) as f32 / 255.0 * tsf,
                    noise_hash(i, 3, 315) as f32 / 255.0 * tsf,
                );
            }

            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 310);
                    let xf = x as f32;
                    let yf = y as f32;

                    let mut r = (180.0 + (mn - 0.5) * 18.0).clamp(0.0, 255.0);
                    let mut g = (218.0 + (mn - 0.5) * 14.0).clamp(0.0, 255.0);
                    let mut b = (240.0 + (mn - 0.5) * 8.0).clamp(0.0, 255.0);

                    // Crack line network (lighter cracks in ice)
                    for &(x1, y1, x2, y2) in &cracks {
                        let d = dist_to_line(xf, yf, x1, y1, x2, y2);
                        if d < 1.5 {
                            let intensity = 1.0 - d / 1.5;
                            r += 20.0 * intensity;
                            g += 15.0 * intensity;
                            b += 10.0 * intensity;
                        }
                    }

                    // Small bubble inclusions (bright spots)
                    let bubble = noise_hash(x.wrapping_mul(7), y.wrapping_mul(11), 318);
                    if bubble < 5 {
                        r += 18.0;
                        g += 14.0;
                        b += 8.0;
                    }

                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 215, ts);
                }
            }
        }

        // ── 12: Obsidian — very dark with diagonal glossy streaks and purple tint ──
        12 => {
            // Pre-generate 3 diagonal glossy streak lines
            let mut streaks = [(0.0f32, 0.0f32, 0.0f32, 0.0f32); 3];
            for i in 0..3u32 {
                let x1 = noise_hash(i, 0, 325) as f32 / 255.0 * tsf;
                let x2 = noise_hash(i, 1, 325) as f32 / 255.0 * tsf;
                streaks[i as usize] = (x1, 0.0, x2, tsf);
            }

            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 320);
                    let xf = x as f32;
                    let yf = y as f32;

                    let mut r = (24.0 + (mn - 0.5) * 10.0).clamp(0.0, 255.0);
                    let mut g = (20.0 + (mn - 0.5) * 8.0).clamp(0.0, 255.0);
                    let mut b = (28.0 + (mn - 0.5) * 12.0).clamp(0.0, 255.0);

                    // Diagonal glossy streaks
                    for &(x1, y1, x2, y2) in &streaks {
                        let d = dist_to_line(xf, yf, x1, y1, x2, y2);
                        if d < 3.0 {
                            let intensity = 1.0 - d / 3.0;
                            let gloss = intensity * intensity; // sharper falloff
                            r += 12.0 * gloss;
                            g += 8.0 * gloss;
                            b += 16.0 * gloss;
                        }
                    }

                    // Subtle purple tint in patches
                    let purple = noise_hash(x / 4, y / 4, 326) as f32 / 255.0;
                    if purple < 0.3 {
                        r += 5.0;
                        b += 8.0;
                    }

                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                }
            }
        }

        // ── 13: VolcanicRock — dark grey-brown with orange-red vein network, porous dots ──
        13 => {
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 330);

                    // Cell noise for vein network at boundaries
                    let cn = cell_noise(x, y, ts, 330, 8);

                    // Veins at cell boundaries (where cn is high)
                    let vein_threshold = 0.55;
                    let is_vein = cn > vein_threshold;

                    let (mut r, mut g, mut b) = if is_vein {
                        // Orange-red vein with glow
                        let glow = ((cn - vein_threshold) / (1.0 - vein_threshold)).min(1.0);
                        (
                            185.0 + glow * 40.0 + (mn - 0.5) * 12.0,
                            68.0 + glow * 28.0 + (mn - 0.5) * 8.0,
                            15.0 + glow * 12.0 + (mn - 0.5) * 6.0,
                        )
                    } else {
                        // Dark ashy base
                        (
                            72.0 + (mn - 0.5) * 16.0,
                            48.0 + (mn - 0.5) * 12.0,
                            40.0 + (mn - 0.5) * 10.0,
                        )
                    };

                    // Porous dark dots
                    let pore = noise_hash(x.wrapping_mul(5), y.wrapping_mul(3), 335);
                    if pore < 10 && !is_vein {
                        r -= 18.0;
                        g -= 14.0;
                        b -= 12.0;
                    }

                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                }
            }
        }

        // ── 14: Cactus top — central star pattern with darker rim and thorn dots ──
        14 => {
            let cx = tsf / 2.0;
            let cy = tsf / 2.0;
            let rim_start = tsf * 0.42;

            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 340);
                    let dx = x as f32 - cx;
                    let dy = y as f32 - cy;
                    let dist = (dx * dx + dy * dy).sqrt();

                    // Star/cross pattern: brighter along 5 arms
                    let angle = dy.atan2(dx);
                    let star_val = ((angle * 2.5).cos().abs() * 0.5 + 0.5).min(1.0);
                    let arm_dist = (1.0 - star_val) * 6.0; // distance from arm center

                    // Rim detection
                    let on_rim = dist > rim_start;

                    let (mut r, mut g, mut b) = if on_rim {
                        (
                            50.0 + (mn - 0.5) * 10.0,
                            108.0 + (mn - 0.5) * 14.0,
                            38.0 + (mn - 0.5) * 8.0,
                        )
                    } else if arm_dist < 2.5 {
                        // On a star arm — lighter
                        (
                            82.0 + (mn - 0.5) * 12.0,
                            158.0 + (mn - 0.5) * 16.0,
                            66.0 + (mn - 0.5) * 10.0,
                        )
                    } else {
                        // Standard cactus green
                        (
                            72.0 + (mn - 0.5) * 12.0,
                            142.0 + (mn - 0.5) * 18.0,
                            56.0 + (mn - 0.5) * 10.0,
                        )
                    };

                    // Thorn dots near edges (bright specks)
                    let thorn = noise_hash(x.wrapping_mul(7), y.wrapping_mul(11), 345);
                    if thorn < 5 && dist > tsf * 0.2 && dist < rim_start {
                        r = 200.0;
                        g = 210.0;
                        b = 170.0;
                    }

                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                }
            }
        }

        // ── 15: Cactus side — 4 vertical ribs with darker valleys and thorn dots ──
        15 => {
            let num_ribs = 4.0f32;
            let _rib_width = tsf / (num_ribs * 2.0); // ~8px at 64

            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 350);

                    // Vertical ribs using cosine — peaks are rib crests
                    let rib_phase = (x as f32 * num_ribs * 6.2832 / tsf).cos();
                    let on_rib = rib_phase > 0.0; // top half of cosine = rib
                    let rib_intensity = if on_rib { rib_phase } else { 0.0 };

                    let (mut r, mut g, mut b) = if on_rib {
                        // Lighter rib surface
                        let bright = rib_intensity * 14.0;
                        (
                            65.0 + bright + (mn - 0.5) * 14.0,
                            138.0 + bright * 1.4 + (mn - 0.5) * 20.0,
                            52.0 + bright * 0.5 + (mn - 0.5) * 10.0,
                        )
                    } else {
                        // Darker valley between ribs
                        let dark = rib_phase.abs() * 10.0;
                        (
                            50.0 - dark + (mn - 0.5) * 12.0,
                            110.0 - dark * 1.5 + (mn - 0.5) * 16.0,
                            40.0 - dark * 0.5 + (mn - 0.5) * 8.0,
                        )
                    };

                    // Thorn dots on rib crests
                    let thorn = noise_hash(x, y.wrapping_mul(5), 355);
                    let thorn_spacing = ts / 8;
                    if on_rib && rib_phase > 0.7 && thorn < 10 && thorn_spacing > 0 && (y % thorn_spacing) < 2 {
                        r = 195.0;
                        g = 200.0;
                        b = 160.0;
                    }

                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                }
            }
        }

        // ── 16: SandDunes — golden with prominent diagonal wind ripple waves ──
        16 => {
            let period = tsf / 6.4; // ~10px at 64
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 360);

                    // Prominent diagonal wind ripple waves
                    let wave = ((x as f32 * 0.75 + y as f32 * 0.65) * 6.2832 / period).sin() * 0.5 + 0.5;
                    // Secondary subtle wave for complexity
                    let wave2 = ((x as f32 * 0.3 - y as f32 * 0.9) * 6.2832 / (period * 2.5)).sin() * 0.15 + 0.5;

                    let combined = wave * 0.8 + wave2 * 0.2;

                    let mut r = (215.0 + combined * 24.0 + (mn - 0.5) * 14.0).clamp(0.0, 255.0);
                    let mut g = (195.0 + combined * 20.0 + (mn - 0.5) * 12.0).clamp(0.0, 255.0);
                    let mut b = (130.0 + combined * 12.0 + (mn - 0.5) * 10.0).clamp(0.0, 255.0);

                    // Fine grain
                    let grain = noise_hash(x, y, 361) as f32 / 255.0;
                    r += (grain - 0.5) * 8.0;
                    g += (grain - 0.5) * 6.0;
                    b += (grain - 0.5) * 5.0;

                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                }
            }
        }

        // ── 17: CopperOre — stone base with orange-brown copper veins ──
        17 => {
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 370);
                    let cn = cell_noise(x, y, ts, 371, 6);
                    
                    // Copper veins appear at cell boundaries
                    let vein_threshold = 0.52;
                    let is_vein = cn > vein_threshold;
                    
                    let (mut r, mut g, mut b) = if is_vein {
                        // Orange-brown copper with metallic sheen
                        let intensity = ((cn - vein_threshold) / (1.0 - vein_threshold)).min(1.0);
                        (
                            180.0 + intensity * 35.0 + (mn - 0.5) * 15.0,
                            95.0 + intensity * 25.0 + (mn - 0.5) * 10.0,
                            55.0 + intensity * 15.0 + (mn - 0.5) * 8.0,
                        )
                    } else {
                        // Stone base (darker than pure stone)
                        (
                            105.0 + (mn - 0.5) * 18.0,
                            100.0 + (mn - 0.5) * 16.0,
                            98.0 + (mn - 0.5) * 14.0,
                        )
                    };
                    
                    // Oxidation patches (green tint)
                    let oxidize = noise_hash(x / 3, y / 3, 372) as f32 / 255.0;
                    if oxidize < 0.15 && is_vein {
                        g += 25.0;
                        b += 15.0;
                        r -= 20.0;
                    }
                    
                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                }
            }
        }
        
        // ── 18: IronOre — stone base with dark grey-brown iron deposits ──
        18 => {
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 380);
                    let cn = cell_noise(x, y, ts, 381, 5);
                    
                    let vein_threshold = 0.48;
                    let is_vein = cn > vein_threshold;
                    
                    let (mut r, mut g, b) = if is_vein {
                        // Dark iron deposits with reddish-brown tint
                        let intensity = ((cn - vein_threshold) / (1.0 - vein_threshold)).min(1.0);
                        (
                            85.0 + intensity * 20.0 + (mn - 0.5) * 12.0,
                            65.0 + intensity * 15.0 + (mn - 0.5) * 10.0,
                            55.0 + intensity * 10.0 + (mn - 0.5) * 8.0,
                        )
                    } else {
                        // Stone base
                        (
                            108.0 + (mn - 0.5) * 18.0,
                            103.0 + (mn - 0.5) * 16.0,
                            100.0 + (mn - 0.5) * 14.0,
                        )
                    };
                    
                    // Rust spots
                    let rust = noise_hash(x.wrapping_mul(3), y.wrapping_mul(5), 382);
                    if rust < 8 && is_vein {
                        r += 30.0;
                        g -= 10.0;
                    }
                    
                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                }
            }
        }
        
        // ── 19: SilverOre — stone base with bright silver-white veins ──
        19 => {
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 390);
                    let cn = cell_noise(x, y, ts, 391, 4);
                    
                    let vein_threshold = 0.55;
                    let is_vein = cn > vein_threshold;
                    
                    let (mut r, mut g, mut b) = if is_vein {
                        // Bright silver with slight blue tint (magical)
                        let intensity = ((cn - vein_threshold) / (1.0 - vein_threshold)).min(1.0);
                        (
                            185.0 + intensity * 45.0 + (mn - 0.5) * 12.0,
                            190.0 + intensity * 50.0 + (mn - 0.5) * 14.0,
                            205.0 + intensity * 40.0 + (mn - 0.5) * 10.0,
                        )
                    } else {
                        // Darker stone base for contrast
                        (
                            95.0 + (mn - 0.5) * 16.0,
                            92.0 + (mn - 0.5) * 14.0,
                            90.0 + (mn - 0.5) * 12.0,
                        )
                    };
                    
                    // Sparkle highlights
                    let sparkle = noise_hash(x.wrapping_mul(7), y.wrapping_mul(11), 392);
                    if sparkle < 4 && is_vein {
                        r = 255.0;
                        g = 255.0;
                        b = 255.0;
                    }
                    
                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
                }
            }
        }
        
        // ── 20: GoldOre — stone base with rich golden veins ──
        20 => {
            for y in 0..ts {
                for x in 0..ts {
                    let mn = multi_noise(x, y, 400);
                    let cn = cell_noise(x, y, ts, 401, 4);
                    
                    let vein_threshold = 0.58;
                    let is_vein = cn > vein_threshold;
                    
                    let (mut r, mut g, mut b) = if is_vein {
                        // Rich golden yellow
                        let intensity = ((cn - vein_threshold) / (1.0 - vein_threshold)).min(1.0);
                        (
                            220.0 + intensity * 30.0 + (mn - 0.5) * 14.0,
                            175.0 + intensity * 35.0 + (mn - 0.5) * 12.0,
                            45.0 + intensity * 20.0 + (mn - 0.5) * 8.0,
                        )
                    } else {
                        // Stone base
                        (
                            100.0 + (mn - 0.5) * 16.0,
                            96.0 + (mn - 0.5) * 14.0,
                            92.0 + (mn - 0.5) * 12.0,
                        )
                    };
                    
                    // Golden sparkles
                    let sparkle = noise_hash(x.wrapping_mul(5), y.wrapping_mul(7), 402);
                    if sparkle < 3 && is_vein {
                        r = 255.0;
                        g = 230.0;
                        b = 120.0;
                    }
                    
                    set(&mut data, x, y, r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255, ts);
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
            BlockType::CopperOre,
            BlockType::IronOre,
            BlockType::SilverOre,
            BlockType::GoldOre,
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
            BlockType::CopperOre,
            BlockType::IronOre,
            BlockType::SilverOre,
            BlockType::GoldOre,
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
