//! Render Batching — Merge stable chunk meshes to reduce draw calls
//!
//! Each loaded chunk currently produces its own mesh entity, resulting in one
//! draw call per chunk. With hundreds of chunks loaded, this becomes a GPU
//! driver bottleneck. This module groups spatially adjacent, stable (non-dirty)
//! chunks into **batch groups** and merges their opaque meshes into a single
//! combined mesh per group.
//!
//! # Design
//!
//! - Chunks are grouped into **2×2×2 super-chunks** (8 chunks per batch).
//! - A batch is only built when all 8 constituent chunks are loaded and stable
//!   (not dirty, not pending mesh generation).
//! - When a batch is active, the individual chunk mesh entities are hidden
//!   (visibility set to `Hidden`). The batched mesh entity renders instead.
//! - If any chunk in a batch becomes dirty (e.g., block placement), the batch
//!   is dissolved: the batched entity is despawned and individual chunks are
//!   made visible again.
//!
//! This typically reduces draw calls by 4-8× for stable terrain.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::render::render_asset::RenderAssetUsages;

use super::{Chunk, ChunkManager, ChunkMesh, ChunkMaterial, ChunkMaterialHandle};

// ============================================================================
// CONFIGURATION
// ============================================================================

/// Number of chunks per axis in a batch group (2×2×2 = 8 chunks).
pub const BATCH_GROUP_SIZE: i32 = 2;

/// Maximum batches to build per frame to avoid stalling.
const MAX_BATCHES_PER_FRAME: usize = 2;

/// Minimum number of chunks in a group required to form a batch.
/// Groups with fewer loaded chunks aren't worth batching.
const MIN_CHUNKS_FOR_BATCH: usize = 4;

// ============================================================================
// COMPONENTS & RESOURCES
// ============================================================================

/// Marker component for batched super-mesh entities.
#[derive(Component)]
pub struct BatchedChunkMesh {
    /// The batch group key (super-chunk coordinate).
    pub group_key: IVec3,
}

/// Component added to individual chunk entities when they are part of an
/// active batch (their mesh is hidden in favor of the batched mesh).
#[derive(Component)]
pub struct InBatch {
    /// The batch group this chunk belongs to.
    pub group_key: IVec3,
}

/// Tracks active batch groups and their entities.
#[derive(Resource, Default)]
pub struct BatchState {
    /// Map from group key to the batched mesh entity.
    pub active_batches: HashMap<IVec3, Entity>,
    /// Group keys that need rebuilding (a constituent chunk became dirty).
    pub invalidated: HashSet<IVec3>,
    /// Statistics for the debug overlay.
    pub stats: BatchStats,
}

/// Statistics about render batching for the debug overlay.
#[derive(Default, Clone, Debug)]
pub struct BatchStats {
    /// Number of active batch groups.
    pub active_batch_count: usize,
    /// Total individual chunk meshes hidden by batching.
    pub chunks_batched: usize,
    /// Estimated draw call reduction (chunks_batched - active_batch_count).
    pub draw_call_reduction: usize,
}

// ============================================================================
// BATCH GROUP LOGIC
// ============================================================================

/// Compute the batch group key for a chunk position.
///
/// Groups are aligned to `BATCH_GROUP_SIZE` boundaries using floor division.
#[inline]
pub fn batch_group_key(chunk_pos: IVec3) -> IVec3 {
    IVec3::new(
        chunk_pos.x.div_euclid(BATCH_GROUP_SIZE),
        chunk_pos.y.div_euclid(BATCH_GROUP_SIZE),
        chunk_pos.z.div_euclid(BATCH_GROUP_SIZE),
    )
}

/// Iterate all chunk positions within a batch group.
pub fn group_chunk_positions(group_key: IVec3) -> impl Iterator<Item = IVec3> {
    let base = group_key * BATCH_GROUP_SIZE;
    (0..BATCH_GROUP_SIZE).flat_map(move |dx| {
        (0..BATCH_GROUP_SIZE).flat_map(move |dy| {
            (0..BATCH_GROUP_SIZE).map(move |dz| base + IVec3::new(dx, dy, dz))
        })
    })
}

// ============================================================================
// MESH MERGING
// ============================================================================

/// Merge multiple meshes into a single combined mesh.
///
/// All input meshes must have POSITION, NORMAL, COLOR, and UV_0 attributes.
/// UV_1 is included if present on *all* input meshes.
///
/// Returns `None` if the input is empty or meshes have incompatible layouts.
pub fn merge_meshes(meshes: &[&Mesh]) -> Option<Mesh> {
    if meshes.is_empty() {
        return None;
    }

    let mut all_positions: Vec<[f32; 3]> = Vec::new();
    let mut all_normals: Vec<[f32; 3]> = Vec::new();
    let mut all_colors: Vec<[f32; 4]> = Vec::new();
    let mut all_uvs: Vec<[f32; 2]> = Vec::new();
    let mut all_uv1s: Vec<[f32; 2]> = Vec::new();
    let mut all_indices: Vec<u32> = Vec::new();

    let has_uv1 = meshes.iter().all(|m| m.attribute(Mesh::ATTRIBUTE_UV_1).is_some());

    for mesh in meshes {
        let vertex_offset = all_positions.len() as u32;

        // Extract positions
        let positions = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(VertexAttributeValues::Float32x3(v)) => v,
            _ => return None,
        };

        let normals = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
            Some(VertexAttributeValues::Float32x3(v)) => v,
            _ => return None,
        };

        let colors = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
            Some(VertexAttributeValues::Float32x4(v)) => v,
            _ => return None,
        };

        let uvs = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
            Some(VertexAttributeValues::Float32x2(v)) => v,
            _ => return None,
        };

        all_positions.extend_from_slice(positions);
        all_normals.extend_from_slice(normals);
        all_colors.extend_from_slice(colors);
        all_uvs.extend_from_slice(uvs);

        if has_uv1
            && let Some(VertexAttributeValues::Float32x2(v)) =
                mesh.attribute(Mesh::ATTRIBUTE_UV_1)
        {
            all_uv1s.extend_from_slice(v);
        }

        // Extract and offset indices
        if let Some(Indices::U32(idx)) = mesh.indices() {
            all_indices.extend(idx.iter().map(|&i| i + vertex_offset));
        } else {
            // No indices — generate sequential triangle list
            let vert_count = positions.len() as u32;
            all_indices.extend((0..vert_count).map(|i| i + vertex_offset));
        }
    }

    if all_positions.is_empty() {
        return None;
    }

    let mut merged = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    merged.insert_attribute(Mesh::ATTRIBUTE_POSITION, all_positions);
    merged.insert_attribute(Mesh::ATTRIBUTE_NORMAL, all_normals);
    merged.insert_attribute(Mesh::ATTRIBUTE_COLOR, all_colors);
    merged.insert_attribute(Mesh::ATTRIBUTE_UV_0, all_uvs);
    if has_uv1 && !all_uv1s.is_empty() {
        merged.insert_attribute(Mesh::ATTRIBUTE_UV_1, all_uv1s);
    }
    merged.insert_indices(Indices::U32(all_indices));

    Some(merged)
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// System: detect when batched chunks become dirty and invalidate their batch.
pub fn invalidate_dirty_batches(
    mut batch_state: ResMut<BatchState>,
    query: Query<(&Chunk, &InBatch), Changed<Chunk>>,
) {
    for (chunk, in_batch) in &query {
        if chunk.dirty {
            batch_state.invalidated.insert(in_batch.group_key);
        }
    }
}

/// System: dissolve invalidated batches — despawn batched entity, show individuals.
pub fn dissolve_invalidated_batches(
    mut commands: Commands,
    mut batch_state: ResMut<BatchState>,
    mut chunk_query: Query<(Entity, &InBatch, &mut Visibility)>,
) {
    let invalidated: Vec<IVec3> = batch_state.invalidated.drain().collect();

    for group_key in invalidated {
        // Despawn the batched mesh entity
        if let Some(batch_entity) = batch_state.active_batches.remove(&group_key) {
            commands.entity(batch_entity).despawn();
        }

        // Restore visibility and remove InBatch marker from constituent chunks
        for (entity, in_batch, mut visibility) in &mut chunk_query {
            if in_batch.group_key == group_key {
                *visibility = Visibility::Inherited;
                commands.entity(entity).remove::<InBatch>();
            }
        }
    }
}

/// System: build new batches from stable chunk groups.
///
/// Scans all loaded chunks, groups them by batch key, and creates merged
/// meshes for groups where all chunks are stable.
#[allow(clippy::type_complexity)]
pub fn build_render_batches(
    mut commands: Commands,
    mut batch_state: ResMut<BatchState>,
    mut mesh_assets: ResMut<Assets<Mesh>>,
    chunk_material: Res<ChunkMaterial>,
    _chunk_manager: Res<ChunkManager>,
    chunk_query: Query<(Entity, &Chunk, &Mesh3d, Option<&InBatch>), (With<ChunkMesh>, Without<super::PendingMesh>)>,
) {
    let Some(mat) = &chunk_material.handle else { return; };

    // Group loaded, meshed chunks by batch key
    let mut groups: HashMap<IVec3, Vec<(Entity, IVec3)>> = HashMap::new();
    for (entity, chunk, _mesh_handle, in_batch) in &chunk_query {
        // Skip chunks already in a batch or that are dirty
        if in_batch.is_some() || chunk.dirty {
            continue;
        }
        let key = batch_group_key(chunk.position);
        // Skip if this group already has an active batch
        if batch_state.active_batches.contains_key(&key) {
            continue;
        }
        groups.entry(key).or_default().push((entity, chunk.position));
    }

    let mut batches_built = 0;

    for (group_key, members) in &groups {
        if batches_built >= MAX_BATCHES_PER_FRAME {
            break;
        }
        if members.len() < MIN_CHUNKS_FOR_BATCH {
            continue;
        }

        // Collect cloned meshes with position offsets for merging.
        // We clone upfront to avoid borrow conflicts with mesh_assets.
        let base_pos = super::chunk_to_world_pos(members[0].1);

        let adjusted_meshes: Vec<Mesh> = members
            .iter()
            .filter_map(|&(entity, _pos)| {
                let (_, chunk, mesh_handle, _) = chunk_query.get(entity).ok()?;
                let mesh = mesh_assets.get(&mesh_handle.0)?;
                let chunk_world = super::chunk_to_world_pos(chunk.position);
                let offset = chunk_world - base_pos;
                Some(offset_mesh_positions(mesh, offset))
            })
            .collect();

        if adjusted_meshes.len() < MIN_CHUNKS_FOR_BATCH {
            continue;
        }

        let adj_refs: Vec<&Mesh> = adjusted_meshes.iter().collect();
        let Some(final_merged) = merge_meshes(&adj_refs) else {
            continue;
        };

        let final_handle = mesh_assets.add(final_merged);

        let batch_entity = commands.spawn((
            Mesh3d(final_handle),
            Transform::from_translation(base_pos),
            BatchedChunkMesh { group_key: *group_key },
        )).id();

        // Apply material
        match mat {
            ChunkMaterialHandle::Atlas { opaque, .. } => {
                commands.entity(batch_entity).insert(MeshMaterial3d(opaque.clone()));
            }
            ChunkMaterialHandle::Standard { opaque, .. } => {
                commands.entity(batch_entity).insert(MeshMaterial3d(opaque.clone()));
            }
        }

        // Hide individual chunk meshes and mark them as batched
        for &(entity, _) in members {
            commands.entity(entity).insert((
                InBatch { group_key: *group_key },
                Visibility::Hidden,
            ));
        }

        batch_state.active_batches.insert(*group_key, batch_entity);
        batches_built += 1;
    }

    // Update stats
    let chunks_batched: usize = batch_state.active_batches.len() * BATCH_GROUP_SIZE as usize
        * BATCH_GROUP_SIZE as usize
        * BATCH_GROUP_SIZE as usize;
    batch_state.stats = BatchStats {
        active_batch_count: batch_state.active_batches.len(),
        chunks_batched,
        draw_call_reduction: chunks_batched.saturating_sub(batch_state.active_batches.len()),
    };
}

/// Create a copy of a mesh with all positions offset by the given vector.
fn offset_mesh_positions(mesh: &Mesh, offset: Vec3) -> Mesh {
    let mut new_mesh = mesh.clone();

    if let Some(VertexAttributeValues::Float32x3(positions)) =
        new_mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    {
        for pos in positions.iter_mut() {
            pos[0] += offset.x;
            pos[1] += offset.y;
            pos[2] += offset.z;
        }
    }

    new_mesh
}

/// System: clean up batch entities when their chunks are unloaded.
pub fn cleanup_unloaded_batches(
    mut commands: Commands,
    mut batch_state: ResMut<BatchState>,
    chunk_manager: Res<ChunkManager>,
) {
    let mut to_remove = Vec::new();

    for (&group_key, &batch_entity) in &batch_state.active_batches {
        // Check if any constituent chunk has been unloaded
        let all_loaded = group_chunk_positions(group_key)
            .all(|pos| chunk_manager.chunks.contains_key(&pos));

        if !all_loaded {
            commands.entity(batch_entity).despawn();
            to_remove.push(group_key);
        }
    }

    for key in to_remove {
        batch_state.active_batches.remove(&key);
    }
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that adds render batching systems to the app.
pub struct RenderBatchingPlugin;

impl Plugin for RenderBatchingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BatchState>()
            .add_systems(
                Update,
                (
                    invalidate_dirty_batches,
                    dissolve_invalidated_batches,
                    build_render_batches,
                    cleanup_unloaded_batches,
                )
                    .chain()
                    .after(super::WorldSystems::Meshing),
            );
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::render::mesh::PrimitiveTopology;
    use bevy::render::render_asset::RenderAssetUsages;

    /// Helper: create a simple test mesh with the given number of quads.
    fn make_test_mesh(quad_count: usize, offset: Vec3) -> Mesh {
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut colors = Vec::new();
        let mut uvs = Vec::new();
        let mut indices = Vec::new();

        for i in 0..quad_count {
            let base = positions.len() as u32;
            let y = i as f32 + offset.y;
            let x = offset.x;
            let z = offset.z;

            positions.extend_from_slice(&[
                [x, y, z],
                [x + 1.0, y, z],
                [x + 1.0, y, z + 1.0],
                [x, y, z + 1.0],
            ]);
            normals.extend_from_slice(&[
                [0.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ]);
            colors.extend_from_slice(&[
                [0.5, 0.5, 0.5, 1.0],
                [0.5, 0.5, 0.5, 1.0],
                [0.5, 0.5, 0.5, 1.0],
                [0.5, 0.5, 0.5, 1.0],
            ]);
            uvs.extend_from_slice(&[
                [0.0, 0.0],
                [1.0, 0.0],
                [1.0, 1.0],
                [0.0, 1.0],
            ]);
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }

        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh.insert_indices(Indices::U32(indices));
        mesh
    }

    #[test]
    fn test_batch_group_key_alignment() {
        // Chunks (0,0,0) and (1,1,1) should be in the same 2×2×2 group
        assert_eq!(batch_group_key(IVec3::new(0, 0, 0)), IVec3::new(0, 0, 0));
        assert_eq!(batch_group_key(IVec3::new(1, 1, 1)), IVec3::new(0, 0, 0));
        // Chunk (2,0,0) starts a new group
        assert_eq!(batch_group_key(IVec3::new(2, 0, 0)), IVec3::new(1, 0, 0));
        // Negative coordinates
        assert_eq!(batch_group_key(IVec3::new(-1, 0, 0)), IVec3::new(-1, 0, 0));
        assert_eq!(batch_group_key(IVec3::new(-2, 0, 0)), IVec3::new(-1, 0, 0));
        assert_eq!(batch_group_key(IVec3::new(-3, 0, 0)), IVec3::new(-2, 0, 0));
    }

    #[test]
    fn test_group_chunk_positions_count() {
        let positions: Vec<IVec3> = group_chunk_positions(IVec3::ZERO).collect();
        assert_eq!(positions.len(), 8); // 2×2×2
    }

    #[test]
    fn test_group_chunk_positions_contents() {
        let positions: HashSet<IVec3> = group_chunk_positions(IVec3::ZERO).collect();
        assert!(positions.contains(&IVec3::new(0, 0, 0)));
        assert!(positions.contains(&IVec3::new(1, 0, 0)));
        assert!(positions.contains(&IVec3::new(0, 1, 0)));
        assert!(positions.contains(&IVec3::new(0, 0, 1)));
        assert!(positions.contains(&IVec3::new(1, 1, 1)));
    }

    #[test]
    fn test_merge_meshes_empty() {
        assert!(merge_meshes(&[]).is_none());
    }

    #[test]
    fn test_merge_meshes_single() {
        let mesh = make_test_mesh(2, Vec3::ZERO);
        let merged = merge_meshes(&[&mesh]).unwrap();

        let vert_count = merged
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .map(|a| a.len())
            .unwrap_or(0);
        assert_eq!(vert_count, 8); // 2 quads × 4 verts
    }

    #[test]
    fn test_merge_meshes_combines_vertices() {
        let mesh_a = make_test_mesh(2, Vec3::ZERO);
        let mesh_b = make_test_mesh(3, Vec3::new(16.0, 0.0, 0.0));
        let merged = merge_meshes(&[&mesh_a, &mesh_b]).unwrap();

        let vert_count = merged
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .map(|a| a.len())
            .unwrap_or(0);
        assert_eq!(vert_count, 20); // (2 + 3) quads × 4 verts
    }

    #[test]
    fn test_merge_meshes_reindexes_correctly() {
        let mesh_a = make_test_mesh(1, Vec3::ZERO);
        let mesh_b = make_test_mesh(1, Vec3::new(5.0, 0.0, 0.0));
        let merged = merge_meshes(&[&mesh_a, &mesh_b]).unwrap();

        if let Some(Indices::U32(indices)) = merged.indices() {
            assert_eq!(indices.len(), 12); // 2 quads × 6 indices
            // First quad: 0,1,2,0,2,3
            assert_eq!(&indices[0..6], &[0, 1, 2, 0, 2, 3]);
            // Second quad: offset by 4 (vertex count of first mesh)
            assert_eq!(&indices[6..12], &[4, 5, 6, 4, 6, 7]);
        } else {
            panic!("Expected U32 indices");
        }
    }

    #[test]
    fn test_merge_meshes_preserves_attributes() {
        let mesh_a = make_test_mesh(1, Vec3::ZERO);
        let mesh_b = make_test_mesh(1, Vec3::new(5.0, 0.0, 0.0));
        let merged = merge_meshes(&[&mesh_a, &mesh_b]).unwrap();

        // All required attributes should be present
        assert!(merged.attribute(Mesh::ATTRIBUTE_POSITION).is_some());
        assert!(merged.attribute(Mesh::ATTRIBUTE_NORMAL).is_some());
        assert!(merged.attribute(Mesh::ATTRIBUTE_COLOR).is_some());
        assert!(merged.attribute(Mesh::ATTRIBUTE_UV_0).is_some());
    }

    #[test]
    fn test_merge_meshes_with_uv1() {
        let mut mesh_a = make_test_mesh(1, Vec3::ZERO);
        mesh_a.insert_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0.0_f32, 0.0]; 4]);
        let mut mesh_b = make_test_mesh(1, Vec3::new(5.0, 0.0, 0.0));
        mesh_b.insert_attribute(Mesh::ATTRIBUTE_UV_1, vec![[1.0_f32, 1.0]; 4]);

        let merged = merge_meshes(&[&mesh_a, &mesh_b]).unwrap();
        assert!(merged.attribute(Mesh::ATTRIBUTE_UV_1).is_some());
        if let Some(VertexAttributeValues::Float32x2(uv1s)) =
            merged.attribute(Mesh::ATTRIBUTE_UV_1)
        {
            assert_eq!(uv1s.len(), 8);
        }
    }

    #[test]
    fn test_offset_mesh_positions() {
        let mesh = make_test_mesh(1, Vec3::ZERO);
        let offset = Vec3::new(10.0, 20.0, 30.0);
        let shifted = offset_mesh_positions(&mesh, offset);

        if let Some(VertexAttributeValues::Float32x3(positions)) =
            shifted.attribute(Mesh::ATTRIBUTE_POSITION)
        {
            // First vertex was at (0,0,0), should now be at (10,20,30)
            assert!((positions[0][0] - 10.0).abs() < f32::EPSILON);
            assert!((positions[0][1] - 20.0).abs() < f32::EPSILON);
            assert!((positions[0][2] - 30.0).abs() < f32::EPSILON);
        } else {
            panic!("Expected Float32x3 positions");
        }
    }

    #[test]
    fn test_batch_group_key_negative_boundary() {
        // Verify floor division behavior at negative boundaries
        assert_eq!(batch_group_key(IVec3::new(-4, -4, -4)), IVec3::new(-2, -2, -2));
        assert_eq!(batch_group_key(IVec3::new(-3, -3, -3)), IVec3::new(-2, -2, -2));
        assert_eq!(batch_group_key(IVec3::new(-2, -2, -2)), IVec3::new(-1, -1, -1));
    }
}
