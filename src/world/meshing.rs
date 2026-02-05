//! Chunk mesh generation - converts voxel data to renderable meshes
//!
//! Supports two strategies:
//! - **Greedy meshing** (`build_chunk_mesh`): Merges adjacent coplanar faces of the same
//!   block type into larger quads, reducing vertex count by 80-90% in typical terrain.
//! - **Naive meshing** (`build_chunk_mesh_naive`): One quad per visible block face.
//!   Retained for benchmarking comparisons.

use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;

use super::{BlockType, Chunk, CHUNK_SIZE};
use super::texture_atlas;

/// Face direction for cube faces
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Top,    // +Y
    Bottom, // -Y
    North,  // +Z
    South,  // -Z
    East,   // +X
    West,   // -X
}

impl Face {
    /// Get the normal vector for this face
    pub fn normal(&self) -> [f32; 3] {
        match self {
            Face::Top => [0.0, 1.0, 0.0],
            Face::Bottom => [0.0, -1.0, 0.0],
            Face::North => [0.0, 0.0, 1.0],
            Face::South => [0.0, 0.0, -1.0],
            Face::East => [1.0, 0.0, 0.0],
            Face::West => [-1.0, 0.0, 0.0],
        }
    }

    /// Get the offset to check for neighboring block
    pub fn offset(&self) -> (i32, i32, i32) {
        match self {
            Face::Top => (0, 1, 0),
            Face::Bottom => (0, -1, 0),
            Face::North => (0, 0, 1),
            Face::South => (0, 0, -1),
            Face::East => (1, 0, 0),
            Face::West => (-1, 0, 0),
        }
    }
}

/// Get the color for a block type
pub fn block_color(block: BlockType) -> [f32; 4] {
    match block {
        BlockType::Air => [0.0, 0.0, 0.0, 0.0],
        BlockType::Stone => [0.5, 0.5, 0.5, 1.0],
        BlockType::Dirt => [0.45, 0.32, 0.22, 1.0],
        BlockType::Grass => [0.35, 0.6, 0.25, 1.0],
        BlockType::Sand => [0.9, 0.85, 0.6, 1.0],
        BlockType::Water => [0.2, 0.4, 0.8, 0.7],
        BlockType::Wood => [0.5, 0.35, 0.2, 1.0],
        BlockType::Leaves => [0.2, 0.5, 0.15, 1.0],
        BlockType::Sandstone => [0.82, 0.73, 0.53, 1.0],
        BlockType::Snow => [0.95, 0.95, 0.97, 1.0],
        BlockType::Ice => [0.7, 0.85, 0.95, 1.0],
        BlockType::Obsidian => [0.1, 0.08, 0.12, 1.0],
        BlockType::VolcanicRock => [0.3, 0.18, 0.15, 1.0],
        BlockType::Cactus => [0.25, 0.55, 0.2, 1.0],
        BlockType::SandDunes => [0.85, 0.78, 0.55, 1.0],
    }
}

// â”€â”€ Biome Vegetation Tinting â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Simple deterministic hash for per-block tint noise.
#[inline]
fn tint_noise(x: i32, z: i32, seed: u32) -> f32 {
    let n = (x as u32)
        .wrapping_mul(73)
        .wrapping_add((z as u32).wrapping_mul(37))
        .wrapping_add(seed);
    let n = n ^ (n >> 13);
    let n = n.wrapping_mul(1274126177);
    ((n >> 24) as f32) / 255.0
}

/// Compute a biome-inspired vegetation tint multiplier for a world position.
///
/// Returns an RGB multiplier that shifts grass/leaf color based on smooth
/// noise fields, simulating temperature variation across the world:
/// - Warm areas: yellow-green tint (higher R, lower B)
/// - Cool areas: deep blue-green tint (lower R, higher B)
/// - Per-block noise adds Â±8% brightness variation
///
/// The smooth component varies over ~48 blocks, producing natural-looking
/// biome-scale color gradients without requiring actual biome data at
/// mesh time.
pub fn biome_grass_tint(world_x: f32, world_z: f32) -> [f32; 3] {
    // Smooth large-scale variation over ~48 blocks
    let scale = 1.0 / 48.0;
    let sx = world_x * scale;
    let sz = world_z * scale;

    // Two overlapping waves for organic, non-axis-aligned variation
    let raw = (sx * 0.7 + sz * 0.3).sin() * 0.5 + 0.5
        + (sx * 0.3 - sz * 0.8).cos() * 0.25;
    let temp = raw.clamp(0.0, 1.0);

    // Temperature â†’ color multiplier
    //   cool (tempâ‰ˆ0): [0.85, 0.95, 1.05]  â€” blue-green (forest/tundra edge)
    //   warm (tempâ‰ˆ1): [1.08, 1.00, 0.82]  â€” yellow-green (plains/desert edge)
    let r = 0.85 + temp * 0.23;
    let g = 0.95 + temp * 0.05;
    let b = 1.05 - temp * 0.23;

    // Per-block noise for subtle variation so adjacent blocks differ
    let noise = tint_noise(world_x.floor() as i32, world_z.floor() as i32, 54321);
    let variation = noise * 0.36 - 0.18; // Â±18%

    [
        (r + variation).max(0.0),
        (g + variation).max(0.0),
        (b + variation).max(0.0),
    ]
}

/// Whether a block type + face combination should receive vegetation tinting.
#[inline]
fn should_tint(block_type: BlockType, face: Face) -> bool {
    match block_type {
        BlockType::Grass => face == Face::Top,
        BlockType::Leaves => true,
        _ => false,
    }
}

/// Atlas configuration passed into face-building helpers.
///
/// When `None`, faces use legacy 0-1 UVs and block_color vertex colors.
/// When `Some`, faces use atlas-mapped UVs and white vertex colors (AO only).
#[derive(Clone, Copy)]
pub struct AtlasConfig {
    pub tiles_per_row: u32,
    pub tile_size: u32,
    pub atlas_size: u32,
}

/// Add vertices for a single 1Ã—1 face of a cube (used by naive meshing).
///
/// `ao` contains per-vertex ambient occlusion levels (0-3) matching the vertex
/// winding order. The diagonal is flipped when AO creates asymmetry to avoid
/// the "dark triangle" artifact.
#[allow(clippy::too_many_arguments)]
fn add_face(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    colors: &mut Vec<[f32; 4]>,
    uvs: &mut Vec<[f32; 2]>,
    uv1s: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    x: f32,
    y: f32,
    z: f32,
    face: Face,
    color: [f32; 4],
    ao: [u8; 4],
    atlas: Option<AtlasConfig>,
    block_type: BlockType,
    world_offset: IVec3,
) {
    let base_index = positions.len() as u32;
    let normal = face.normal();

    // Lowered water surface: top face of water sits at y + 0.875 instead of y + 1.0
    let water_top_offset = if block_type == BlockType::Water && face == Face::Top {
        -0.125
    } else {
        0.0
    };

    let verts: [[f32; 3]; 4] = match face {
        Face::Top => [
            [x, y + 1.0 + water_top_offset, z],
            [x + 1.0, y + 1.0 + water_top_offset, z],
            [x + 1.0, y + 1.0 + water_top_offset, z + 1.0],
            [x, y + 1.0 + water_top_offset, z + 1.0],
        ],
        Face::Bottom => [
            [x, y, z + 1.0],
            [x + 1.0, y, z + 1.0],
            [x + 1.0, y, z],
            [x, y, z],
        ],
        Face::North => [
            [x, y, z + 1.0],
            [x, y + 1.0, z + 1.0],
            [x + 1.0, y + 1.0, z + 1.0],
            [x + 1.0, y, z + 1.0],
        ],
        Face::South => [
            [x + 1.0, y, z],
            [x + 1.0, y + 1.0, z],
            [x, y + 1.0, z],
            [x, y, z],
        ],
        Face::East => [
            [x + 1.0, y, z + 1.0],
            [x + 1.0, y + 1.0, z + 1.0],
            [x + 1.0, y + 1.0, z],
            [x + 1.0, y, z],
        ],
        Face::West => [
            [x, y, z],
            [x, y + 1.0, z],
            [x, y + 1.0, z + 1.0],
            [x, y, z + 1.0],
        ],
    };

    let face_uvs: [[f32; 2]; 4] = if let Some(ac) = atlas {
        let tile = texture_atlas::block_face_texture(block_type, face);
        texture_atlas::face_uvs_atlas(tile, ac.tiles_per_row, ac.tile_size, ac.atlas_size, 1.0, 1.0)
    } else {
        [
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
        ]
    };

    // When atlas is active, vertex color is white so only AO darkens.
    // Water blocks get semi-transparent alpha even in atlas mode.
    // Otherwise vertex color is the block color darkened by AO.
    let vert_color = if atlas.is_some() {
        if block_type == BlockType::Water {
            [1.0_f32, 1.0, 1.0, 0.7]
        } else {
            [1.0_f32, 1.0, 1.0, 1.0]
        }
    } else {
        color
    };

    // Tile grid coordinates for UV1 (used by the atlas shader).
    // When atlas is None, we still push [0,0] to keep vectors aligned;
    // the attribute just won't be added to the mesh.
    let tile_xy = if let Some(ac) = atlas {
        let tile = texture_atlas::block_face_texture(block_type, face);
        super::atlas_material::tile_grid_coords(tile, ac.tiles_per_row)
    } else {
        [0.0, 0.0]
    };

    let do_tint = should_tint(block_type, face);

    for (i, vert) in verts.iter().enumerate() {
        positions.push(*vert);
        normals.push(normal);
        let mut vc = vert_color;
        if do_tint {
            let wx = vert[0] + world_offset.x as f32;
            let wz = vert[2] + world_offset.z as f32;
            let tint = biome_grass_tint(wx, wz);
            vc = [vc[0] * tint[0], vc[1] * tint[1], vc[2] * tint[2], vc[3]];
        }
        colors.push(apply_ao(vc, ao[i]));
        uvs.push(face_uvs[i]);
        uv1s.push(tile_xy);
    }

    // Flip the quad diagonal when AO creates asymmetry.
    let flip = ao[0] as u16 + ao[2] as u16 > ao[1] as u16 + ao[3] as u16;

    if flip {
        indices.extend_from_slice(&[
            base_index,
            base_index + 3,
            base_index + 1,
            base_index + 1,
            base_index + 3,
            base_index + 2,
        ]);
    } else {
        indices.extend_from_slice(&[
            base_index,
            base_index + 2,
            base_index + 1,
            base_index,
            base_index + 3,
            base_index + 2,
        ]);
    }
}

/// Add vertices for a greedy-merged quad face.
///
/// The `quad_w` and `quad_h` parameters represent the extent of the merged quad
/// in the face's two planar axes:
/// - **Top/Bottom** (Y-normal): `quad_w` = extent in X, `quad_h` = extent in Z
/// - **North/South** (Z-normal): `quad_w` = extent in X, `quad_h` = extent in Y
/// - **East/West** (X-normal): `quad_w` = extent in Z, `quad_h` = extent in Y
///
/// `ao` contains per-vertex ambient occlusion levels (0-3). Since the greedy
/// mesher only merges faces with identical AO patterns, the 4 AO values from
/// any constituent cell apply to the whole quad.
#[allow(clippy::too_many_arguments)]
fn add_greedy_face(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    colors: &mut Vec<[f32; 4]>,
    uvs: &mut Vec<[f32; 2]>,
    uv1s: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    x: f32,
    y: f32,
    z: f32,
    quad_w: f32,
    quad_h: f32,
    face: Face,
    color: [f32; 4],
    ao: [u8; 4],
    atlas: Option<AtlasConfig>,
    block_type: BlockType,
    world_offset: IVec3,
) {
    let base_index = positions.len() as u32;
    let normal = face.normal();

    // Lowered water surface: top face of water sits at y + 0.875 instead of y + 1.0
    let water_top_offset = if block_type == BlockType::Water && face == Face::Top {
        -0.125
    } else {
        0.0
    };

    let verts: [[f32; 3]; 4] = match face {
        Face::Top => [
            [x, y + 1.0 + water_top_offset, z],
            [x + quad_w, y + 1.0 + water_top_offset, z],
            [x + quad_w, y + 1.0 + water_top_offset, z + quad_h],
            [x, y + 1.0 + water_top_offset, z + quad_h],
        ],
        Face::Bottom => [
            [x, y, z + quad_h],
            [x + quad_w, y, z + quad_h],
            [x + quad_w, y, z],
            [x, y, z],
        ],
        Face::North => [
            [x, y, z + 1.0],
            [x, y + quad_h, z + 1.0],
            [x + quad_w, y + quad_h, z + 1.0],
            [x + quad_w, y, z + 1.0],
        ],
        Face::South => [
            [x + quad_w, y, z],
            [x + quad_w, y + quad_h, z],
            [x, y + quad_h, z],
            [x, y, z],
        ],
        Face::East => [
            [x + 1.0, y, z + quad_w],
            [x + 1.0, y + quad_h, z + quad_w],
            [x + 1.0, y + quad_h, z],
            [x + 1.0, y, z],
        ],
        Face::West => [
            [x, y, z],
            [x, y + quad_h, z],
            [x, y + quad_h, z + quad_w],
            [x, y, z + quad_w],
        ],
    };

    let face_uvs: [[f32; 2]; 4] = if let Some(ac) = atlas {
        let tile = texture_atlas::block_face_texture(block_type, face);
        texture_atlas::face_uvs_atlas(tile, ac.tiles_per_row, ac.tile_size, ac.atlas_size, quad_w, quad_h)
    } else {
        [
            [0.0, 0.0],
            [quad_w, 0.0],
            [quad_w, quad_h],
            [0.0, quad_h],
        ]
    };

    // When atlas is active, vertex color is white so only AO darkens.
    // Water blocks get semi-transparent alpha even in atlas mode.
    let vert_color = if atlas.is_some() {
        if block_type == BlockType::Water {
            [1.0_f32, 1.0, 1.0, 0.7]
        } else {
            [1.0_f32, 1.0, 1.0, 1.0]
        }
    } else {
        color
    };

    // Tile grid coordinates for UV1 (used by the atlas shader).
    let tile_xy = if let Some(ac) = atlas {
        let tile = texture_atlas::block_face_texture(block_type, face);
        super::atlas_material::tile_grid_coords(tile, ac.tiles_per_row)
    } else {
        [0.0, 0.0]
    };

    let do_tint = should_tint(block_type, face);

    for (i, vert) in verts.iter().enumerate() {
        positions.push(*vert);
        normals.push(normal);
        let mut vc = vert_color;
        if do_tint {
            let wx = vert[0] + world_offset.x as f32;
            let wz = vert[2] + world_offset.z as f32;
            let tint = biome_grass_tint(wx, wz);
            vc = [vc[0] * tint[0], vc[1] * tint[1], vc[2] * tint[2], vc[3]];
        }
        colors.push(apply_ao(vc, ao[i]));
        uvs.push(face_uvs[i]);
        uv1s.push(tile_xy);
    }

    // Flip diagonal when AO is asymmetric (same logic as add_face)
    let flip = ao[0] as u16 + ao[2] as u16 > ao[1] as u16 + ao[3] as u16;

    if flip {
        indices.extend_from_slice(&[
            base_index,
            base_index + 3,
            base_index + 1,
            base_index + 1,
            base_index + 3,
            base_index + 2,
        ]);
    } else {
        indices.extend_from_slice(&[
            base_index,
            base_index + 2,
            base_index + 1,
            base_index,
            base_index + 3,
            base_index + 2,
        ]);
    }
}

/// Determine whether a face of `block` should be rendered given its `neighbor`.
///
/// - Air never renders faces.
/// - Water hides faces adjacent to other water (internal culling), but shows
///   faces adjacent to air or any other transparent block.
/// - Solid blocks show faces when the neighbor is transparent (air or water).
fn should_render_face(block: BlockType, neighbor: BlockType) -> bool {
    if block == BlockType::Air {
        return false;
    }
    if block == BlockType::Water {
        // Water-to-water faces are hidden; water-to-anything-else is shown.
        return neighbor != BlockType::Water;
    }
    // Solid blocks: show face when neighbor is transparent
    neighbor.is_transparent()
}

/// Check if a neighboring block at the given offset is transparent
fn is_neighbor_transparent(chunk: &Chunk, x: i32, y: i32, z: i32) -> bool {
    // If outside chunk bounds, treat as transparent (air)
    if x < 0
        || x >= CHUNK_SIZE as i32
        || y < 0
        || y >= CHUNK_SIZE as i32
        || z < 0
        || z >= CHUNK_SIZE as i32
    {
        return true;
    }
    chunk
        .get_block(x as usize, y as usize, z as usize)
        .is_transparent()
}

// â”€â”€ Ambient Occlusion â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// AO darkening multipliers for levels 0-3.
/// Level 0 = no occlusion (full brightness), level 3 = maximum occlusion.
const AO_CURVE: [f32; 4] = [1.0, 0.75, 0.55, 0.35];

/// Compute the ambient occlusion level (0-3) for a single vertex.
///
/// `side1` and `side2` are whether the two edge-adjacent blocks are opaque,
/// and `corner` is whether the diagonal corner block is opaque.
///
/// Standard voxel AO formula:
/// - If both sides are opaque, AO = 3 (the corner is irrelevant).
/// - Otherwise AO = side1 + side2 + corner.
pub fn vertex_ao(side1: bool, side2: bool, corner: bool) -> u8 {
    if side1 && side2 {
        3
    } else {
        side1 as u8 + side2 as u8 + corner as u8
    }
}

/// Check if a block position is opaque (not transparent). Out-of-chunk = Air = not opaque.
fn is_opaque(chunk: &Chunk, x: i32, y: i32, z: i32) -> bool {
    !is_neighbor_transparent(chunk, x, y, z)
}

/// Apply AO darkening to an RGBA color.
fn apply_ao(color: [f32; 4], ao_level: u8) -> [f32; 4] {
    let m = AO_CURVE[ao_level.min(3) as usize];
    [color[0] * m, color[1] * m, color[2] * m, color[3]]
}

/// Compute the AO values for all 4 vertices of a face.
///
/// Returns `[ao0, ao1, ao2, ao3]` matching the vertex winding order used in
/// `add_face` / `add_greedy_face`.
///
/// For each face direction, we define two tangent axes (t1, t2) in the plane of
/// the face. Each vertex sits at one of the four corners of the face. For a corner
/// identified by signs (s1, s2) along the tangent axes, the three neighbor offsets
/// checked are:
///   - side1: normal + s1*t1
///   - side2: normal + s2*t2
///   - corner: normal + s1*t1 + s2*t2
pub fn compute_face_ao(chunk: &Chunk, x: usize, y: usize, z: usize, face: Face) -> [u8; 4] {
    let (bx, by, bz) = (x as i32, y as i32, z as i32);
    let (nx, ny, nz) = face.offset();

    // Neighbor position (the air block this face looks into)
    let (fx, fy, fz) = (bx + nx, by + ny, bz + nz);

    // Tangent axes spanning the face plane.
    let (t1, t2): ((i32, i32, i32), (i32, i32, i32)) = match face {
        Face::Top =>    ((1, 0, 0), (0, 0, 1)),
        Face::Bottom => ((1, 0, 0), (0, 0, 1)),
        Face::North =>  ((1, 0, 0), (0, 1, 0)),
        Face::South =>  ((1, 0, 0), (0, 1, 0)),
        Face::East =>   ((0, 0, 1), (0, 1, 0)),
        Face::West =>   ((0, 0, 1), (0, 1, 0)),
    };

    // (s1, s2) pairs matching the vertex winding order in add_face for each face.
    let corners: [(i32, i32); 4] = match face {
        Face::Top =>    [(-1, -1), ( 1, -1), ( 1,  1), (-1,  1)],
        Face::Bottom => [(-1,  1), ( 1,  1), ( 1, -1), (-1, -1)],
        Face::North =>  [(-1, -1), (-1,  1), ( 1,  1), ( 1, -1)],
        Face::South =>  [( 1, -1), ( 1,  1), (-1,  1), (-1, -1)],
        Face::East =>   [( 1, -1), ( 1,  1), (-1,  1), (-1, -1)],
        Face::West =>   [(-1, -1), (-1,  1), ( 1,  1), ( 1, -1)],
    };

    let mut ao = [0u8; 4];
    for (i, &(s1, s2)) in corners.iter().enumerate() {
        let side1 = is_opaque(chunk, fx + s1 * t1.0, fy + s1 * t1.1, fz + s1 * t1.2);
        let side2 = is_opaque(chunk, fx + s2 * t2.0, fy + s2 * t2.1, fz + s2 * t2.2);
        let corner = is_opaque(
            chunk,
            fx + s1 * t1.0 + s2 * t2.0,
            fy + s1 * t1.1 + s2 * t2.1,
            fz + s1 * t1.2 + s2 * t2.2,
        );
        ao[i] = vertex_ao(side1, side2, corner);
    }
    ao
}


// â”€â”€ Cross-Chunk Neighbor Data â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Block data from neighboring chunks for cross-chunk face culling.
///
/// Each neighbor is a flat array of CHUNK_SIZEÂ³ `BlockType` values, or `None`
/// if the neighbor chunk doesn't exist (treated as air/transparent).
///
/// The flat array uses Chunk-native indexing: `x + y * CHUNK_SIZE + z * CHUNK_SIZE * CHUNK_SIZE`.
pub struct ChunkNeighbors {
    pub pos_x: Option<Vec<BlockType>>,  // +X neighbor
    pub neg_x: Option<Vec<BlockType>>,  // -X neighbor
    pub pos_y: Option<Vec<BlockType>>,  // +Y neighbor
    pub neg_y: Option<Vec<BlockType>>,  // -Y neighbor
    pub pos_z: Option<Vec<BlockType>>,  // +Z neighbor
    pub neg_z: Option<Vec<BlockType>>,  // -Z neighbor
}

impl ChunkNeighbors {
    /// Create an empty set of neighbors (all `None` â€” treats boundaries as air).
    #[allow(dead_code)]
    pub fn empty() -> Self {
        Self {
            pos_x: None, neg_x: None,
            pos_y: None, neg_y: None,
            pos_z: None, neg_z: None,
        }
    }
}

/// Look up a block from a flat chunk-data array using Chunk-native indexing.
#[inline]
fn get_block_from_data(data: &[BlockType], x: usize, y: usize, z: usize) -> BlockType {
    data[x + y * CHUNK_SIZE + z * CHUNK_SIZE * CHUNK_SIZE]
}

/// Get the block type at the given coordinate, looking into neighbor data
/// when the coordinate falls outside 0..CHUNK_SIZE.
fn get_block_with_neighbors(
    chunk: &Chunk,
    neighbors: &ChunkNeighbors,
    x: i32, y: i32, z: i32,
) -> BlockType {
    let cs = CHUNK_SIZE as i32;
    if x >= 0 && x < cs && y >= 0 && y < cs && z >= 0 && z < cs {
        return chunk.get_block(x as usize, y as usize, z as usize);
    }
    let x_out = x < 0 || x >= cs;
    let y_out = y < 0 || y >= cs;
    let z_out = z < 0 || z >= cs;
    // More than one axis out of range = diagonal neighbor (not stored)
    if (x_out as u8 + y_out as u8 + z_out as u8) > 1 {
        return BlockType::Air;
    }
    if x < 0 {
        return neighbors.neg_x.as_ref()
            .map(|d| get_block_from_data(d, CHUNK_SIZE - 1, y as usize, z as usize))
            .unwrap_or(BlockType::Air);
    }
    if x >= cs {
        return neighbors.pos_x.as_ref()
            .map(|d| get_block_from_data(d, 0, y as usize, z as usize))
            .unwrap_or(BlockType::Air);
    }
    if y < 0 {
        return neighbors.neg_y.as_ref()
            .map(|d| get_block_from_data(d, x as usize, CHUNK_SIZE - 1, z as usize))
            .unwrap_or(BlockType::Air);
    }
    if y >= cs {
        return neighbors.pos_y.as_ref()
            .map(|d| get_block_from_data(d, x as usize, 0, z as usize))
            .unwrap_or(BlockType::Air);
    }
    if z < 0 {
        return neighbors.neg_z.as_ref()
            .map(|d| get_block_from_data(d, x as usize, y as usize, CHUNK_SIZE - 1))
            .unwrap_or(BlockType::Air);
    }
    if z >= cs {
        return neighbors.pos_z.as_ref()
            .map(|d| get_block_from_data(d, x as usize, y as usize, 0))
            .unwrap_or(BlockType::Air);
    }
    BlockType::Air
}

/// Check if opaque, with cross-chunk neighbor lookups.
fn is_opaque_with_neighbors(chunk: &Chunk, neighbors: &ChunkNeighbors, x: i32, y: i32, z: i32) -> bool {
    !get_block_with_neighbors(chunk, neighbors, x, y, z).is_transparent()
}

/// Compute AO for a face with cross-chunk neighbor lookups.
pub fn compute_face_ao_with_neighbors(
    chunk: &Chunk, neighbors: &ChunkNeighbors,
    x: usize, y: usize, z: usize, face: Face,
) -> [u8; 4] {
    let (bx, by, bz) = (x as i32, y as i32, z as i32);
    let (nx, ny, nz) = face.offset();
    let (fx, fy, fz) = (bx + nx, by + ny, bz + nz);
    let (t1, t2): ((i32, i32, i32), (i32, i32, i32)) = match face {
        Face::Top =>    ((1, 0, 0), (0, 0, 1)),
        Face::Bottom => ((1, 0, 0), (0, 0, 1)),
        Face::North =>  ((1, 0, 0), (0, 1, 0)),
        Face::South =>  ((1, 0, 0), (0, 1, 0)),
        Face::East =>   ((0, 0, 1), (0, 1, 0)),
        Face::West =>   ((0, 0, 1), (0, 1, 0)),
    };
    let corners: [(i32, i32); 4] = match face {
        Face::Top =>    [(-1, -1), ( 1, -1), ( 1,  1), (-1,  1)],
        Face::Bottom => [(-1,  1), ( 1,  1), ( 1, -1), (-1, -1)],
        Face::North =>  [(-1, -1), (-1,  1), ( 1,  1), ( 1, -1)],
        Face::South =>  [( 1, -1), ( 1,  1), (-1,  1), (-1, -1)],
        Face::East =>   [( 1, -1), ( 1,  1), (-1,  1), (-1, -1)],
        Face::West =>   [(-1, -1), (-1,  1), ( 1,  1), ( 1, -1)],
    };
    let mut ao = [0u8; 4];
    for (i, &(s1, s2)) in corners.iter().enumerate() {
        let side1 = is_opaque_with_neighbors(chunk, neighbors, fx + s1 * t1.0, fy + s1 * t1.1, fz + s1 * t1.2);
        let side2 = is_opaque_with_neighbors(chunk, neighbors, fx + s2 * t2.0, fy + s2 * t2.1, fz + s2 * t2.2);
        let corner = is_opaque_with_neighbors(chunk, neighbors,
            fx + s1 * t1.0 + s2 * t2.0, fy + s1 * t1.1 + s2 * t2.1, fz + s1 * t1.2 + s2 * t2.2);
        ao[i] = vertex_ao(side1, side2, corner);
    }
    ao
}

/// Build a chunk mesh with cross-chunk neighbor data and atlas configuration.
///
/// This is the primary meshing entry point used by the runtime pipeline.
/// It eliminates visible seams at chunk boundaries by looking into adjacent
/// chunk data when determining face visibility and AO.
///
/// Returns `(opaque_mesh, Option<water_mesh>)`. The opaque mesh contains all
/// solid block faces and uses `AlphaMode::Opaque` for correct depth sorting.
/// The optional water mesh contains only water block faces and uses
/// `AlphaMode::Blend` for transparency. This split prevents the see-through
/// terrain artifacts caused by rendering everything in the transparent pass.
pub fn build_chunk_mesh_with_neighbors(
    chunk: &Chunk, atlas: Option<AtlasConfig>, neighbors: &ChunkNeighbors,
) -> (Mesh, Option<Mesh>) {
    build_chunk_mesh_neighbors_inner(chunk, atlas, neighbors)
}

/// Inner greedy meshing with cross-chunk neighbor awareness.
///
/// Produces two meshes: opaque (solid blocks) and water (transparent).
/// Water faces are directed to separate buffers so they can be rendered
/// with `AlphaMode::Blend` on a child entity, while the main chunk entity
/// uses `AlphaMode::Opaque` for reliable depth writes.
#[allow(clippy::needless_range_loop)]
fn build_chunk_mesh_neighbors_inner(
    chunk: &Chunk, atlas: Option<AtlasConfig>, neighbors: &ChunkNeighbors,
) -> (Mesh, Option<Mesh>) {
    let world_offset = chunk.world_position();

    // Opaque mesh buffers (solid blocks)
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut uv1s: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    // Water mesh buffers (transparent blocks)
    let mut w_positions: Vec<[f32; 3]> = Vec::new();
    let mut w_normals: Vec<[f32; 3]> = Vec::new();
    let mut w_colors: Vec<[f32; 4]> = Vec::new();
    let mut w_uvs: Vec<[f32; 2]> = Vec::new();
    let mut w_uv1s: Vec<[f32; 2]> = Vec::new();
    let mut w_indices: Vec<u32> = Vec::new();

    let faces = [Face::Top, Face::Bottom, Face::North, Face::South, Face::East, Face::West];
    for face in faces {
        for slice in 0..CHUNK_SIZE {
            let mut mask: [[Option<(BlockType, [u8; 4])>; CHUNK_SIZE]; CHUNK_SIZE] =
                [[None; CHUNK_SIZE]; CHUNK_SIZE];
            for v in 0..CHUNK_SIZE {
                for u in 0..CHUNK_SIZE {
                    let (x, y, z) = match face {
                        Face::Top | Face::Bottom => (u, slice, v),
                        Face::North | Face::South => (u, v, slice),
                        Face::East | Face::West => (slice, v, u),
                    };
                    let block = chunk.get_block(x, y, z);
                    if block == BlockType::Air { continue; }
                    let (ox, oy, oz) = face.offset();
                    let (nx, ny, nz) = (x as i32 + ox, y as i32 + oy, z as i32 + oz);
                    let neighbor = get_block_with_neighbors(chunk, neighbors, nx, ny, nz);
                    if should_render_face(block, neighbor) {
                        let ao = compute_face_ao_with_neighbors(chunk, neighbors, x, y, z, face);
                        mask[v][u] = Some((block, ao));
                    }
                }
            }
            let mut visited = [[false; CHUNK_SIZE]; CHUNK_SIZE];
            for v in 0..CHUNK_SIZE {
                for u in 0..CHUNK_SIZE {
                    if visited[v][u] || mask[v][u].is_none() { continue; }
                    let (block_type, ao) = mask[v][u].unwrap();
                    let key = (block_type, ao);
                    let mut w = 1usize;
                    while u + w < CHUNK_SIZE && !visited[v][u + w] && mask[v][u + w] == Some(key) { w += 1; }
                    let mut h = 1usize;
                    'expand_v: while v + h < CHUNK_SIZE {
                        for du in 0..w {
                            if visited[v + h][u + du] || mask[v + h][u + du] != Some(key) { break 'expand_v; }
                        }
                        h += 1;
                    }
                    for dv in 0..h { for du in 0..w { visited[v + dv][u + du] = true; } }
                    let (x, y, z) = match face {
                        Face::Top | Face::Bottom => (u, slice, v),
                        Face::North | Face::South => (u, v, slice),
                        Face::East | Face::West => (slice, v, u),
                    };
                    let color = block_color(block_type);

                    // Route water faces to the water mesh buffers
                    if block_type == BlockType::Water {
                        add_greedy_face(
                            &mut w_positions, &mut w_normals, &mut w_colors,
                            &mut w_uvs, &mut w_uv1s, &mut w_indices,
                            x as f32, y as f32, z as f32, w as f32, h as f32,
                            face, color, ao, atlas, block_type, world_offset,
                        );
                    } else {
                        add_greedy_face(
                            &mut positions, &mut normals, &mut colors,
                            &mut uvs, &mut uv1s, &mut indices,
                            x as f32, y as f32, z as f32, w as f32, h as f32,
                            face, color, ao, atlas, block_type, world_offset,
                        );
                    }
                }
            }
        }
    }

    // Build opaque mesh
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    if atlas.is_some() { mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv1s); }
    mesh.insert_indices(Indices::U32(indices));

    // Build water mesh (only if there are water faces)
    let water_mesh = if !w_positions.is_empty() {
        let mut wm = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        wm.insert_attribute(Mesh::ATTRIBUTE_POSITION, w_positions);
        wm.insert_attribute(Mesh::ATTRIBUTE_NORMAL, w_normals);
        wm.insert_attribute(Mesh::ATTRIBUTE_COLOR, w_colors);
        wm.insert_attribute(Mesh::ATTRIBUTE_UV_0, w_uvs);
        if atlas.is_some() { wm.insert_attribute(Mesh::ATTRIBUTE_UV_1, w_uv1s); }
        wm.insert_indices(Indices::U32(w_indices));
        Some(wm)
    } else {
        None
    };

    (mesh, water_mesh)
}

/// Build a mesh for a chunk using **greedy meshing**.
///
/// For each of the 6 face directions, iterates over 2-D slices perpendicular to the
/// face normal. Within each slice a boolean+BlockType mask of visible faces is built,
/// then rectangles of identical block type are greedily merged before being emitted as
/// single quads. This typically reduces vertex count by 80-90 % compared to naive
/// per-block-face generation.
///
/// Uses legacy vertex-color mode (no texture atlas). For atlas-mapped meshing,
/// use `build_chunk_mesh_with_atlas`.
#[allow(clippy::needless_range_loop)]
pub fn build_chunk_mesh(chunk: &Chunk) -> Mesh {
    build_chunk_mesh_inner(chunk, None)
}

/// Build a chunk mesh with explicit atlas configuration.
///
/// Pass `None` for `atlas` to get legacy vertex-color mode.
#[allow(clippy::needless_range_loop)]
pub fn build_chunk_mesh_with_atlas(chunk: &Chunk, atlas: Option<AtlasConfig>) -> Mesh {
    build_chunk_mesh_inner(chunk, atlas)
}

#[allow(clippy::needless_range_loop)]
fn build_chunk_mesh_inner(chunk: &Chunk, atlas: Option<AtlasConfig>) -> Mesh {
    let world_offset = chunk.world_position();

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut uv1s: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let faces = [
        Face::Top,
        Face::Bottom,
        Face::North,
        Face::South,
        Face::East,
        Face::West,
    ];

    for face in faces {
        for slice in 0..CHUNK_SIZE {
            // Build a 2-D mask of visible faces for this slice.
            // mask[v][u] = Some((block_type, ao_values)) if the face is visible.
            // AO is included in the merge key so faces with different AO can't merge.
            let mut mask: [[Option<(BlockType, [u8; 4])>; CHUNK_SIZE]; CHUNK_SIZE] =
                [[None; CHUNK_SIZE]; CHUNK_SIZE];

            for v in 0..CHUNK_SIZE {
                for u in 0..CHUNK_SIZE {
                    // Map (slice, u, v) â†’ (x, y, z) depending on face direction.
                    //   Top/Bottom (Y-normal): u=X, v=Z, slice=Y
                    //   North/South (Z-normal): u=X, v=Y, slice=Z
                    //   East/West (X-normal): u=Z, v=Y, slice=X
                    let (x, y, z) = match face {
                        Face::Top | Face::Bottom => (u, slice, v),
                        Face::North | Face::South => (u, v, slice),
                        Face::East | Face::West => (slice, v, u),
                    };

                    let block = chunk.get_block(x, y, z);
                    if block == BlockType::Air {
                        continue;
                    }

                    let (ox, oy, oz) = face.offset();
                    let nx = x as i32 + ox;
                    let ny = y as i32 + oy;
                    let nz = z as i32 + oz;

                    // Determine neighbor block type (out-of-bounds = Air)
                    let neighbor = if nx < 0
                        || nx >= CHUNK_SIZE as i32
                        || ny < 0
                        || ny >= CHUNK_SIZE as i32
                        || nz < 0
                        || nz >= CHUNK_SIZE as i32
                    {
                        BlockType::Air
                    } else {
                        chunk.get_block(nx as usize, ny as usize, nz as usize)
                    };

                    if should_render_face(block, neighbor) {
                        let ao = compute_face_ao(chunk, x, y, z, face);
                        mask[v][u] = Some((block, ao));
                    }
                }
            }

            // Greedy rectangle merging â€” matches on (BlockType, AO pattern)
            let mut visited = [[false; CHUNK_SIZE]; CHUNK_SIZE];

            for v in 0..CHUNK_SIZE {
                for u in 0..CHUNK_SIZE {
                    if visited[v][u] || mask[v][u].is_none() {
                        continue;
                    }

                    let (block_type, ao) = mask[v][u].unwrap();
                    let key = (block_type, ao);

                    // Expand width in the u-direction
                    let mut w = 1usize;
                    while u + w < CHUNK_SIZE
                        && !visited[v][u + w]
                        && mask[v][u + w] == Some(key)
                    {
                        w += 1;
                    }

                    // Expand height in the v-direction
                    let mut h = 1usize;
                    'expand_v: while v + h < CHUNK_SIZE {
                        for du in 0..w {
                            if visited[v + h][u + du]
                                || mask[v + h][u + du] != Some(key)
                            {
                                break 'expand_v;
                            }
                        }
                        h += 1;
                    }

                    // Mark merged cells as visited
                    for dv in 0..h {
                        for du in 0..w {
                            visited[v + dv][u + du] = true;
                        }
                    }

                    // Map starting (u, v) back to world (x, y, z)
                    let (x, y, z) = match face {
                        Face::Top | Face::Bottom => (u, slice, v),
                        Face::North | Face::South => (u, v, slice),
                        Face::East | Face::West => (slice, v, u),
                    };

                    let color = block_color(block_type);

                    add_greedy_face(
                        &mut positions,
                        &mut normals,
                        &mut colors,
                        &mut uvs,
                        &mut uv1s,
                        &mut indices,
                        x as f32,
                        y as f32,
                        z as f32,
                        w as f32,
                        h as f32,
                        face,
                        color,
                        ao,
                        atlas,
                        block_type,
                        world_offset,
                    );
                }
            }
        }
    }

    // Create the mesh
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());

    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);

    // UV1 carries tile grid coordinates for the atlas shader.
    // Only added when atlas mode is active â€” the shader checks
    // `#ifdef VERTEX_UVS_B` which Bevy enables when UV1 is present.
    if atlas.is_some() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv1s);
    }

    mesh.insert_indices(Indices::U32(indices));

    mesh
}

/// Build a mesh for a chunk using **naive** face culling (one quad per visible block face).
///
/// Retained for benchmarking comparisons against greedy meshing.
#[allow(dead_code)]
pub fn build_chunk_mesh_naive(chunk: &Chunk) -> Mesh {
    build_chunk_mesh_naive_inner(chunk, None)
}

/// Naive meshing with explicit atlas configuration.
#[allow(dead_code)]
pub fn build_chunk_mesh_naive_with_atlas(chunk: &Chunk, atlas: Option<AtlasConfig>) -> Mesh {
    build_chunk_mesh_naive_inner(chunk, atlas)
}

#[allow(dead_code)]
fn build_chunk_mesh_naive_inner(chunk: &Chunk, atlas: Option<AtlasConfig>) -> Mesh {
    let world_offset = chunk.world_position();

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut uv1s: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let faces = [
        Face::Top,
        Face::Bottom,
        Face::North,
        Face::South,
        Face::East,
        Face::West,
    ];

    for x in 0..CHUNK_SIZE {
        for y in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                let block = chunk.get_block(x, y, z);

                // Skip air blocks
                if block == BlockType::Air {
                    continue;
                }

                let color = block_color(block);
                let fx = x as f32;
                let fy = y as f32;
                let fz = z as f32;

                // Check each face
                for face in faces {
                    let (ox, oy, oz) = face.offset();
                    let nx = x as i32 + ox;
                    let ny = y as i32 + oy;
                    let nz = z as i32 + oz;

                    // Determine neighbor block type (out-of-bounds = Air)
                    let neighbor = if nx < 0
                        || nx >= CHUNK_SIZE as i32
                        || ny < 0
                        || ny >= CHUNK_SIZE as i32
                        || nz < 0
                        || nz >= CHUNK_SIZE as i32
                    {
                        BlockType::Air
                    } else {
                        chunk.get_block(nx as usize, ny as usize, nz as usize)
                    };

                    // Only add face if should_render_face says so
                    if should_render_face(block, neighbor) {
                        let ao = compute_face_ao(chunk, x, y, z, face);
                        add_face(
                            &mut positions,
                            &mut normals,
                            &mut colors,
                            &mut uvs,
                            &mut uv1s,
                            &mut indices,
                            fx,
                            fy,
                            fz,
                            face,
                            color,
                            ao,
                            atlas,
                            block,
                            world_offset,
                        );
                    }
                }
            }
        }
    }

    // Create the mesh
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());

    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);

    if atlas.is_some() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv1s);
    }

    mesh.insert_indices(Indices::U32(indices));

    mesh
}

/// Helper: extract the position attribute length from a mesh (= vertex count).
#[cfg(test)]
fn mesh_vertex_count(mesh: &Mesh) -> usize {
    mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        .map(|attr| attr.len())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::IVec3;

    // ==================================================================
    //  Ambient Occlusion unit tests
    // ==================================================================

    #[test]
    fn test_vertex_ao_no_occlusion() {
        assert_eq!(vertex_ao(false, false, false), 0);
    }

    #[test]
    fn test_vertex_ao_one_side() {
        assert_eq!(vertex_ao(true, false, false), 1);
        assert_eq!(vertex_ao(false, true, false), 1);
    }

    #[test]
    fn test_vertex_ao_corner_only() {
        assert_eq!(vertex_ao(false, false, true), 1);
    }

    #[test]
    fn test_vertex_ao_side_plus_corner() {
        assert_eq!(vertex_ao(true, false, true), 2);
        assert_eq!(vertex_ao(false, true, true), 2);
    }

    #[test]
    fn test_vertex_ao_two_sides() {
        // Both sides occlude â†’ max AO regardless of corner
        assert_eq!(vertex_ao(true, true, false), 3);
        assert_eq!(vertex_ao(true, true, true), 3);
    }

    #[test]
    fn test_apply_ao_multipliers() {
        // AO level 0: no darkening
        let color = [1.0, 0.8, 0.6, 1.0];
        let ao0 = apply_ao(color, 0);
        assert!((ao0[0] - 1.0).abs() < 1e-6);
        assert!((ao0[1] - 0.8).abs() < 1e-6);
        assert!((ao0[2] - 0.6).abs() < 1e-6);
        assert_eq!(ao0[3], 1.0);

        // AO level 1: 0.75x darkening
        let ao1 = apply_ao(color, 1);
        assert!((ao1[0] - 0.75).abs() < 1e-6);
        assert!((ao1[1] - 0.6).abs() < 1e-5);
        assert!((ao1[2] - 0.45).abs() < 1e-5);
        assert_eq!(ao1[3], 1.0);

        // AO level 2: 0.55x darkening
        let ao2 = apply_ao(color, 2);
        assert!((ao2[0] - 0.55).abs() < 1e-6);
        assert!((ao2[1] - 0.44).abs() < 1e-5);
        assert!((ao2[2] - 0.33).abs() < 1e-5);

        // AO level 3: 0.35x darkening
        let ao3 = apply_ao(color, 3);
        assert!((ao3[0] - 0.35).abs() < 1e-5);
        assert!((ao3[1] - 0.28).abs() < 1e-5);
        assert!((ao3[2] - 0.21).abs() < 1e-5);

        // Alpha is never affected
        let transparent = [0.5, 0.5, 0.5, 0.5];
        assert_eq!(apply_ao(transparent, 3)[3], 0.5);
    }

    #[test]
    fn test_ao_isolated_block_no_occlusion() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(8, 8, 8, BlockType::Stone);

        for face in [Face::Top, Face::Bottom, Face::North, Face::South, Face::East, Face::West] {
            let ao = compute_face_ao(&chunk, 8, 8, 8, face);
            assert_eq!(ao, [0, 0, 0, 0], "isolated block face {:?} should have zero AO", face);
        }
    }

    #[test]
    fn test_ao_fully_surrounded_top_face() {
        // Fill a 3Ã—3Ã—3 region, check AO on center block's top face
        let mut chunk = Chunk::new(IVec3::ZERO);
        for x in 7..=9 {
            for y in 7..=9 {
                for z in 7..=9 {
                    chunk.set_block(x, y, z, BlockType::Stone);
                }
            }
        }
        // Remove block above center to expose the top face
        chunk.set_block(8, 9, 8, BlockType::Air);

        let ao = compute_face_ao(&chunk, 8, 8, 8, Face::Top);
        // All 4 vertices should see surrounding blocks and have AO > 0
        for (i, &val) in ao.iter().enumerate() {
            assert!(val > 0, "vertex {} of surrounded top face should have AO > 0, got {}", i, val);
        }
    }

    #[test]
    fn test_ao_darkening_applied_to_mesh() {
        // Isolated block â†’ AO=0 everywhere â†’ colors should be unmodified
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(8, 8, 8, BlockType::Stone);

        let mesh = build_chunk_mesh_naive(&chunk);
        let base_color = block_color(BlockType::Stone);

        if let Some(bevy::render::mesh::VertexAttributeValues::Float32x4(mesh_colors)) =
            mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        {
            for c in mesh_colors {
                assert_eq!(*c, base_color, "isolated block should have full brightness");
            }
        } else {
            panic!("COLOR attribute missing");
        }
    }

    #[test]
    fn test_ao_darkening_reduces_brightness() {
        // 3Ã—3Ã—3 cube with center-top removed creates AO on cavity walls
        let mut chunk = Chunk::new(IVec3::ZERO);
        for x in 0..3 {
            for z in 0..3 {
                for y in 0..3 {
                    chunk.set_block(x, y, z, BlockType::Stone);
                }
            }
        }
        chunk.set_block(1, 2, 1, BlockType::Air);

        let mesh = build_chunk_mesh_naive(&chunk);
        let base_color = block_color(BlockType::Stone);

        if let Some(bevy::render::mesh::VertexAttributeValues::Float32x4(mesh_colors)) =
            mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        {
            let has_darkened = mesh_colors.iter().any(|c| c[0] < base_color[0] - 0.001);
            assert!(has_darkened, "AO should darken some vertices in a cavity");
        } else {
            panic!("COLOR attribute missing");
        }
    }

    // ==================================================================
    //  Greedy vs naive vertex count tests (updated for AO)
    // ==================================================================

    #[test]
    fn test_greedy_fewer_vertices_than_naive_filled_chunk() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.fill(BlockType::Stone);

        let greedy = build_chunk_mesh(&chunk);
        let naive = build_chunk_mesh_naive(&chunk);

        let greedy_verts = mesh_vertex_count(&greedy);
        let naive_verts = mesh_vertex_count(&naive);

        // Naive: 6 * 256 * 4 = 6144 vertices (unchanged by AO)
        assert_eq!(naive_verts, 6144);
        // Greedy with AO: edge/corner blocks have different AO patterns than interior,
        // preventing full merge into one quad per face. But still much less than naive.
        assert!(
            greedy_verts < naive_verts / 2,
            "greedy ({greedy_verts}) should be much less than naive ({naive_verts})"
        );
    }

    #[test]
    fn test_single_block_produces_six_faces() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(7, 7, 7, BlockType::Stone);

        let greedy = build_chunk_mesh(&chunk);
        let naive = build_chunk_mesh_naive(&chunk);

        // Single block in air â†’ AO=0 on all faces â†’ all merge normally
        assert_eq!(mesh_vertex_count(&greedy), 24);
        assert_eq!(mesh_vertex_count(&naive), 24);
    }

    #[test]
    fn test_different_block_types_not_merged() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(0, 0, 0, BlockType::Stone);
        chunk.set_block(1, 0, 0, BlockType::Dirt);

        let greedy = build_chunk_mesh(&chunk);
        let naive = build_chunk_mesh_naive(&chunk);

        assert_eq!(mesh_vertex_count(&naive), 40);
        assert_eq!(
            mesh_vertex_count(&greedy),
            mesh_vertex_count(&naive),
            "Different block types must not merge"
        );
    }

    #[test]
    fn test_same_block_types_are_merged() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        for x in 0..4 {
            chunk.set_block(x, 0, 0, BlockType::Stone);
        }

        let greedy = build_chunk_mesh(&chunk);
        let naive = build_chunk_mesh_naive(&chunk);

        let greedy_verts = mesh_vertex_count(&greedy);
        let naive_verts = mesh_vertex_count(&naive);

        assert_eq!(naive_verts, 72);
        // AO may prevent some merges on edge faces, but greedy should still help
        assert!(
            greedy_verts < naive_verts,
            "greedy ({greedy_verts}) should be less than naive ({naive_verts})"
        );
    }

    #[test]
    fn test_empty_chunk_no_vertices() {
        let chunk = Chunk::new(IVec3::ZERO);
        assert_eq!(mesh_vertex_count(&build_chunk_mesh(&chunk)), 0);
        assert_eq!(mesh_vertex_count(&build_chunk_mesh_naive(&chunk)), 0);
    }

    #[test]
    fn test_greedy_preserves_normals() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        for x in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                chunk.set_block(x, 0, z, BlockType::Grass);
            }
        }

        let mesh = build_chunk_mesh(&chunk);
        let normals: Vec<[f32; 3]> = mesh
            .attribute(Mesh::ATTRIBUTE_NORMAL)
            .unwrap()
            .as_float3()
            .unwrap()
            .to_vec();

        for n in &normals {
            let is_valid = *n == [0.0, 1.0, 0.0]
                || *n == [0.0, -1.0, 0.0]
                || *n == [0.0, 0.0, 1.0]
                || *n == [0.0, 0.0, -1.0]
                || *n == [1.0, 0.0, 0.0]
                || *n == [-1.0, 0.0, 0.0];
            assert!(is_valid, "unexpected normal: {n:?}");
        }
    }

    #[test]
    fn test_greedy_preserves_colors_isolated_block() {
        // Isolated block: AO=0 â†’ colors should match base exactly
        // Uses Stone (not subject to biome tinting) for exact color comparison.
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(8, 8, 8, BlockType::Stone);

        let mesh = build_chunk_mesh(&chunk);
        let expected = block_color(BlockType::Stone);

        if let Some(bevy::render::mesh::VertexAttributeValues::Float32x4(mesh_colors)) =
            mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        {
            for c in mesh_colors {
                assert_eq!(*c, expected, "isolated block should have full brightness");
            }
        } else {
            panic!("COLOR attribute missing or wrong type");
        }
    }

    #[test]
    fn test_checkerboard_no_merge() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        for x in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                let block = if (x + z) % 2 == 0 { BlockType::Stone } else { BlockType::Dirt };
                chunk.set_block(x, 0, z, block);
            }
        }

        let greedy = build_chunk_mesh(&chunk);
        let naive = build_chunk_mesh_naive(&chunk);

        assert_eq!(
            mesh_vertex_count(&greedy),
            mesh_vertex_count(&naive),
            "checkerboard should prevent all merging"
        );
    }

    #[test]
    fn test_flat_slab_merging() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        for x in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                chunk.set_block(x, 0, z, BlockType::Stone);
            }
        }

        let greedy = build_chunk_mesh(&chunk);
        let naive = build_chunk_mesh_naive(&chunk);

        let gv = mesh_vertex_count(&greedy);
        let nv = mesh_vertex_count(&naive);

        assert_eq!(nv, 2304);
        // Greedy with AO still provides significant reduction
        assert!(gv < nv / 2, "greedy ({gv}) should be much less than naive ({nv})");
    }

    // ==================================================================
    //  UV tests (updated for AO)
    // ==================================================================

    #[test]
    fn test_naive_mesh_has_uvs() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(0, 0, 0, BlockType::Stone);
        chunk.set_block(5, 5, 5, BlockType::Dirt);

        let mesh = build_chunk_mesh_naive(&chunk);
        let vert_count = mesh_vertex_count(&mesh);

        let uv_attr = mesh
            .attribute(Mesh::ATTRIBUTE_UV_0)
            .expect("naive mesh should have UV_0 attribute");
        assert_eq!(uv_attr.len(), vert_count, "UV count must equal vertex count");
    }

    #[test]
    fn test_greedy_mesh_has_uvs() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(0, 0, 0, BlockType::Stone);
        chunk.set_block(5, 5, 5, BlockType::Dirt);

        let mesh = build_chunk_mesh(&chunk);
        let vert_count = mesh_vertex_count(&mesh);

        let uv_attr = mesh
            .attribute(Mesh::ATTRIBUTE_UV_0)
            .expect("greedy mesh should have UV_0 attribute");
        assert_eq!(uv_attr.len(), vert_count, "UV count must equal vertex count");
    }

    #[test]
    fn test_greedy_uvs_scale_with_quad() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.fill(BlockType::Stone);

        let mesh = build_chunk_mesh(&chunk);

        if let Some(bevy::render::mesh::VertexAttributeValues::Float32x2(uv_data)) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        {
            let max_u = uv_data.iter().map(|uv| uv[0]).fold(0.0_f32, f32::max);
            let max_v = uv_data.iter().map(|uv| uv[1]).fold(0.0_f32, f32::max);

            // Interior blocks share AO=[0,0,0,0] and merge into large quads
            assert!(max_u > 1.0, "max U should be > 1.0 for merged quads, got {max_u}");
            assert!(max_v > 1.0, "max V should be > 1.0 for merged quads, got {max_v}");
        } else {
            panic!("UV_0 attribute missing or wrong type");
        }
    }

    #[test]
    fn test_single_block_uvs() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(7, 7, 7, BlockType::Stone);

        for (label, mesh) in [
            ("naive", build_chunk_mesh_naive(&chunk)),
            ("greedy", build_chunk_mesh(&chunk)),
        ] {
            if let Some(bevy::render::mesh::VertexAttributeValues::Float32x2(uv_data)) =
                mesh.attribute(Mesh::ATTRIBUTE_UV_0)
            {
                assert_eq!(uv_data.len(), 24, "{label}: expected 24 UVs");

                for uv in uv_data {
                    assert!(uv[0] >= 0.0 && uv[0] <= 1.0, "{label}: U out of range: {}", uv[0]);
                    assert!(uv[1] >= 0.0 && uv[1] <= 1.0, "{label}: V out of range: {}", uv[1]);
                }

                for face_idx in 0..6 {
                    let base = face_idx * 4;
                    let face_uvs: Vec<[f32; 2]> = uv_data[base..base + 4].to_vec();
                    assert!(face_uvs.contains(&[0.0, 0.0]), "{label} face {face_idx}: missing [0,0]");
                    assert!(face_uvs.contains(&[1.0, 0.0]), "{label} face {face_idx}: missing [1,0]");
                    assert!(face_uvs.contains(&[1.0, 1.0]), "{label} face {face_idx}: missing [1,1]");
                    assert!(face_uvs.contains(&[0.0, 1.0]), "{label} face {face_idx}: missing [0,1]");
                }
            } else {
                panic!("{label}: UV_0 attribute missing or wrong type");
            }
        }
    }

    #[test]
    fn test_atlas_uvs_stay_within_tile_bounds_for_greedy_quads() {
        // Arrange: a fully filled chunk produces very large greedy quads.
        // If atlas UVs scale with quad size, they will walk across neighboring tiles
        // and sample "random" textures (often including magenta debug tiles).
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.fill(BlockType::Stone);

        // Use a plausible atlas config (16x16 tiles, 16px per tile => 256px atlas).
        let ac = AtlasConfig {
            tiles_per_row: 16,
            tile_size: 64,
            atlas_size: 1024,
        };

        // Act
        let mesh = build_chunk_mesh_with_atlas(&chunk, Some(ac));

        // Assert
        let tile = texture_atlas::block_face_texture(BlockType::Stone, Face::Top);
        let (u_min, v_min, u_size, v_size) =
            texture_atlas::atlas_uv(tile, ac.tiles_per_row, ac.tile_size, ac.atlas_size);
        let u_max = u_min + u_size;
        let v_max = v_min + v_size;

        let Some(bevy::render::mesh::VertexAttributeValues::Float32x2(uvs)) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("UV_0 attribute missing or wrong type");
        };

        for uv in uvs {
            assert!(
                (0.0..=1.0).contains(&uv[0]) && (0.0..=1.0).contains(&uv[1]),
                "atlas UVs must be normalized: {:?}",
                uv
            );
            assert!(
                uv[0] >= u_min && uv[0] <= u_max && uv[1] >= v_min && uv[1] <= v_max,
                "atlas UV {:?} escaped tile bounds [{},{}]Ã—[{},{}]",
                uv,
                u_min,
                u_max,
                v_min,
                v_max
            );
        }
    }

    // ==================================================================
    //  AO + greedy interaction tests
    // ==================================================================

    #[test]
    fn test_greedy_and_naive_same_for_single_block() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(5, 5, 5, BlockType::Dirt);

        assert_eq!(
            mesh_vertex_count(&build_chunk_mesh(&chunk)),
            mesh_vertex_count(&build_chunk_mesh_naive(&chunk)),
            "single block: greedy and naive should match"
        );
    }

    #[test]
    fn test_ao_symmetry_on_symmetric_geometry() {
        // A single block should have symmetric AO on opposite faces
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(8, 8, 8, BlockType::Stone);

        let top = compute_face_ao(&chunk, 8, 8, 8, Face::Top);
        let bottom = compute_face_ao(&chunk, 8, 8, 8, Face::Bottom);

        // Both should be all zeros for an isolated block
        assert_eq!(top, [0, 0, 0, 0]);
        assert_eq!(bottom, [0, 0, 0, 0]);
    }

    // ==================================================================
    //  Texture atlas integration tests
    // ==================================================================

    #[test]
    fn test_use_textures_false_keeps_vertex_colors() {
        // With atlas=None (use_textures=false), vertex colors should be
        // the original block_color * AO â€” identical to legacy behaviour.
        // Uses Stone (not subject to biome tinting) for exact comparison.
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(8, 8, 8, BlockType::Stone);

        let legacy = build_chunk_mesh_with_atlas(&chunk, None);
        let expected = block_color(BlockType::Stone);

        if let Some(bevy::render::mesh::VertexAttributeValues::Float32x4(colors)) =
            legacy.attribute(Mesh::ATTRIBUTE_COLOR)
        {
            // Isolated block â†’ AO=0 â†’ colors == base block color
            for c in colors {
                assert_eq!(*c, expected, "legacy mode should use block_color");
            }
        } else {
            panic!("COLOR attribute missing");
        }

        // With atlas=Some, vertex colors should be white * AO instead
        let atlas_cfg = AtlasConfig {
            tiles_per_row: 16,
            tile_size: 64,
            atlas_size: 1024,
        };
        let textured = build_chunk_mesh_with_atlas(&chunk, Some(atlas_cfg));

        if let Some(bevy::render::mesh::VertexAttributeValues::Float32x4(colors)) =
            textured.attribute(Mesh::ATTRIBUTE_COLOR)
        {
            for c in colors {
                assert_eq!(*c, [1.0, 1.0, 1.0, 1.0], "atlas mode should use white vertex colors");
            }
        } else {
            panic!("COLOR attribute missing");
        }
    }

    #[test]
    fn test_atlas_uvs_single_block() {
        // With atlas enabled, a single Stone block (tile 0) should have
        // atlas-mapped UVs within the first tile region.
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(8, 8, 8, BlockType::Stone);

        let atlas_cfg = AtlasConfig {
            tiles_per_row: 16,
            tile_size: 64,
            atlas_size: 1024,
        };
        let mesh = build_chunk_mesh_with_atlas(&chunk, Some(atlas_cfg));

        if let Some(bevy::render::mesh::VertexAttributeValues::Float32x2(uv_data)) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        {
            // Stone = tile 0, so UVs should be within [0, 1/16] range
            let tile_uv = 1.0 / 16.0_f32;
            for uv in uv_data {
                assert!(uv[0] >= -1e-6 && uv[0] <= tile_uv + 1e-6,
                    "Stone tile 0 U should be in [0, {}], got {}", tile_uv, uv[0]);
                assert!(uv[1] >= -1e-6 && uv[1] <= tile_uv + 1e-6,
                    "Stone tile 0 V should be in [0, {}], got {}", tile_uv, uv[1]);
            }
        } else {
            panic!("UV_0 attribute missing");
        }
    }

    #[test]
    fn test_atlas_greedy_uvs_tile_correctly() {
        // For a packed atlas using StandardMaterial, greedy-merged quads must NOT
        // scale UVs beyond the tile rectangle (that would sample neighboring tiles
        // and cause stripes/magenta artifacts). Instead we keep UVs within the tile
        // bounds (the texture is stretched across the merged quad).
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.fill(BlockType::Stone);

        let atlas_cfg = AtlasConfig {
            tiles_per_row: 16,
            tile_size: 64,
            atlas_size: 1024,
        };
        let mesh = build_chunk_mesh_with_atlas(&chunk, Some(atlas_cfg));

        if let Some(bevy::render::mesh::VertexAttributeValues::Float32x2(uv_data)) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        {
            let tile_uv = 1.0 / 16.0_f32;
            let max_u = uv_data.iter().map(|uv| uv[0]).fold(0.0_f32, f32::max);
            let max_v = uv_data.iter().map(|uv| uv[1]).fold(0.0_f32, f32::max);
            let min_u = uv_data.iter().map(|uv| uv[0]).fold(1.0_f32, f32::min);
            let min_v = uv_data.iter().map(|uv| uv[1]).fold(1.0_f32, f32::min);

            assert!(
                min_u >= -1e-6 && min_v >= -1e-6,
                "atlas UV mins should be non-negative: min_u={min_u}, min_v={min_v}"
            );
            assert!(
                max_u <= tile_uv + 1e-6 && max_v <= tile_uv + 1e-6,
                "atlas UVs must stay within single-tile region: max_u={max_u}, max_v={max_v}, tile_uv={tile_uv}"
            );
        } else {
            panic!("UV_0 attribute missing");
        }
    }

    // ==================================================================
    //  UV1 (tile coordinate) tests
    // ==================================================================

    #[test]
    fn test_atlas_mesh_has_uv1() {
        // When atlas is enabled, the mesh should have UV_1 for tile coordinates.
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(8, 8, 8, BlockType::Stone);

        let atlas_cfg = AtlasConfig {
            tiles_per_row: 16,
            tile_size: 64,
            atlas_size: 1024,
        };
        let mesh = build_chunk_mesh_with_atlas(&chunk, Some(atlas_cfg));

        let uv1_attr = mesh
            .attribute(Mesh::ATTRIBUTE_UV_1)
            .expect("atlas mesh should have UV_1 attribute for tile coordinates");
        assert_eq!(
            uv1_attr.len(),
            mesh_vertex_count(&mesh),
            "UV1 count must equal vertex count"
        );
    }

    #[test]
    fn test_no_atlas_mesh_lacks_uv1() {
        // When atlas is disabled (None), the mesh should NOT have UV_1.
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(8, 8, 8, BlockType::Stone);

        let mesh = build_chunk_mesh_with_atlas(&chunk, None);
        assert!(
            mesh.attribute(Mesh::ATTRIBUTE_UV_1).is_none(),
            "non-atlas mesh should not have UV_1"
        );
    }

    #[test]
    fn test_uv1_contains_correct_tile_coordinates() {
        // Stone uses tile 0 for all faces â†’ tile_xy should be (0, 0)
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(8, 8, 8, BlockType::Stone);

        let atlas_cfg = AtlasConfig {
            tiles_per_row: 16,
            tile_size: 64,
            atlas_size: 1024,
        };
        let mesh = build_chunk_mesh_with_atlas(&chunk, Some(atlas_cfg));

        if let Some(bevy::render::mesh::VertexAttributeValues::Float32x2(uv1_data)) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_1)
        {
            for uv1 in uv1_data {
                // Stone tile 0 â†’ grid coords (0, 0)
                assert_eq!(*uv1, [0.0, 0.0], "Stone tile 0 should have UV1 = (0, 0)");
            }
        } else {
            panic!("UV_1 attribute missing or wrong type");
        }
    }

    #[test]
    fn test_uv1_grass_has_different_top_and_side_tiles() {
        // Grass has different tile indices for top (tile 2) and side (tile 3).
        // The UV1 values should reflect the correct tile grid coordinates.
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(8, 8, 8, BlockType::Grass);

        let atlas_cfg = AtlasConfig {
            tiles_per_row: 16,
            tile_size: 64,
            atlas_size: 1024,
        };
        let mesh = build_chunk_mesh_naive_with_atlas(&chunk, Some(atlas_cfg));

        if let Some(bevy::render::mesh::VertexAttributeValues::Float32x2(uv1_data)) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_1)
        {
            // Collect unique UV1 values
            let mut unique: Vec<[f32; 2]> = Vec::new();
            for uv1 in uv1_data {
                if !unique.iter().any(|u| (u[0] - uv1[0]).abs() < 1e-6 && (u[1] - uv1[1]).abs() < 1e-6) {
                    unique.push(*uv1);
                }
            }

            // Grass should have at least 2 different tile coordinates:
            // top (tile 2), bottom/dirt (tile 1), side (tile 3)
            assert!(
                unique.len() >= 2,
                "Grass should have multiple tile coordinates, got {:?}",
                unique
            );

            // Grass top = tile 2 â†’ (2, 0)
            assert!(
                unique.iter().any(|u| (u[0] - 2.0).abs() < 1e-6 && u[1].abs() < 1e-6),
                "Grass should have tile (2, 0) for top face, unique tiles: {:?}",
                unique
            );

            // Grass side = tile 3 â†’ (3, 0)
            assert!(
                unique.iter().any(|u| (u[0] - 3.0).abs() < 1e-6 && u[1].abs() < 1e-6),
                "Grass should have tile (3, 0) for side faces, unique tiles: {:?}",
                unique
            );
        } else {
            panic!("UV_1 attribute missing or wrong type");
        }
    }

    #[test]
    fn test_uv1_greedy_mesh_has_consistent_tile_coords() {
        // In a filled chunk of one block type (greedy-merged), all UV1 values
        // should be the same tile coordinate for faces of the same type.
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.fill(BlockType::Dirt);

        let atlas_cfg = AtlasConfig {
            tiles_per_row: 16,
            tile_size: 64,
            atlas_size: 1024,
        };
        let mesh = build_chunk_mesh_with_atlas(&chunk, Some(atlas_cfg));

        if let Some(bevy::render::mesh::VertexAttributeValues::Float32x2(uv1_data)) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_1)
        {
            // Dirt = tile 1 for all faces â†’ grid coords (1, 0)
            for uv1 in uv1_data {
                assert_eq!(
                    *uv1,
                    [1.0, 0.0],
                    "All Dirt vertices should have UV1 = (1, 0)"
                );
            }
        } else {
            panic!("UV_1 attribute missing or wrong type");
        }
    }

    #[test]
    fn test_atlas_uv_inset_prevents_boundary_sampling() {
        // Verify that the full-texel inset in face_uvs_atlas keeps UVs strictly
        // inside the tile region, never touching the exact tile boundary.
        let atlas_cfg = AtlasConfig {
            tiles_per_row: 16,
            tile_size: 64,
            atlas_size: 1024,
        };

        // Check every tile that's actually used
        for tile_idx in 0..=texture_atlas::MAX_TILE_INDEX {
            let uvs = texture_atlas::face_uvs_atlas(
                tile_idx,
                atlas_cfg.tiles_per_row,
                atlas_cfg.tile_size,
                atlas_cfg.atlas_size,
                1.0,
                1.0,
            );
            let (u_min, v_min, u_size, v_size) = texture_atlas::atlas_uv(
                tile_idx,
                atlas_cfg.tiles_per_row,
                atlas_cfg.tile_size,
                atlas_cfg.atlas_size,
            );
            let u_max = u_min + u_size;
            let v_max = v_min + v_size;

            for uv in &uvs {
                // UVs must be strictly inside the tile (not on the boundary)
                assert!(
                    uv[0] > u_min && uv[0] < u_max,
                    "Tile {tile_idx}: U={} is on or outside tile boundary [{u_min}, {u_max}]",
                    uv[0]
                );
                assert!(
                    uv[1] > v_min && uv[1] < v_max,
                    "Tile {tile_idx}: V={} is on or outside tile boundary [{v_min}, {v_max}]",
                    uv[1]
                );
            }
        }
    }
}
