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
    }
}

/// Add vertices for a single 1×1 face of a cube (used by naive meshing)
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
) {
    let base_index = positions.len() as u32;
    let normal = face.normal();

    // Vertices for each face (4 vertices per face, forming 2 triangles)
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

    // Standard 1×1 UVs matching vertex winding
    let face_uvs: [[f32; 2]; 4] = [
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
    ];

    // Add 4 vertices
    for (i, vert) in verts.iter().enumerate() {
        positions.push(*vert);
        normals.push(normal);
        colors.push(color);
        uvs.push(face_uvs[i]);
    }

    // Add 2 triangles (6 indices) - counter-clockwise winding for front-facing
    indices.extend_from_slice(&[
        base_index,
        base_index + 2,
        base_index + 1,
        base_index,
        base_index + 3,
        base_index + 2,
    ]);
}

/// Add vertices for a greedy-merged quad face.
///
/// The `quad_w` and `quad_h` parameters represent the extent of the merged quad
/// in the face's two planar axes:
/// - **Top/Bottom** (Y-normal): `quad_w` = extent in X, `quad_h` = extent in Z
/// - **North/South** (Z-normal): `quad_w` = extent in X, `quad_h` = extent in Y
/// - **East/West** (X-normal): `quad_w` = extent in Z, `quad_h` = extent in Y
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

    // UVs scale with quad dimensions for tiling textures across merged faces
    let face_uvs: [[f32; 2]; 4] = [
        [0.0, 0.0],
        [quad_w, 0.0],
        [quad_w, quad_h],
        [0.0, quad_h],
    ];

    for (i, vert) in verts.iter().enumerate() {
        positions.push(*vert);
        normals.push(normal);
        colors.push(color);
        uvs.push(face_uvs[i]);
    }

    indices.extend_from_slice(&[
        base_index,
        base_index + 2,
        base_index + 1,
        base_index,
        base_index + 3,
        base_index + 2,
    ]);
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
            // mask[v][u] = Some(block_type) if the face is visible, None otherwise.
            let mut mask: [[Option<BlockType>; CHUNK_SIZE]; CHUNK_SIZE] =
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
                        mask[v][u] = Some(block);
                    }
                }
            }

            // Greedy rectangle merging
            let mut visited = [[false; CHUNK_SIZE]; CHUNK_SIZE];

            for v in 0..CHUNK_SIZE {
                for u in 0..CHUNK_SIZE {
                    if visited[v][u] || mask[v][u].is_none() {
                        continue;
                    }

                    let block_type = mask[v][u].unwrap();

                    // Expand width in the u-direction
                    let mut w = 1usize;
                    while u + w < CHUNK_SIZE
                        && !visited[v][u + w]
                        && mask[v][u + w] == Some(block_type)
                    {
                        w += 1;
                    }

                    // Expand height in the v-direction
                    let mut h = 1usize;
                    'expand_v: while v + h < CHUNK_SIZE {
                        for du in 0..w {
                            if visited[v + h][u + du]
                                || mask[v + h][u + du] != Some(block_type)
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

    // ------------------------------------------------------------------
    // Greedy vs naive: filled chunk produces far fewer vertices
    // ------------------------------------------------------------------
    #[test]
    fn test_greedy_fewer_vertices_than_naive_filled_chunk() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.fill(BlockType::Stone);

        let greedy = build_chunk_mesh(&chunk);
        let naive = build_chunk_mesh_naive(&chunk);

        let greedy_verts = mesh_vertex_count(&greedy);
        let naive_verts = mesh_vertex_count(&naive);

        // A fully filled chunk has 6 outer faces, each 16×16 = 256 block-faces.
        // Naive: 6 * 256 * 4 = 6144 vertices
        // Greedy: each face direction merges into one 16×16 quad → 6 * 4 = 24 vertices
        assert_eq!(naive_verts, 6144);
        assert_eq!(greedy_verts, 24);
        assert!(
            greedy_verts < naive_verts,
            "greedy ({greedy_verts}) should be less than naive ({naive_verts})"
        );
    }

    // ------------------------------------------------------------------
    // Single block: greedy and naive should both emit 6 faces (24 verts)
    // ------------------------------------------------------------------
    #[test]
    fn test_single_block_produces_six_faces() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(7, 7, 7, BlockType::Stone);

        let greedy = build_chunk_mesh(&chunk);
        let naive = build_chunk_mesh_naive(&chunk);

        let greedy_verts = mesh_vertex_count(&greedy);
        let naive_verts = mesh_vertex_count(&naive);

        // Single block surrounded by air → 6 faces, 4 verts each
        assert_eq!(greedy_verts, 24);
        assert_eq!(naive_verts, 24);
    }

    // ------------------------------------------------------------------
    // Different adjacent block types must NOT be merged
    // ------------------------------------------------------------------
    #[test]
    fn test_different_block_types_not_merged() {
        let mut chunk = Chunk::new(IVec3::ZERO);

        // Place two different blocks side by side on the top layer
        chunk.set_block(0, 0, 0, BlockType::Stone);
        chunk.set_block(1, 0, 0, BlockType::Dirt);

        let greedy = build_chunk_mesh(&chunk);
        let naive = build_chunk_mesh_naive(&chunk);

        let greedy_verts = mesh_vertex_count(&greedy);
        let naive_verts = mesh_vertex_count(&naive);

        // Two isolated blocks each with faces exposed.
        // They share an internal boundary: Stone's East face sees Dirt (not transparent),
        // and Dirt's West face sees Stone (not transparent) → those faces are culled.
        // Each block has 5 visible faces → 10 faces total → 40 verts for both methods.
        assert_eq!(naive_verts, 40);
        assert_eq!(greedy_verts, naive_verts,
            "Different block types must not merge — vertex counts should match naive");
    }

    // ------------------------------------------------------------------
    // Same adjacent block types ARE merged (fewer verts than naive)
    // ------------------------------------------------------------------
    #[test]
    fn test_same_block_types_are_merged() {
        let mut chunk = Chunk::new(IVec3::ZERO);

        // Place a row of 4 stone blocks along X at y=0, z=0
        for x in 0..4 {
            chunk.set_block(x, 0, 0, BlockType::Stone);
        }

        let greedy = build_chunk_mesh(&chunk);
        let naive = build_chunk_mesh_naive(&chunk);

        let greedy_verts = mesh_vertex_count(&greedy);
        let naive_verts = mesh_vertex_count(&naive);

        // Naive: each block has 4 external faces (internal X-faces are culled between
        // adjacent same-type blocks). End blocks have 5, inner blocks have 4.
        // Specifically: 2 end blocks × 5 faces + 2 inner blocks × 4 faces = 18 faces → 72 verts
        // Actually let me recount:
        // Block 0: Top, Bottom, North, South, West (East is culled — neighbor is Stone at 1)  → 5
        // Block 1: Top, Bottom, North, South  (West culled by 0, East culled by 2)            → 4
        // Block 2: Top, Bottom, North, South  (West culled by 1, East culled by 3)            → 4
        // Block 3: Top, Bottom, North, South, East (West culled by 2)                         → 5
        // Total = 18 faces → 72 verts
        assert_eq!(naive_verts, 72);

        // Greedy should merge coplanar same-type faces.
        // Top face: 4 blocks merge into one 4×1 quad → 4 verts
        // Bottom: same → 4
        // North: same → 4
        // South: same → 4
        // West: 1 block face → 4
        // East: 1 block face → 4
        // Total = 6 quads → 24 verts
        assert_eq!(greedy_verts, 24);
        assert!(
            greedy_verts < naive_verts,
            "greedy ({greedy_verts}) should be less than naive ({naive_verts})"
        );
    }

    // ------------------------------------------------------------------
    // Empty chunk produces zero vertices for both methods
    // ------------------------------------------------------------------
    #[test]
    fn test_empty_chunk_no_vertices() {
        let chunk = Chunk::new(IVec3::ZERO);

        let greedy = build_chunk_mesh(&chunk);
        let naive = build_chunk_mesh_naive(&chunk);

        assert_eq!(mesh_vertex_count(&greedy), 0);
        assert_eq!(mesh_vertex_count(&naive), 0);
    }

    // ------------------------------------------------------------------
    // Greedy meshing preserves correct normals
    // ------------------------------------------------------------------
    #[test]
    fn test_greedy_preserves_normals() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        // Fill bottom layer only
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

        // Every normal should be one of the 6 axis-aligned directions
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

    // ------------------------------------------------------------------
    // Greedy meshing preserves vertex colors
    // ------------------------------------------------------------------
    #[test]
    fn test_greedy_preserves_colors() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(0, 0, 0, BlockType::Grass);

        let mesh = build_chunk_mesh(&chunk);
        let expected = block_color(BlockType::Grass);

        // Read colors from the mesh attribute
        if let Some(bevy::render::mesh::VertexAttributeValues::Float32x4(mesh_colors)) =
            mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        {
            for c in mesh_colors {
                assert_eq!(*c, expected, "vertex color should match Grass color");
            }
        } else {
            panic!("COLOR attribute missing or wrong type");
        }
    }

    // ------------------------------------------------------------------
    // Checkerboard pattern: no merging possible, counts must match naive
    // ------------------------------------------------------------------
    #[test]
    fn test_checkerboard_no_merge() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        // Checkerboard of Stone and Dirt on a single Y-layer
        for x in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                let block = if (x + z) % 2 == 0 {
                    BlockType::Stone
                } else {
                    BlockType::Dirt
                };
                chunk.set_block(x, 0, z, block);
            }
        }

        let greedy = build_chunk_mesh(&chunk);
        let naive = build_chunk_mesh_naive(&chunk);

        // With a perfect checkerboard the greedy algorithm cannot merge any faces
        // within the Top or Bottom planes (every neighbor is a different type).
        // North/South/East/West faces can still merge along Y (height 1 only) but
        // each cell is isolated in the u-direction because neighbors differ.
        // So vertex counts should be identical.
        assert_eq!(
            mesh_vertex_count(&greedy),
            mesh_vertex_count(&naive),
            "checkerboard should prevent all merging"
        );
    }

    // ------------------------------------------------------------------
    // Flat slab: big merge on top/bottom, partial on sides
    // ------------------------------------------------------------------
    #[test]
    fn test_flat_slab_merging() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        // 16×1×16 slab of stone at y=0
        for x in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                chunk.set_block(x, 0, z, BlockType::Stone);
            }
        }

        let greedy = build_chunk_mesh(&chunk);
        let naive = build_chunk_mesh_naive(&chunk);

        let gv = mesh_vertex_count(&greedy);
        let nv = mesh_vertex_count(&naive);

        // Naive: Top 256 + Bottom 256 + North 16 + South 16 + East 16 + West 16 = 576 faces
        // → 576 * 4 = 2304 verts
        assert_eq!(nv, 2304);

        // Greedy: Top 1 quad + Bottom 1 quad + North 1 quad + South 1 quad
        //         + East 1 quad + West 1 quad = 6 quads → 24 verts
        assert_eq!(gv, 24);
    }

    // ------------------------------------------------------------------
    // UV coordinates: naive mesh has UVs with correct count
    // ------------------------------------------------------------------
    #[test]
    fn test_naive_mesh_has_uvs() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(0, 0, 0, BlockType::Stone);
        chunk.set_block(5, 5, 5, BlockType::Dirt);

        let mesh = build_chunk_mesh_naive(&chunk);
        let vert_count = mesh_vertex_count(&mesh);

        // UV attribute must exist
        let uv_attr = mesh
            .attribute(Mesh::ATTRIBUTE_UV_0)
            .expect("naive mesh should have UV_0 attribute");
        assert_eq!(
            uv_attr.len(),
            vert_count,
            "UV count must equal vertex count"
        );
    }

    // ------------------------------------------------------------------
    // UV coordinates: greedy mesh has UVs with correct count
    // ------------------------------------------------------------------
    #[test]
    fn test_greedy_mesh_has_uvs() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(0, 0, 0, BlockType::Stone);
        chunk.set_block(5, 5, 5, BlockType::Dirt);

        let mesh = build_chunk_mesh(&chunk);
        let vert_count = mesh_vertex_count(&mesh);

        // UV attribute must exist
        let uv_attr = mesh
            .attribute(Mesh::ATTRIBUTE_UV_0)
            .expect("greedy mesh should have UV_0 attribute");
        assert_eq!(
            uv_attr.len(),
            vert_count,
            "UV count must equal vertex count"
        );
    }

    // ------------------------------------------------------------------
    // UV coordinates: greedy UVs scale with quad dimensions
    // ------------------------------------------------------------------
    #[test]
    fn test_greedy_uvs_scale_with_quad() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.fill(BlockType::Stone);

        let mesh = build_chunk_mesh(&chunk);

        // A filled chunk produces 6 faces, each merged into a single 16×16 quad.
        // The greedy UVs should scale: max UV component should be 16.0.
        if let Some(bevy::render::mesh::VertexAttributeValues::Float32x2(uv_data)) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        {
            let max_u = uv_data
                .iter()
                .map(|uv| uv[0])
                .fold(0.0_f32, f32::max);
            let max_v = uv_data
                .iter()
                .map(|uv| uv[1])
                .fold(0.0_f32, f32::max);

            assert_eq!(
                max_u, 16.0,
                "max U should be 16.0 for a full-chunk greedy quad"
            );
            assert_eq!(
                max_v, 16.0,
                "max V should be 16.0 for a full-chunk greedy quad"
            );
        } else {
            panic!("UV_0 attribute missing or wrong type");
        }
    }

    // ------------------------------------------------------------------
    // UV coordinates: single block has standard 0-1 UVs for both methods
    // ------------------------------------------------------------------
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
                // Single block → 6 faces × 4 verts = 24 UVs
                assert_eq!(uv_data.len(), 24, "{label}: expected 24 UVs");

                // Every UV component should be in [0.0, 1.0]
                for uv in uv_data {
                    assert!(
                        uv[0] >= 0.0 && uv[0] <= 1.0,
                        "{label}: U out of range: {}",
                        uv[0]
                    );
                    assert!(
                        uv[1] >= 0.0 && uv[1] <= 1.0,
                        "{label}: V out of range: {}",
                        uv[1]
                    );
                }

                // Check that each face has the expected UV corners {0,0}, {1,0}, {1,1}, {0,1}
                for face_idx in 0..6 {
                    let base = face_idx * 4;
                    let face_uvs: Vec<[f32; 2]> = uv_data[base..base + 4].to_vec();
                    assert!(
                        face_uvs.contains(&[0.0, 0.0]),
                        "{label} face {face_idx}: missing [0,0]"
                    );
                    assert!(
                        face_uvs.contains(&[1.0, 0.0]),
                        "{label} face {face_idx}: missing [1,0]"
                    );
                    assert!(
                        face_uvs.contains(&[1.0, 1.0]),
                        "{label} face {face_idx}: missing [1,1]"
                    );
                    assert!(
                        face_uvs.contains(&[0.0, 1.0]),
                        "{label} face {face_idx}: missing [0,1]"
                    );
                }
            } else {
                panic!("{label}: UV_0 attribute missing or wrong type");
            }
        }
    }
}
