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
        BlockType::Water => [0.2, 0.4, 0.8, 1.0],
        BlockType::Wood => [0.5, 0.35, 0.2, 1.0],
        BlockType::Leaves => [0.2, 0.5, 0.15, 1.0],
        BlockType::Sandstone => [0.82, 0.73, 0.53, 1.0],
        BlockType::Snow => [0.95, 0.95, 0.97, 1.0],
        BlockType::Ice => [0.7, 0.85, 0.95, 1.0],
    }
}

/// Add vertices for a single 1×1 face of a cube (used by naive meshing).
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
    indices: &mut Vec<u32>,
    x: f32,
    y: f32,
    z: f32,
    face: Face,
    color: [f32; 4],
    ao: [u8; 4],
) {
    let base_index = positions.len() as u32;
    let normal = face.normal();

    let verts: [[f32; 3]; 4] = match face {
        Face::Top => [
            [x, y + 1.0, z],
            [x + 1.0, y + 1.0, z],
            [x + 1.0, y + 1.0, z + 1.0],
            [x, y + 1.0, z + 1.0],
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

    let face_uvs: [[f32; 2]; 4] = [
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
    ];

    for (i, vert) in verts.iter().enumerate() {
        positions.push(*vert);
        normals.push(normal);
        colors.push(apply_ao(color, ao[i]));
        uvs.push(face_uvs[i]);
    }

    // Flip the quad diagonal when AO creates asymmetry.
    // Default triangulation: (0,2,1) + (0,3,2)  — diagonal along 0-2
    // Flipped triangulation: (0,3,1) + (1,3,2)  — diagonal along 1-3
    // Flip when ao[0]+ao[2] > ao[1]+ao[3] to keep the brighter diagonal.
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
    indices: &mut Vec<u32>,
    x: f32,
    y: f32,
    z: f32,
    quad_w: f32,
    quad_h: f32,
    face: Face,
    color: [f32; 4],
    ao: [u8; 4],
) {
    let base_index = positions.len() as u32;
    let normal = face.normal();

    let verts: [[f32; 3]; 4] = match face {
        Face::Top => [
            [x, y + 1.0, z],
            [x + quad_w, y + 1.0, z],
            [x + quad_w, y + 1.0, z + quad_h],
            [x, y + 1.0, z + quad_h],
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

    let face_uvs: [[f32; 2]; 4] = [
        [0.0, 0.0],
        [quad_w, 0.0],
        [quad_w, quad_h],
        [0.0, quad_h],
    ];

    for (i, vert) in verts.iter().enumerate() {
        positions.push(*vert);
        normals.push(normal);
        colors.push(apply_ao(color, ao[i]));
        uvs.push(face_uvs[i]);
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

// ── Ambient Occlusion ────────────────────────────────────────────────

/// AO darkening multipliers for levels 0-3.
/// Level 0 = no occlusion (full brightness), level 3 = maximum occlusion.
const AO_CURVE: [f32; 4] = [1.0, 0.75, 0.5, 0.25];

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

/// Build a mesh for a chunk using **greedy meshing**.
///
/// For each of the 6 face directions, iterates over 2-D slices perpendicular to the
/// face normal. Within each slice a boolean+BlockType mask of visible faces is built,
/// then rectangles of identical block type are greedily merged before being emitted as
/// single quads. This typically reduces vertex count by 80-90 % compared to naive
/// per-block-face generation.
#[allow(clippy::needless_range_loop)]
pub fn build_chunk_mesh(chunk: &Chunk) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
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
                    // Map (slice, u, v) → (x, y, z) depending on face direction.
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

                    if is_neighbor_transparent(chunk, nx, ny, nz) {
                        let ao = compute_face_ao(chunk, x, y, z, face);
                        mask[v][u] = Some((block, ao));
                    }
                }
            }

            // Greedy rectangle merging — matches on (BlockType, AO pattern)
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
                        &mut indices,
                        x as f32,
                        y as f32,
                        z as f32,
                        w as f32,
                        h as f32,
                        face,
                        color,
                        ao,
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
    mesh.insert_indices(Indices::U32(indices));

    mesh
}

/// Build a mesh for a chunk using **naive** face culling (one quad per visible block face).
///
/// Retained for benchmarking comparisons against greedy meshing.
#[allow(dead_code)]
pub fn build_chunk_mesh_naive(chunk: &Chunk) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
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

                    // Only add face if neighbor is transparent
                    if is_neighbor_transparent(chunk, nx, ny, nz) {
                        let ao = compute_face_ao(chunk, x, y, z, face);
                        add_face(
                            &mut positions,
                            &mut normals,
                            &mut colors,
                            &mut uvs,
                            &mut indices,
                            fx,
                            fy,
                            fz,
                            face,
                            color,
                            ao,
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
        // Both sides occlude → max AO regardless of corner
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

        // AO level 1: 0.75× darkening
        let ao1 = apply_ao(color, 1);
        assert!((ao1[0] - 0.75).abs() < 1e-6);
        assert!((ao1[1] - 0.6).abs() < 1e-6);
        assert!((ao1[2] - 0.45).abs() < 1e-5); // float precision
        assert_eq!(ao1[3], 1.0);

        // AO level 2: 0.5× darkening
        let ao2 = apply_ao(color, 2);
        assert!((ao2[0] - 0.5).abs() < 1e-6);
        assert!((ao2[1] - 0.4).abs() < 1e-6);
        assert!((ao2[2] - 0.3).abs() < 1e-6);

        // AO level 3: 0.25× darkening
        let ao3 = apply_ao(color, 3);
        assert!((ao3[0] - 0.25).abs() < 1e-6);
        assert!((ao3[1] - 0.2).abs() < 1e-6);
        assert!((ao3[2] - 0.15).abs() < 1e-5);

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
        // Fill a 3×3×3 region, check AO on center block's top face
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
        // Isolated block → AO=0 everywhere → colors should be unmodified
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
        // 3×3×3 cube with center-top removed creates AO on cavity walls
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

        // Single block in air → AO=0 on all faces → all merge normally
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
        // Isolated block: AO=0 → colors should match base exactly
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(8, 8, 8, BlockType::Grass);

        let mesh = build_chunk_mesh(&chunk);
        let expected = block_color(BlockType::Grass);

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
}
