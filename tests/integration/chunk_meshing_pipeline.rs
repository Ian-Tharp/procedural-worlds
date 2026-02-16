//! Integration tests for the chunk meshing pipeline.
//!
//! Verifies the flow from generated chunk data through mesh construction,
//! including greedy meshing, cross-chunk neighbor awareness, and
//! opaque/water mesh splitting.

use bevy::math::IVec3;
use bevy::prelude::Mesh;
use bevy::tasks::{AsyncComputeTaskPool, block_on};

use procedural_worlds::generation::{
    TerrainConfig, generate_cacti, generate_caves, generate_chunk_terrain, generate_trees,
};
use procedural_worlds::world::meshing::{
    ChunkNeighbors, build_chunk_mesh, build_chunk_mesh_with_neighbors,
};
use procedural_worlds::world::{BlockType, CHUNK_SIZE, CHUNK_VOLUME, Chunk};

// ============================================================================
// Helpers
// ============================================================================

fn init_task_pool() {
    AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
}

fn generate_full(chunk: &mut Chunk, config: &TerrainConfig) {
    generate_chunk_terrain(chunk, config);
    generate_caves(chunk, config);
    generate_trees(chunk, config);
    generate_cacti(chunk, config);
}

fn mesh_vertex_count(mesh: &Mesh) -> usize {
    mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        .map(|attr| attr.len())
        .unwrap_or(0)
}

fn mesh_index_count(mesh: &Mesh) -> usize {
    mesh.indices().map(|i| i.len()).unwrap_or(0)
}

// ============================================================================
// Tests: Generated terrain produces valid meshes
// ============================================================================

/// Generate realistic terrain and verify the mesh has geometry.
#[test]
fn generated_terrain_produces_nonempty_mesh() {
    let config = TerrainConfig::default();
    let pos = IVec3::new(0, 2, 0); // Surface-level chunk

    let mut chunk = Chunk::new(pos);
    generate_full(&mut chunk, &config);

    let mesh = build_chunk_mesh(&chunk);
    let verts = mesh_vertex_count(&mesh);
    let indices = mesh_index_count(&mesh);

    assert!(verts > 0, "Surface chunk should produce vertices");
    assert!(indices > 0, "Surface chunk should produce indices");
    // Indices should be a multiple of 3 (triangles)
    assert_eq!(indices % 3, 0, "Index count should be divisible by 3");
}

/// Underground chunks (below sea level) should have surface faces at boundaries.
#[test]
fn underground_chunk_has_boundary_faces() {
    let config = TerrainConfig::default();
    let pos = IVec3::new(0, 0, 0); // Below surface

    let mut chunk = Chunk::new(pos);
    generate_full(&mut chunk, &config);

    let mesh = build_chunk_mesh(&chunk);
    let verts = mesh_vertex_count(&mesh);

    // Underground chunks should have faces at chunk boundaries
    // (where they're adjacent to air/unloaded chunks)
    assert!(verts > 0, "Underground chunk should have boundary faces");
}

/// A chunk high in the sky (all air) should produce an empty mesh.
#[test]
fn sky_chunk_produces_empty_mesh() {
    let config = TerrainConfig::default();
    let pos = IVec3::new(0, 10, 0); // Well above terrain

    let mut chunk = Chunk::new(pos);
    generate_full(&mut chunk, &config);

    let mesh = build_chunk_mesh(&chunk);
    let verts = mesh_vertex_count(&mesh);

    assert_eq!(verts, 0, "Sky chunk should produce no vertices");
}

// ============================================================================
// Tests: Opaque/water mesh splitting
// ============================================================================

/// A chunk with only water should produce a water mesh and an empty opaque mesh.
#[test]
fn water_only_chunk_splits_correctly() {
    let mut chunk = Chunk::new(IVec3::ZERO);
    chunk.fill(BlockType::Water);

    let neighbors = ChunkNeighbors::empty();
    let (opaque, water) = build_chunk_mesh_with_neighbors(&chunk, None, &neighbors);

    // Water faces should be in the water mesh
    let water_mesh = water.expect("Water-filled chunk should produce a water mesh");
    let water_verts = mesh_vertex_count(&water_mesh);
    assert!(water_verts > 0, "Water mesh should have vertices");

    // Opaque mesh should be empty (water is transparent)
    let opaque_verts = mesh_vertex_count(&opaque);
    assert_eq!(
        opaque_verts, 0,
        "Opaque mesh should be empty for water-only chunk"
    );
}

/// A chunk with only solid blocks should produce no water mesh.
#[test]
fn solid_only_chunk_no_water_mesh() {
    let mut chunk = Chunk::new(IVec3::ZERO);
    chunk.fill(BlockType::Stone);

    let neighbors = ChunkNeighbors::empty();
    let (opaque, water) = build_chunk_mesh_with_neighbors(&chunk, None, &neighbors);

    assert!(
        water.is_none(),
        "Solid-only chunk should not produce water mesh"
    );
    assert!(
        mesh_vertex_count(&opaque) > 0,
        "Solid chunk should have opaque vertices"
    );
}

/// A chunk with both solid and water blocks should produce both meshes.
#[test]
fn mixed_chunk_produces_both_meshes() {
    let mut chunk = Chunk::new(IVec3::ZERO);

    // Bottom half: stone, top half: water
    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            for y in 0..8 {
                chunk.set_block(x, y, z, BlockType::Stone);
            }
            for y in 8..CHUNK_SIZE {
                chunk.set_block(x, y, z, BlockType::Water);
            }
        }
    }

    let neighbors = ChunkNeighbors::empty();
    let (opaque, water) = build_chunk_mesh_with_neighbors(&chunk, None, &neighbors);

    let opaque_verts = mesh_vertex_count(&opaque);
    let water_mesh = water.expect("Mixed chunk should produce water mesh");
    let water_verts = mesh_vertex_count(&water_mesh);

    assert!(opaque_verts > 0, "Mixed chunk should have opaque vertices");
    assert!(water_verts > 0, "Mixed chunk should have water vertices");
}

// ============================================================================
// Tests: Cross-chunk neighbor meshing
// ============================================================================

/// When a solid chunk has a solid neighbor, internal boundary faces should
/// be culled, producing fewer vertices than without the neighbor.
#[test]
fn neighbor_culling_reduces_face_count() {
    let mut chunk = Chunk::new(IVec3::ZERO);
    chunk.fill(BlockType::Stone);

    // Build mesh without neighbors (boundary faces rendered)
    let no_neighbors = ChunkNeighbors::empty();
    let (mesh_alone, _) = build_chunk_mesh_with_neighbors(&chunk, None, &no_neighbors);
    let verts_alone = mesh_vertex_count(&mesh_alone);

    // Build mesh with a solid +X neighbor
    let mut neighbor_blocks = vec![BlockType::Air; CHUNK_VOLUME];
    neighbor_blocks.fill(BlockType::Stone);
    let with_neighbor = ChunkNeighbors {
        pos_x: Some(neighbor_blocks),
        neg_x: None,
        pos_y: None,
        neg_y: None,
        pos_z: None,
        neg_z: None,
    };
    let (mesh_with, _) = build_chunk_mesh_with_neighbors(&chunk, None, &with_neighbor);
    let verts_with = mesh_vertex_count(&mesh_with);

    assert!(
        verts_with < verts_alone,
        "Solid neighbor should cull boundary faces: {} < {}",
        verts_with,
        verts_alone
    );
}

/// With all six solid neighbors, the only faces should be internal
/// (none at boundaries), which for a fully solid chunk means zero faces.
#[test]
fn fully_surrounded_chunk_has_no_faces() {
    let mut chunk = Chunk::new(IVec3::ZERO);
    chunk.fill(BlockType::Stone);

    let solid_neighbor: Vec<BlockType> = vec![BlockType::Stone; CHUNK_VOLUME];
    let all_solid = ChunkNeighbors {
        pos_x: Some(solid_neighbor.clone()),
        neg_x: Some(solid_neighbor.clone()),
        pos_y: Some(solid_neighbor.clone()),
        neg_y: Some(solid_neighbor.clone()),
        pos_z: Some(solid_neighbor.clone()),
        neg_z: Some(solid_neighbor.clone()),
    };

    let (opaque, water) = build_chunk_mesh_with_neighbors(&chunk, None, &all_solid);

    assert_eq!(
        mesh_vertex_count(&opaque),
        0,
        "Fully surrounded solid chunk should have no visible faces"
    );
    assert!(water.is_none());
}

// ============================================================================
// Tests: Async meshing matches sync meshing
// ============================================================================

/// Verify that meshing a chunk on the async task pool produces the same
/// result as meshing on the main thread.
#[test]
fn async_meshing_matches_sync() {
    init_task_pool();

    let config = TerrainConfig::default();
    let pos = IVec3::new(0, 2, 0);

    let mut chunk = Chunk::new(pos);
    generate_full(&mut chunk, &config);

    // Sync reference
    let ref_mesh = build_chunk_mesh(&chunk);

    // Async
    let chunk_clone = chunk.clone();
    let task = AsyncComputeTaskPool::get().spawn(async move { build_chunk_mesh(&chunk_clone) });
    let async_mesh = block_on(task);

    assert_eq!(
        mesh_vertex_count(&ref_mesh),
        mesh_vertex_count(&async_mesh),
        "Async and sync meshing should produce same vertex count"
    );
    assert_eq!(
        mesh_index_count(&ref_mesh),
        mesh_index_count(&async_mesh),
        "Async and sync meshing should produce same index count"
    );
}

/// Generate and mesh multiple chunks concurrently, verify all produce valid geometry.
#[test]
fn concurrent_mesh_generation_all_valid() {
    init_task_pool();

    let config = TerrainConfig::default();
    let task_pool = AsyncComputeTaskPool::get();

    let positions = vec![
        IVec3::new(0, 2, 0),
        IVec3::new(1, 2, 0),
        IVec3::new(0, 2, 1),
        IVec3::new(-1, 2, -1),
    ];

    let tasks: Vec<_> = positions
        .iter()
        .map(|&pos| {
            let cfg = config.clone();
            task_pool.spawn(async move {
                let mut chunk = Chunk::new(pos);
                generate_chunk_terrain(&mut chunk, &cfg);
                generate_caves(&mut chunk, &cfg);
                generate_trees(&mut chunk, &cfg);
                generate_cacti(&mut chunk, &cfg);
                let mesh = build_chunk_mesh(&chunk);
                (pos, mesh)
            })
        })
        .collect();

    for task in tasks {
        let (pos, mesh) = block_on(task);
        let verts = mesh_vertex_count(&mesh);
        let indices = mesh_index_count(&mesh);

        // Surface chunks should always have some geometry
        assert!(verts > 0, "Chunk at {:?} should have vertices", pos);
        assert_eq!(
            indices % 3,
            0,
            "Chunk at {:?} should have triangle indices",
            pos
        );
    }
}

// ============================================================================
// Tests: Mesh attribute integrity
// ============================================================================

/// Verify that all mesh attributes (position, normal, color, UV) have
/// consistent lengths.
#[test]
fn mesh_attributes_consistent_lengths() {
    let mut chunk = Chunk::new(IVec3::ZERO);
    chunk.fill(BlockType::Stone);

    let mesh = build_chunk_mesh(&chunk);

    let pos_count = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .map(|a| a.len())
        .unwrap_or(0);
    let normal_count = mesh
        .attribute(Mesh::ATTRIBUTE_NORMAL)
        .map(|a| a.len())
        .unwrap_or(0);
    let color_count = mesh
        .attribute(Mesh::ATTRIBUTE_COLOR)
        .map(|a| a.len())
        .unwrap_or(0);
    let uv_count = mesh
        .attribute(Mesh::ATTRIBUTE_UV_0)
        .map(|a| a.len())
        .unwrap_or(0);

    assert!(pos_count > 0, "Should have position data");
    assert_eq!(
        pos_count, normal_count,
        "Position and normal counts must match"
    );
    assert_eq!(
        pos_count, color_count,
        "Position and color counts must match"
    );
    assert_eq!(pos_count, uv_count, "Position and UV counts must match");
}

/// Verify greedy meshing produces fewer vertices than naive for a
/// realistic terrain chunk.
#[test]
fn greedy_more_efficient_than_naive_for_terrain() {
    use procedural_worlds::world::meshing::build_chunk_mesh_naive;

    let config = TerrainConfig::default();
    let mut chunk = Chunk::new(IVec3::new(0, 2, 0));
    generate_full(&mut chunk, &config);

    let greedy = build_chunk_mesh(&chunk);
    let naive = build_chunk_mesh_naive(&chunk);

    let greedy_verts = mesh_vertex_count(&greedy);
    let naive_verts = mesh_vertex_count(&naive);

    assert!(
        greedy_verts <= naive_verts,
        "Greedy ({}) should produce <= naive ({}) vertices for terrain",
        greedy_verts,
        naive_verts
    );

    // For realistic terrain, greedy should be significantly better
    if naive_verts > 100 {
        assert!(
            greedy_verts < naive_verts * 9 / 10,
            "Greedy ({}) should be at least 10% better than naive ({}) for terrain",
            greedy_verts,
            naive_verts
        );
    }
}
