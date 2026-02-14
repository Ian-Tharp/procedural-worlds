//! DDA Voxel Raycast System
//!
//! Implements a Digital Differential Analyzer (DDA) algorithm to walk through
//! the voxel grid and determine which block the player is looking at.
//!
//! # How it works
//!
//! The DDA algorithm steps through voxel cells one at a time along a ray,
//! checking each cell for a solid block. It returns:
//! - The position of the first solid block hit
//! - The face normal (which face of the block was hit)
//! - The adjacent empty block position (for block placement)
//! - The distance from the ray origin to the hit
//!
//! # Usage
//!
//! Add [`RaycastPlugin`] to your app. The system runs each frame and stores
//! the result in the [`CurrentTarget`] resource. Other systems (debug overlay,
//! block interaction) can read this resource.
//!
//! ```ignore
//! fn my_system(target: Res<CurrentTarget>) {
//!     if let Some(ref result) = target.0 {
//!         info!("Looking at {:?} at {:?}", result.block_type, result.block_pos);
//!     }
//! }
//! ```

use bevy::prelude::*;

use crate::engine::controller::CameraController;
use crate::world::{BlockType, Chunk, ChunkManager};

/// Maximum distance (in blocks) for the raycast
const MAX_RAY_DISTANCE: f32 = 10.0;

// ============================================================================
// DATA STRUCTURES
// ============================================================================

/// Result of a voxel raycast
///
/// Contains all information needed for block interaction:
/// highlighting, placement, and destruction.
#[derive(Debug, Clone)]
pub struct RaycastResult {
    /// Whether a solid block was hit
    pub hit: bool,
    /// World position of the hit block (integer coordinates)
    pub block_pos: IVec3,
    /// Normal of the face that was hit (points outward from the block)
    pub face_normal: IVec3,
    /// Position of the empty block adjacent to the hit face (for placement)
    pub adjacent_pos: IVec3,
    /// Distance from the ray origin to the hit point
    pub distance: f32,
    /// Type of block that was hit
    pub block_type: BlockType,
}

impl RaycastResult {
    /// Create a "miss" result (no block hit)
    pub fn miss() -> Self {
        Self {
            hit: false,
            block_pos: IVec3::ZERO,
            face_normal: IVec3::ZERO,
            adjacent_pos: IVec3::ZERO,
            distance: 0.0,
            block_type: BlockType::Air,
        }
    }
}

/// Resource storing the current raycast target
///
/// Updated every frame by the raycast system. `None` when no block
/// is within range or the raycast hasn't run yet.
#[derive(Resource, Default)]
pub struct CurrentTarget(pub Option<RaycastResult>);

// ============================================================================
// DDA ALGORITHM
// ============================================================================

/// Perform a DDA voxel raycast through the world
///
/// Walks through the voxel grid one cell at a time along the given ray,
/// returning the first solid block hit within `max_distance`.
///
/// # Arguments
/// * `origin` - Ray origin in world space (e.g., camera position)
/// * `direction` - Normalized ray direction
/// * `max_distance` - Maximum distance to search (in blocks)
/// * `chunk_manager` - The chunk manager for block lookups
/// * `chunks` - Query for chunk data
///
/// # Returns
/// A [`RaycastResult`] describing the first solid block hit, or a miss.
///
/// # Algorithm
///
/// The DDA (Digital Differential Analyzer) algorithm:
/// 1. Determine which voxel cell the ray starts in
/// 2. For each axis, compute `t_max` (distance to next cell boundary)
///    and `t_delta` (distance to cross one full cell)
/// 3. Step along the axis with the smallest `t_max`
/// 4. Check the new cell for a solid block
/// 5. Repeat until a hit or max distance is reached
pub fn voxel_raycast(
    origin: Vec3,
    direction: Vec3,
    max_distance: f32,
    chunk_manager: &ChunkManager,
    chunks: &Query<&Chunk>,
) -> RaycastResult {
    // Normalize direction (safety — caller should already provide normalized)
    let dir = if direction.length_squared() > 0.0 {
        direction.normalize()
    } else {
        return RaycastResult::miss();
    };

    // Current voxel position (floor to get the containing block)
    let mut voxel = IVec3::new(
        origin.x.floor() as i32,
        origin.y.floor() as i32,
        origin.z.floor() as i32,
    );

    // Step direction for each axis (+1 or -1)
    let step = IVec3::new(
        if dir.x >= 0.0 { 1 } else { -1 },
        if dir.y >= 0.0 { 1 } else { -1 },
        if dir.z >= 0.0 { 1 } else { -1 },
    );

    // Distance along ray to cross one full voxel on each axis
    // (infinite if direction component is zero — we'll never step that axis)
    let t_delta = Vec3::new(
        if dir.x != 0.0 {
            (1.0 / dir.x).abs()
        } else {
            f32::INFINITY
        },
        if dir.y != 0.0 {
            (1.0 / dir.y).abs()
        } else {
            f32::INFINITY
        },
        if dir.z != 0.0 {
            (1.0 / dir.z).abs()
        } else {
            f32::INFINITY
        },
    );

    // Distance along ray to the NEXT voxel boundary on each axis
    let t_max_initial = Vec3::new(
        if dir.x >= 0.0 {
            ((voxel.x as f32 + 1.0) - origin.x) / dir.x
        } else if dir.x != 0.0 {
            (voxel.x as f32 - origin.x) / dir.x
        } else {
            f32::INFINITY
        },
        if dir.y >= 0.0 {
            ((voxel.y as f32 + 1.0) - origin.y) / dir.y
        } else if dir.y != 0.0 {
            (voxel.y as f32 - origin.y) / dir.y
        } else {
            f32::INFINITY
        },
        if dir.z >= 0.0 {
            ((voxel.z as f32 + 1.0) - origin.z) / dir.z
        } else if dir.z != 0.0 {
            (voxel.z as f32 - origin.z) / dir.z
        } else {
            f32::INFINITY
        },
    );

    let mut t_max = t_max_initial;

    // Track face normal (which axis we last stepped along, and in which direction)
    let mut face_normal = IVec3::ZERO;

    // Maximum number of steps to prevent infinite loops
    // A ray traveling diagonally through max_distance blocks visits at most
    // ~3 * max_distance cells (one step per axis per block)
    let max_steps = (max_distance * 3.0) as u32 + 1;

    for _ in 0..max_steps {
        // Check current voxel for a solid block
        let block_type = crate::world::get_block_at(voxel, chunk_manager, chunks);

        if block_type.is_solid() {
            // Calculate the distance to the hit point
            // The hit happened when we stepped INTO this voxel, so the distance
            // is the t_max of the axis we last stepped along minus t_delta
            let distance = match (face_normal.x != 0, face_normal.y != 0, face_normal.z != 0) {
                (true, _, _) => t_max.x - t_delta.x,
                (_, true, _) => t_max.y - t_delta.y,
                (_, _, true) => t_max.z - t_delta.z,
                _ => 0.0, // Origin is inside a solid block
            };

            // Check distance limit
            if distance > max_distance {
                return RaycastResult::miss();
            }

            return RaycastResult {
                hit: true,
                block_pos: voxel,
                face_normal,
                adjacent_pos: voxel + face_normal,
                distance,
                block_type,
            };
        }

        // Step to the next voxel boundary (choose the closest axis)
        if t_max.x < t_max.y && t_max.x < t_max.z {
            // Check distance before stepping
            if t_max.x > max_distance {
                return RaycastResult::miss();
            }
            voxel.x += step.x;
            face_normal = IVec3::new(-step.x, 0, 0);
            t_max.x += t_delta.x;
        } else if t_max.y < t_max.z {
            if t_max.y > max_distance {
                return RaycastResult::miss();
            }
            voxel.y += step.y;
            face_normal = IVec3::new(0, -step.y, 0);
            t_max.y += t_delta.y;
        } else {
            if t_max.z > max_distance {
                return RaycastResult::miss();
            }
            voxel.z += step.z;
            face_normal = IVec3::new(0, 0, -step.z);
            t_max.z += t_delta.z;
        }
    }

    RaycastResult::miss()
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// System that runs the voxel raycast each frame from the camera
///
/// Reads the camera's world-space transform to get the ray origin and direction,
/// then performs a DDA raycast and stores the result in [`CurrentTarget`].
pub fn update_raycast_target(
    camera_query: Query<&GlobalTransform, With<CameraController>>,
    chunk_manager: Res<ChunkManager>,
    chunks: Query<&Chunk>,
    mut current_target: ResMut<CurrentTarget>,
) {
    let Ok(camera_global) = camera_query.get_single() else {
        current_target.0 = None;
        return;
    };

    // Ray origin is the camera's world-space position
    let origin = camera_global.translation();

    // Forward direction from the camera's world-space rotation
    let direction = camera_global.forward().as_vec3();

    let result = voxel_raycast(origin, direction, MAX_RAY_DISTANCE, &chunk_manager, &chunks);

    current_target.0 = if result.hit { Some(result) } else { None };
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that adds voxel raycast functionality
///
/// Registers:
/// - [`CurrentTarget`] resource (updated every frame)
/// - Raycast system that casts from the camera each frame
///
/// # Dependencies
/// Requires [`CameraController`] on the camera entity and
/// [`ChunkManager`] / [`Chunk`] for block lookups.
///
/// # Note
/// Crosshair rendering has been moved to [`crate::editor::hud::HudPlugin`],
/// which also handles cursor-state-aware visibility and block info display.
pub struct RaycastPlugin;

impl Plugin for RaycastPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CurrentTarget>()
            .add_systems(Update, update_raycast_target);
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::SystemState;

    /// Helper: set up a minimal Bevy world with a single chunk filled with a given block type
    fn setup_test_world(fill_block: BlockType) -> (World, Entity) {
        let mut world = World::new();

        // Insert ChunkManager
        let mut chunk_manager = ChunkManager::default();

        // Create a chunk at origin (0, 0, 0) — covers blocks [0..16) on each axis
        let mut chunk = crate::world::Chunk::new(IVec3::ZERO);
        chunk.fill(fill_block);
        chunk.dirty = false;

        let entity = world.spawn(chunk).id();
        chunk_manager.chunks.insert(IVec3::ZERO, entity);

        world.insert_resource(chunk_manager);

        (world, entity)
    }

    /// Helper: set up a test world with a ground plane (y=0 filled with stone, rest air)
    fn setup_ground_world() -> World {
        let mut world = World::new();
        let mut chunk_manager = ChunkManager::default();

        // Chunk at (0, 0, 0) — ground layer
        let mut chunk = crate::world::Chunk::new(IVec3::ZERO);
        // Fill only the bottom layer (y=0) with stone
        for x in 0..16 {
            for z in 0..16 {
                chunk.set_block(x, 0, z, BlockType::Stone);
            }
        }
        chunk.dirty = false;

        let entity = world.spawn(chunk).id();
        chunk_manager.chunks.insert(IVec3::ZERO, entity);

        // Chunk at (0, -1, 0) — all stone (underground)
        let mut underground_chunk = crate::world::Chunk::new(IVec3::new(0, -1, 0));
        underground_chunk.fill(BlockType::Stone);
        underground_chunk.dirty = false;

        let underground_entity = world.spawn(underground_chunk).id();
        chunk_manager
            .chunks
            .insert(IVec3::new(0, -1, 0), underground_entity);

        world.insert_resource(chunk_manager);
        world
    }

    /// Run a raycast in a test world
    fn run_raycast(
        world: &mut World,
        origin: Vec3,
        direction: Vec3,
        max_dist: f32,
    ) -> RaycastResult {
        let mut system_state: SystemState<(Res<ChunkManager>, Query<&crate::world::Chunk>)> =
            SystemState::new(world);

        let (chunk_manager, chunks) = system_state.get(world);
        voxel_raycast(origin, direction, max_dist, &chunk_manager, &chunks)
    }

    // ========================================================================
    // Axis-aligned ray tests
    // ========================================================================

    #[test]
    fn test_ray_positive_x_hits_stone() {
        let (mut world, _) = setup_test_world(BlockType::Stone);

        // Ray from outside the chunk, pointing +X into the stone-filled chunk
        let result = run_raycast(&mut world, Vec3::new(-2.0, 8.0, 8.0), Vec3::X, 20.0);

        assert!(result.hit);
        assert_eq!(result.block_pos, IVec3::new(0, 8, 8));
        assert_eq!(result.face_normal, IVec3::new(-1, 0, 0)); // Hit the -X face
        assert_eq!(result.adjacent_pos, IVec3::new(-1, 8, 8));
        assert_eq!(result.block_type, BlockType::Stone);
        assert!((result.distance - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_ray_negative_x_hits_stone() {
        let (mut world, _) = setup_test_world(BlockType::Stone);

        // Ray from beyond the chunk, pointing -X
        let result = run_raycast(&mut world, Vec3::new(18.0, 8.0, 8.0), Vec3::NEG_X, 20.0);

        assert!(result.hit);
        assert_eq!(result.block_pos, IVec3::new(15, 8, 8));
        assert_eq!(result.face_normal, IVec3::new(1, 0, 0)); // Hit the +X face
        assert_eq!(result.adjacent_pos, IVec3::new(16, 8, 8));
        assert_eq!(result.block_type, BlockType::Stone);
    }

    #[test]
    fn test_ray_positive_y_hits_stone() {
        let (mut world, _) = setup_test_world(BlockType::Stone);

        let result = run_raycast(&mut world, Vec3::new(8.0, -3.0, 8.0), Vec3::Y, 20.0);

        assert!(result.hit);
        assert_eq!(result.block_pos, IVec3::new(8, 0, 8));
        assert_eq!(result.face_normal, IVec3::new(0, -1, 0)); // Hit the -Y face
        assert_eq!(result.block_type, BlockType::Stone);
    }

    #[test]
    fn test_ray_positive_z_hits_stone() {
        let (mut world, _) = setup_test_world(BlockType::Stone);

        let result = run_raycast(&mut world, Vec3::new(8.0, 8.0, -2.0), Vec3::Z, 20.0);

        assert!(result.hit);
        assert_eq!(result.block_pos, IVec3::new(8, 8, 0));
        assert_eq!(result.face_normal, IVec3::new(0, 0, -1)); // Hit the -Z face
        assert_eq!(result.block_type, BlockType::Stone);
    }

    // ========================================================================
    // Diagonal ray tests
    // ========================================================================

    #[test]
    fn test_ray_diagonal_hits_stone() {
        let (mut world, _) = setup_test_world(BlockType::Stone);

        // Diagonal ray from outside, should hit the first block it enters
        let direction = Vec3::new(1.0, 1.0, 1.0).normalize();
        let result = run_raycast(&mut world, Vec3::new(-1.5, -1.5, -1.5), direction, 20.0);

        assert!(result.hit);
        assert_eq!(result.block_pos, IVec3::new(0, 0, 0));
        assert_eq!(result.block_type, BlockType::Stone);
        // The face normal should be on one of the axes (whichever boundary was crossed first)
        assert!(result.face_normal != IVec3::ZERO);
    }

    #[test]
    fn test_ray_diagonal_xz_hits() {
        let (mut world, _) = setup_test_world(BlockType::Stone);

        // Diagonal in XZ plane
        let direction = Vec3::new(1.0, 0.0, 1.0).normalize();
        let result = run_raycast(&mut world, Vec3::new(-2.0, 8.0, -2.0), direction, 20.0);

        assert!(result.hit);
        assert_eq!(result.block_type, BlockType::Stone);
    }

    // ========================================================================
    // Miss / no-hit tests
    // ========================================================================

    #[test]
    fn test_ray_into_air_misses() {
        let (mut world, _) = setup_test_world(BlockType::Air);

        let result = run_raycast(&mut world, Vec3::new(-2.0, 8.0, 8.0), Vec3::X, 20.0);

        assert!(!result.hit);
    }

    #[test]
    fn test_ray_away_from_blocks_misses() {
        let (mut world, _) = setup_test_world(BlockType::Stone);

        // Ray pointing away from the chunk
        let result = run_raycast(&mut world, Vec3::new(-2.0, 8.0, 8.0), Vec3::NEG_X, 20.0);

        assert!(!result.hit);
    }

    #[test]
    fn test_ray_zero_direction_misses() {
        let (mut world, _) = setup_test_world(BlockType::Stone);

        // Zero direction should not crash, just miss
        let result = run_raycast(&mut world, Vec3::new(8.0, 8.0, 8.0), Vec3::ZERO, 20.0);

        assert!(!result.hit);
    }

    // ========================================================================
    // Max distance tests
    // ========================================================================

    #[test]
    fn test_max_distance_limits_raycast() {
        let (mut world, _) = setup_test_world(BlockType::Stone);

        // Block is 5 units away, but max distance is 3
        let result = run_raycast(&mut world, Vec3::new(-5.0, 8.0, 8.0), Vec3::X, 3.0);

        assert!(!result.hit);
    }

    #[test]
    fn test_max_distance_just_within_range() {
        let (mut world, _) = setup_test_world(BlockType::Stone);

        // Block is 2 units away, max distance is 3 — should hit
        let result = run_raycast(&mut world, Vec3::new(-2.0, 8.0, 8.0), Vec3::X, 3.0);

        assert!(result.hit);
    }

    // ========================================================================
    // Ground plane test (more realistic scenario)
    // ========================================================================

    #[test]
    fn test_looking_down_at_ground() {
        let mut world = setup_ground_world();

        // Standing at y=2, looking straight down
        let result = run_raycast(&mut world, Vec3::new(8.5, 2.5, 8.5), Vec3::NEG_Y, 10.0);

        assert!(result.hit);
        assert_eq!(result.block_pos, IVec3::new(8, 0, 8));
        assert_eq!(result.face_normal, IVec3::new(0, 1, 0)); // Hit the +Y (top) face
        assert_eq!(result.adjacent_pos, IVec3::new(8, 1, 8));
        assert_eq!(result.block_type, BlockType::Stone);
    }

    #[test]
    fn test_looking_at_ground_at_angle() {
        let mut world = setup_ground_world();

        // Standing at y=3, looking down at ~45 degrees in +X direction
        let direction = Vec3::new(1.0, -1.0, 0.0).normalize();
        let result = run_raycast(&mut world, Vec3::new(4.5, 3.5, 8.5), direction, 10.0);

        assert!(result.hit);
        assert_eq!(result.block_type, BlockType::Stone);
        // Should hit the top face of a ground block
        assert_eq!(result.face_normal, IVec3::new(0, 1, 0));
    }

    // ========================================================================
    // Adjacent position (placement) tests
    // ========================================================================

    #[test]
    fn test_adjacent_position_for_placement() {
        let (mut world, _) = setup_test_world(BlockType::Stone);

        // Hit from -X direction
        let result = run_raycast(&mut world, Vec3::new(-2.0, 8.0, 8.0), Vec3::X, 20.0);

        assert!(result.hit);
        // Adjacent block should be one step back along the normal
        assert_eq!(result.adjacent_pos, result.block_pos + result.face_normal);
        assert_eq!(result.adjacent_pos, IVec3::new(-1, 8, 8));
    }

    // ========================================================================
    // Result struct tests
    // ========================================================================

    #[test]
    fn test_raycast_result_miss() {
        let miss = RaycastResult::miss();
        assert!(!miss.hit);
        assert_eq!(miss.block_pos, IVec3::ZERO);
        assert_eq!(miss.face_normal, IVec3::ZERO);
        assert_eq!(miss.block_type, BlockType::Air);
    }

    // ========================================================================
    // Origin inside solid block
    // ========================================================================

    #[test]
    fn test_origin_inside_solid_block() {
        let (mut world, _) = setup_test_world(BlockType::Stone);

        // Origin inside the stone-filled chunk
        let result = run_raycast(&mut world, Vec3::new(8.5, 8.5, 8.5), Vec3::X, 10.0);

        // Should hit the block we're standing in
        assert!(result.hit);
        assert_eq!(result.block_pos, IVec3::new(8, 8, 8));
        assert_eq!(result.distance, 0.0);
    }

    // ========================================================================
    // Water (non-solid transparent) blocks
    // ========================================================================

    #[test]
    fn test_ray_passes_through_water() {
        let mut world = World::new();
        let mut chunk_manager = ChunkManager::default();

        // Create a chunk with water in front of stone
        let mut chunk = crate::world::Chunk::new(IVec3::ZERO);
        // Place water at x=2, stone at x=5
        chunk.set_block(2, 8, 8, BlockType::Water);
        chunk.set_block(3, 8, 8, BlockType::Water);
        chunk.set_block(5, 8, 8, BlockType::Stone);
        chunk.dirty = false;

        let entity = world.spawn(chunk).id();
        chunk_manager.chunks.insert(IVec3::ZERO, entity);
        world.insert_resource(chunk_manager);

        let result = run_raycast(&mut world, Vec3::new(0.5, 8.5, 8.5), Vec3::X, 20.0);

        assert!(result.hit);
        // Should pass through water and hit the stone
        assert_eq!(result.block_pos, IVec3::new(5, 8, 8));
        assert_eq!(result.block_type, BlockType::Stone);
    }
}
