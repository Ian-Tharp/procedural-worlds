//! World systems - chunks, blocks, voxel data structures
//!
//! This module contains:
//! - Chunk data structure (16x16x16 blocks)
//! - Block type registry
//! - World coordinate system
//! - Chunk loading/unloading
//! - Mesh generation
//! - Block queries for collision

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::generation::{generate_caves, generate_chunk_terrain, TerrainConfig};

pub mod meshing;
pub mod persistence;

/// Size of a chunk in blocks (16x16x16)
pub const CHUNK_SIZE: usize = 16;

/// Total blocks per chunk
pub const CHUNK_VOLUME: usize = CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE;

/// Block type identifier
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug, Serialize, Deserialize)]
#[serde(into = "u16", from = "u16")]
#[repr(u16)]
#[allow(dead_code)] // Future block types
pub enum BlockType {
    #[default]
    Air = 0,
    Stone = 1,
    Dirt = 2,
    Grass = 3,
    Sand = 4,
    Water = 5,
    Wood = 6,
    Leaves = 7,
}

impl From<BlockType> for u16 {
    fn from(block: BlockType) -> u16 {
        block as u16
    }
}

impl From<u16> for BlockType {
    fn from(val: u16) -> BlockType {
        match val {
            0 => BlockType::Air,
            1 => BlockType::Stone,
            2 => BlockType::Dirt,
            3 => BlockType::Grass,
            4 => BlockType::Sand,
            5 => BlockType::Water,
            6 => BlockType::Wood,
            7 => BlockType::Leaves,
            _ => BlockType::Air, // Unknown block types default to Air
        }
    }
}

impl BlockType {
    /// Returns true if this block type is transparent (for face culling)
    pub fn is_transparent(&self) -> bool {
        matches!(self, BlockType::Air | BlockType::Water)
    }

    /// Returns true if this block is solid (for collision)
    pub fn is_solid(&self) -> bool {
        !matches!(self, BlockType::Air | BlockType::Water)
    }
}

/// A chunk of voxel data
#[derive(Component, Debug)]
pub struct Chunk {
    /// Block data stored in a flat array [x + y * SIZE + z * SIZE * SIZE]
    blocks: [BlockType; CHUNK_VOLUME],
    /// Chunk position in chunk coordinates
    pub position: IVec3,
    /// Whether this chunk needs its mesh rebuilt
    pub dirty: bool,
}

impl Chunk {
    /// Create a new empty chunk at the given position
    pub fn new(position: IVec3) -> Self {
        Self {
            blocks: [BlockType::Air; CHUNK_VOLUME],
            position,
            dirty: true,
        }
    }

    /// Create a chunk with pre-populated block data
    ///
    /// Used by the persistence system to reconstruct chunks from saved data.
    /// The chunk is marked dirty so its mesh will be rebuilt.
    pub fn from_blocks(position: IVec3, blocks: [BlockType; CHUNK_VOLUME]) -> Self {
        Self {
            blocks,
            position,
            dirty: true,
        }
    }

    /// Get a reference to the raw block data array
    pub fn blocks(&self) -> &[BlockType; CHUNK_VOLUME] {
        &self.blocks
    }

    /// Convert local (x, y, z) coordinates to flat array index
    #[inline]
    fn index(x: usize, y: usize, z: usize) -> usize {
        x + y * CHUNK_SIZE + z * CHUNK_SIZE * CHUNK_SIZE
    }

    /// Get block at local coordinates
    pub fn get_block(&self, x: usize, y: usize, z: usize) -> BlockType {
        if x < CHUNK_SIZE && y < CHUNK_SIZE && z < CHUNK_SIZE {
            self.blocks[Self::index(x, y, z)]
        } else {
            BlockType::Air
        }
    }

    /// Set block at local coordinates
    pub fn set_block(&mut self, x: usize, y: usize, z: usize, block: BlockType) {
        if x < CHUNK_SIZE && y < CHUNK_SIZE && z < CHUNK_SIZE {
            self.blocks[Self::index(x, y, z)] = block;
            self.dirty = true;
        }
    }

    /// Fill entire chunk with a single block type (used in tests)
    #[allow(dead_code)]
    pub fn fill(&mut self, block: BlockType) {
        self.blocks.fill(block);
        self.dirty = true;
    }

    /// Get chunk position in world coordinates (block units)
    pub fn world_position(&self) -> IVec3 {
        self.position * CHUNK_SIZE as i32
    }
}

/// Marker component for chunk mesh entities
#[derive(Component)]
pub struct ChunkMesh;

/// Resource for tracking loaded chunks
#[derive(Resource)]
pub struct ChunkManager {
    /// Map of chunk positions to their entities
    pub chunks: HashMap<IVec3, Entity>,
    /// Render distance in chunks
    pub render_distance: i32,
    /// Player's current chunk position
    pub player_chunk: IVec3,
    /// Chunks generated this frame (for rate limiting)
    chunks_generated_this_frame: u32,
    /// Maximum chunks to generate per frame
    pub max_chunks_per_frame: u32,
}

impl Default for ChunkManager {
    fn default() -> Self {
        Self {
            chunks: HashMap::new(),
            render_distance: 4,
            player_chunk: IVec3::ZERO,
            chunks_generated_this_frame: 0,
            // Increased from 2 to 4 for faster initial load
            // Trade-off: slightly more per-frame work, but faster to playable state
            max_chunks_per_frame: 4,
        }
    }
}

impl ChunkManager {
    #[allow(dead_code)]
    pub fn new(render_distance: i32) -> Self {
        Self {
            render_distance,
            ..default()
        }
    }
}

/// Convert world position to chunk position
pub fn world_to_chunk_pos(world_pos: Vec3) -> IVec3 {
    IVec3::new(
        (world_pos.x / CHUNK_SIZE as f32).floor() as i32,
        (world_pos.y / CHUNK_SIZE as f32).floor() as i32,
        (world_pos.z / CHUNK_SIZE as f32).floor() as i32,
    )
}

/// Convert chunk position to world position (corner of chunk)
pub fn chunk_to_world_pos(chunk_pos: IVec3) -> Vec3 {
    Vec3::new(
        (chunk_pos.x * CHUNK_SIZE as i32) as f32,
        (chunk_pos.y * CHUNK_SIZE as i32) as f32,
        (chunk_pos.z * CHUNK_SIZE as i32) as f32,
    )
}

/// System sets for ordering world systems
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum WorldSystems {
    ChunkLoading,
    Meshing,
    Cleanup,
}

/// Plugin for world management
pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChunkManager>()
            .init_resource::<TerrainConfig>()
            .init_resource::<ChunkMaterial>()
            .configure_sets(
                Update,
                (
                    WorldSystems::ChunkLoading,
                    WorldSystems::Meshing,
                    WorldSystems::Cleanup,
                )
                    .chain(),
            )
            .add_systems(Startup, setup_chunk_material)
            .add_systems(
                Update,
                (update_player_chunk_position, chunk_streaming_system)
                    .chain()
                    .in_set(WorldSystems::ChunkLoading),
            )
            .add_systems(Update, mesh_dirty_chunks.in_set(WorldSystems::Meshing))
            .add_systems(Update, despawn_far_chunks.in_set(WorldSystems::Cleanup));
    }
}

/// Resource holding the shared material for chunk meshes
#[derive(Resource, Default)]
pub struct ChunkMaterial {
    pub handle: Option<Handle<StandardMaterial>>,
}

/// Setup the shared material for all chunk meshes
fn setup_chunk_material(
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut chunk_material: ResMut<ChunkMaterial>,
) {
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE, // Vertex colors will modulate this
        perceptual_roughness: 0.9,
        metallic: 0.0,
        ..default()
    });
    chunk_material.handle = Some(material);
    info!("Chunk material initialized");
}

/// Update the player's current chunk position based on camera
fn update_player_chunk_position(
    camera_query: Query<&Transform, With<Camera3d>>,
    mut chunk_manager: ResMut<ChunkManager>,
) {
    if let Ok(transform) = camera_query.get_single() {
        let new_chunk = world_to_chunk_pos(transform.translation);
        if new_chunk != chunk_manager.player_chunk {
            chunk_manager.player_chunk = new_chunk;
        }
    }
}

/// Load/unload chunks based on player position
fn chunk_streaming_system(
    mut commands: Commands,
    mut chunk_manager: ResMut<ChunkManager>,
    terrain_config: Res<TerrainConfig>,
) {
    // Reset frame counter
    chunk_manager.chunks_generated_this_frame = 0;

    let center = chunk_manager.player_chunk;
    let rd = chunk_manager.render_distance;
    let max_per_frame = chunk_manager.max_chunks_per_frame;

    // Iterate through chunks that should be loaded
    // Priority: closest chunks first (spiral out from center)
    for dist in 0..=rd {
        for x in (center.x - dist)..=(center.x + dist) {
            for z in (center.z - dist)..=(center.z + dist) {
                // Only process chunks at current distance ring
                let dx = (x - center.x).abs();
                let dz = (z - center.z).abs();
                if dx != dist && dz != dist {
                    continue;
                }

                // Vertical range: -2 to +4 chunks (covers underground and surface)
                for y in -2..=4 {
                    let chunk_pos = IVec3::new(x, y, z);

                    // Skip if already loaded
                    if chunk_manager.chunks.contains_key(&chunk_pos) {
                        continue;
                    }

                    // Rate limit chunk generation
                    if chunk_manager.chunks_generated_this_frame >= max_per_frame {
                        return;
                    }

                    // Generate the chunk
                    let mut chunk = Chunk::new(chunk_pos);
                    generate_chunk_terrain(&mut chunk, &terrain_config);
                    generate_caves(&mut chunk, &terrain_config);

                    // Spawn chunk entity (mesh will be built by meshing system)
                    let entity = commands.spawn(chunk).id();
                    chunk_manager.chunks.insert(chunk_pos, entity);
                    chunk_manager.chunks_generated_this_frame += 1;
                }
            }
        }
    }
}

/// Maximum meshes to build per frame (prevents GPU upload stutter)
const MAX_MESHES_PER_FRAME: usize = 6;

/// Build meshes for chunks that have dirty flag set
fn mesh_dirty_chunks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    chunk_material: Res<ChunkMaterial>,
    chunk_manager: Res<ChunkManager>,
    mut chunk_query: Query<(Entity, &mut Chunk), Without<ChunkMesh>>,
) {
    let Some(material_handle) = &chunk_material.handle else {
        return;
    };

    let player_chunk = chunk_manager.player_chunk;
    let mut meshes_built = 0;

    // Collect and sort chunks by distance to player (closest first)
    let mut dirty_chunks: Vec<_> = chunk_query
        .iter_mut()
        .filter(|(_, chunk)| chunk.dirty)
        .collect();

    dirty_chunks.sort_by_key(|(_, chunk)| {
        let diff = chunk.position - player_chunk;
        diff.x.abs() + diff.y.abs() + diff.z.abs() // Manhattan distance
    });

    for (entity, mut chunk) in dirty_chunks {
        if meshes_built >= MAX_MESHES_PER_FRAME {
            break;
        }

        // Build the mesh
        let mesh = meshing::build_chunk_mesh(&chunk);
        let mesh_handle = meshes.add(mesh);

        // Get world position for the chunk
        let world_pos = chunk_to_world_pos(chunk.position);

        // Add mesh components to the chunk entity
        commands.entity(entity).insert((
            Mesh3d(mesh_handle),
            MeshMaterial3d(material_handle.clone()),
            Transform::from_translation(world_pos),
            ChunkMesh,
        ));

        // Clear dirty flag
        chunk.dirty = false;
        meshes_built += 1;
    }
}

/// Remove chunks that are too far from the player
fn despawn_far_chunks(
    mut commands: Commands,
    mut chunk_manager: ResMut<ChunkManager>,
    chunk_query: Query<(Entity, &Chunk)>,
) {
    let center = chunk_manager.player_chunk;
    let max_dist = chunk_manager.render_distance + 2; // Buffer zone

    let mut to_remove = Vec::new();

    for (entity, chunk) in &chunk_query {
        let diff = chunk.position - center;
        let dist = diff.x.abs().max(diff.y.abs()).max(diff.z.abs());

        if dist > max_dist {
            commands.entity(entity).despawn_recursive();
            to_remove.push(chunk.position);
        }
    }

    // Remove from chunk manager
    for pos in to_remove {
        chunk_manager.chunks.remove(&pos);
    }
}

// ============================================================================
// BLOCK QUERIES - For collision and gameplay
// ============================================================================

/// Get the block at a world position
///
/// Returns `BlockType::Air` if the chunk is not loaded or position is invalid.
///
/// # Arguments
/// * `world_pos` - Block position in world coordinates (integer)
/// * `chunk_manager` - The chunk manager resource
/// * `chunks` - Query for chunk data
///
/// # Example
/// ```ignore
/// let block = get_block_at(IVec3::new(10, 32, 10), &chunk_manager, &chunks);
/// if block.is_solid() {
///     // Collision!
/// }
/// ```
#[allow(dead_code)]
pub fn get_block_at(
    world_pos: IVec3,
    chunk_manager: &ChunkManager,
    chunks: &Query<&Chunk>,
) -> BlockType {
    // Calculate which chunk contains this block
    let chunk_pos = IVec3::new(
        world_pos.x.div_euclid(CHUNK_SIZE as i32),
        world_pos.y.div_euclid(CHUNK_SIZE as i32),
        world_pos.z.div_euclid(CHUNK_SIZE as i32),
    );

    // Look up chunk entity
    let Some(&entity) = chunk_manager.chunks.get(&chunk_pos) else {
        return BlockType::Air; // Unloaded chunk = air
    };

    // Get chunk component
    let Ok(chunk) = chunks.get(entity) else {
        return BlockType::Air;
    };

    // Calculate local position within chunk
    let local_x = world_pos.x.rem_euclid(CHUNK_SIZE as i32) as usize;
    let local_y = world_pos.y.rem_euclid(CHUNK_SIZE as i32) as usize;
    let local_z = world_pos.z.rem_euclid(CHUNK_SIZE as i32) as usize;

    chunk.get_block(local_x, local_y, local_z)
}

/// Get the block at a world position (float coordinates)
///
/// Floors the coordinates to get the containing block.
#[allow(dead_code)]
pub fn get_block_at_f32(
    world_pos: Vec3,
    chunk_manager: &ChunkManager,
    chunks: &Query<&Chunk>,
) -> BlockType {
    let block_pos = IVec3::new(
        world_pos.x.floor() as i32,
        world_pos.y.floor() as i32,
        world_pos.z.floor() as i32,
    );
    get_block_at(block_pos, chunk_manager, chunks)
}

/// Check if a block position is solid (for collision)
#[allow(dead_code)]
pub fn is_solid_at(
    world_pos: IVec3,
    chunk_manager: &ChunkManager,
    chunks: &Query<&Chunk>,
) -> bool {
    get_block_at(world_pos, chunk_manager, chunks).is_solid()
}

/// Get all solid blocks in an axis-aligned bounding box
///
/// Returns positions of solid blocks that intersect the AABB.
/// Useful for collision detection.
#[allow(dead_code)]
pub fn get_solid_blocks_in_aabb(
    min: IVec3,
    max: IVec3,
    chunk_manager: &ChunkManager,
    chunks: &Query<&Chunk>,
) -> Vec<IVec3> {
    let mut solids = Vec::new();

    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                let pos = IVec3::new(x, y, z);
                if is_solid_at(pos, chunk_manager, chunks) {
                    solids.push(pos);
                }
            }
        }
    }

    solids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_size_constants() {
        assert_eq!(CHUNK_SIZE, 16);
        assert_eq!(CHUNK_VOLUME, 16 * 16 * 16);
        assert_eq!(CHUNK_VOLUME, 4096);
    }

    #[test]
    fn test_block_type_transparency() {
        assert!(BlockType::Air.is_transparent());
        assert!(BlockType::Water.is_transparent());
        assert!(!BlockType::Stone.is_transparent());
        assert!(!BlockType::Dirt.is_transparent());
        assert!(!BlockType::Grass.is_transparent());
    }

    #[test]
    fn test_block_type_solidity() {
        assert!(!BlockType::Air.is_solid());
        assert!(!BlockType::Water.is_solid());
        assert!(BlockType::Stone.is_solid());
        assert!(BlockType::Dirt.is_solid());
        assert!(BlockType::Grass.is_solid());
    }

    #[test]
    fn test_chunk_new() {
        let chunk = Chunk::new(IVec3::new(1, 2, 3));
        assert_eq!(chunk.position, IVec3::new(1, 2, 3));
        assert!(chunk.dirty);

        // All blocks should be air
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(chunk.get_block(x, y, z), BlockType::Air);
                }
            }
        }
    }

    #[test]
    fn test_chunk_get_set_block() {
        let mut chunk = Chunk::new(IVec3::ZERO);

        // Set and get a block
        chunk.set_block(5, 10, 15, BlockType::Stone);
        assert_eq!(chunk.get_block(5, 10, 15), BlockType::Stone);

        // Other blocks should still be air
        assert_eq!(chunk.get_block(0, 0, 0), BlockType::Air);
        assert_eq!(chunk.get_block(5, 10, 14), BlockType::Air);
    }

    #[test]
    fn test_chunk_out_of_bounds_returns_air() {
        let chunk = Chunk::new(IVec3::ZERO);

        // Out of bounds should return Air without panic
        assert_eq!(chunk.get_block(16, 0, 0), BlockType::Air);
        assert_eq!(chunk.get_block(0, 100, 0), BlockType::Air);
        assert_eq!(chunk.get_block(0, 0, 999), BlockType::Air);
    }

    #[test]
    fn test_chunk_fill() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.fill(BlockType::Stone);

        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(chunk.get_block(x, y, z), BlockType::Stone);
                }
            }
        }
    }

    #[test]
    fn test_chunk_world_position() {
        let chunk = Chunk::new(IVec3::new(0, 0, 0));
        assert_eq!(chunk.world_position(), IVec3::new(0, 0, 0));

        let chunk = Chunk::new(IVec3::new(1, 2, 3));
        assert_eq!(chunk.world_position(), IVec3::new(16, 32, 48));

        let chunk = Chunk::new(IVec3::new(-1, -1, -1));
        assert_eq!(chunk.world_position(), IVec3::new(-16, -16, -16));
    }

    #[test]
    fn test_world_to_chunk_pos() {
        // Origin
        assert_eq!(world_to_chunk_pos(Vec3::new(0.0, 0.0, 0.0)), IVec3::ZERO);

        // Within first chunk
        assert_eq!(world_to_chunk_pos(Vec3::new(8.0, 8.0, 8.0)), IVec3::ZERO);
        assert_eq!(world_to_chunk_pos(Vec3::new(15.9, 15.9, 15.9)), IVec3::ZERO);

        // Crossing into next chunk
        assert_eq!(world_to_chunk_pos(Vec3::new(16.0, 16.0, 16.0)), IVec3::ONE);
        assert_eq!(world_to_chunk_pos(Vec3::new(32.0, 48.0, 64.0)), IVec3::new(2, 3, 4));

        // Negative coordinates
        assert_eq!(world_to_chunk_pos(Vec3::new(-1.0, 0.0, 0.0)), IVec3::new(-1, 0, 0));
        assert_eq!(world_to_chunk_pos(Vec3::new(-0.1, 0.0, 0.0)), IVec3::new(-1, 0, 0));
        assert_eq!(world_to_chunk_pos(Vec3::new(-16.0, -16.0, -16.0)), IVec3::new(-1, -1, -1));
        assert_eq!(world_to_chunk_pos(Vec3::new(-17.0, -17.0, -17.0)), IVec3::new(-2, -2, -2));
    }

    #[test]
    fn test_chunk_to_world_pos() {
        assert_eq!(chunk_to_world_pos(IVec3::ZERO), Vec3::ZERO);
        assert_eq!(chunk_to_world_pos(IVec3::ONE), Vec3::new(16.0, 16.0, 16.0));
        assert_eq!(chunk_to_world_pos(IVec3::new(2, 3, 4)), Vec3::new(32.0, 48.0, 64.0));
        assert_eq!(chunk_to_world_pos(IVec3::new(-1, -1, -1)), Vec3::new(-16.0, -16.0, -16.0));
    }

    #[test]
    fn test_coordinate_round_trip() {
        // World -> Chunk -> World should give chunk corner
        let world_pos = Vec3::new(35.7, 22.3, 50.1);
        let chunk_pos = world_to_chunk_pos(world_pos);
        let chunk_corner = chunk_to_world_pos(chunk_pos);

        assert_eq!(chunk_pos, IVec3::new(2, 1, 3));
        assert_eq!(chunk_corner, Vec3::new(32.0, 16.0, 48.0));

        // The corner should be <= the original world pos
        assert!(chunk_corner.x <= world_pos.x);
        assert!(chunk_corner.y <= world_pos.y);
        assert!(chunk_corner.z <= world_pos.z);
    }

    #[test]
    fn test_chunk_index_calculation() {
        // Test internal index calculation
        assert_eq!(Chunk::index(0, 0, 0), 0);
        assert_eq!(Chunk::index(1, 0, 0), 1);
        assert_eq!(Chunk::index(0, 1, 0), CHUNK_SIZE);
        assert_eq!(Chunk::index(0, 0, 1), CHUNK_SIZE * CHUNK_SIZE);
        assert_eq!(Chunk::index(15, 15, 15), CHUNK_VOLUME - 1);
    }

    #[test]
    fn test_chunk_dirty_flag() {
        let mut chunk = Chunk::new(IVec3::ZERO);

        // New chunks start dirty
        assert!(chunk.dirty);

        // Clear dirty manually
        chunk.dirty = false;
        assert!(!chunk.dirty);

        // Setting a block makes it dirty again
        chunk.set_block(0, 0, 0, BlockType::Stone);
        assert!(chunk.dirty);

        // Fill also makes it dirty
        chunk.dirty = false;
        chunk.fill(BlockType::Air);
        assert!(chunk.dirty);
    }

    // Block query tests (require ECS world, so kept simple)
    #[test]
    fn test_chunk_pos_calculation() {
        // Positive coordinates
        let chunk_pos = IVec3::new(
            35_i32.div_euclid(CHUNK_SIZE as i32),
            22_i32.div_euclid(CHUNK_SIZE as i32),
            50_i32.div_euclid(CHUNK_SIZE as i32),
        );
        assert_eq!(chunk_pos, IVec3::new(2, 1, 3));

        // Negative coordinates
        let chunk_pos = IVec3::new(
            (-5_i32).div_euclid(CHUNK_SIZE as i32),
            (-20_i32).div_euclid(CHUNK_SIZE as i32),
            (-1_i32).div_euclid(CHUNK_SIZE as i32),
        );
        assert_eq!(chunk_pos, IVec3::new(-1, -2, -1));
    }

    #[test]
    fn test_local_pos_calculation() {
        // Positive world position
        let local_x = 35_i32.rem_euclid(CHUNK_SIZE as i32) as usize;
        let local_y = 22_i32.rem_euclid(CHUNK_SIZE as i32) as usize;
        let local_z = 50_i32.rem_euclid(CHUNK_SIZE as i32) as usize;
        assert_eq!((local_x, local_y, local_z), (3, 6, 2));

        // Negative world position
        let local_x = (-5_i32).rem_euclid(CHUNK_SIZE as i32) as usize;
        let local_y = (-20_i32).rem_euclid(CHUNK_SIZE as i32) as usize;
        let local_z = (-1_i32).rem_euclid(CHUNK_SIZE as i32) as usize;
        assert_eq!((local_x, local_y, local_z), (11, 12, 15));
    }
}
