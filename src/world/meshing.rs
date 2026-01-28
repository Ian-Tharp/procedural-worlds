//! Chunk mesh generation - converts voxel data to renderable meshes

use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;

use super::{BlockType, Chunk, CHUNK_SIZE};

/// Face direction for cube faces
#[derive(Clone, Copy)]
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

/// Add vertices for a single face of a cube
fn add_face(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    colors: &mut Vec<[f32; 4]>,
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

    // Add 4 vertices
    for vert in verts {
        positions.push(vert);
        normals.push(normal);
        colors.push(color);
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

/// Check if a neighboring block at the given offset is transparent
fn is_neighbor_transparent(chunk: &Chunk, x: i32, y: i32, z: i32) -> bool {
    // If outside chunk bounds, treat as transparent (air)
    if x < 0 || x >= CHUNK_SIZE as i32 || y < 0 || y >= CHUNK_SIZE as i32 || z < 0 || z >= CHUNK_SIZE as i32 {
        return true;
    }
    chunk.get_block(x as usize, y as usize, z as usize).is_transparent()
}

/// Build a mesh for a chunk using naive face culling
///
/// This generates one face per visible block face (faces adjacent to transparent blocks).
/// Uses vertex colors for block coloring.
pub fn build_chunk_mesh(chunk: &Chunk) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
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
    mesh.insert_indices(Indices::U32(indices));

    mesh
}
