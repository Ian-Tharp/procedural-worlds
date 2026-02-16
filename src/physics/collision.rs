//! Collision detection and resolution for capsule colliders
//!
//! This module provides block-level collision detection using the actual
//! chunk data rather than heightmap estimation. This enables:
//! - Proper cave collision
//! - Horizontal wall collision
//! - Ceiling/head collision
//! - Accurate ground detection
//!
//! # Collision Algorithm
//!
//! 1. Calculate AABB around the capsule
//! 2. Query all solid blocks in that AABB
//! 3. For each solid block, test capsule-AABB intersection
//! 4. Accumulate penetration vectors
//! 5. Push capsule out by the deepest penetration
//!
//! # Coordinate System
//!
//! - Position is at FEET (bottom of capsule)
//! - Blocks occupy integer coordinates [x, x+1) × [y, y+1) × [z, z+1)
//! - A block at (0, 0, 0) spans from (0,0,0) to (1,1,1)

use bevy::prelude::*;

// Re-export Resource for the derive macro

use crate::actors::CapsuleCollider;
use crate::world::{BlockType, CHUNK_SIZE, Chunk, ChunkManager};

/// Result of a collision check
#[derive(Debug, Clone, Default)]
pub struct CollisionResult {
    /// Whether any collision was detected
    pub hit: bool,
    /// Penetration vector to push out of collision (add to position)
    pub penetration: Vec3,
    /// Whether standing on solid ground
    pub grounded: bool,
    /// Normal of the ground surface (if grounded)
    pub ground_normal: Vec3,
    /// Whether head hit a ceiling
    pub head_hit: bool,
    /// Number of blocks collided with
    pub block_count: u32,
}

/// Configuration for collision behavior
#[derive(Resource, Debug, Clone)]
pub struct CollisionParams {
    /// Small epsilon for floating point comparisons
    pub epsilon: f32,
    /// Skin width - keeps capsule slightly away from surfaces
    pub skin_width: f32,
    /// Maximum step height for auto-stepping (Phase 3)
    #[allow(dead_code)]
    pub step_height: f32,
    /// Ground detection tolerance (how close to surface counts as grounded)
    pub ground_tolerance: f32,
}

impl Default for CollisionParams {
    fn default() -> Self {
        Self {
            epsilon: 0.001,
            skin_width: 0.01,
            step_height: 0.6,
            ground_tolerance: 0.05,
        }
    }
}

/// Check collision between a capsule and the world
///
/// # Arguments
/// * `feet_pos` - Position of the capsule's feet (bottom)
/// * `collider` - The capsule collider shape
/// * `chunk_manager` - Resource tracking loaded chunks
/// * `chunks` - Query for chunk block data
/// * `params` - Collision parameters
///
/// # Returns
/// A `CollisionResult` with penetration and ground info
pub fn check_capsule_world_collision(
    feet_pos: Vec3,
    collider: &CapsuleCollider,
    chunk_manager: &ChunkManager,
    chunks: &Query<&Chunk>,
    params: &CollisionParams,
) -> CollisionResult {
    let mut result = CollisionResult::default();

    // Calculate AABB around capsule (with skin width)
    let aabb_min = IVec3::new(
        (feet_pos.x - collider.radius - params.skin_width).floor() as i32,
        (feet_pos.y - params.skin_width).floor() as i32,
        (feet_pos.z - collider.radius - params.skin_width).floor() as i32,
    );
    let aabb_max = IVec3::new(
        (feet_pos.x + collider.radius + params.skin_width).ceil() as i32,
        (feet_pos.y + collider.height + params.skin_width).ceil() as i32,
        (feet_pos.z + collider.radius + params.skin_width).ceil() as i32,
    );

    // Check all blocks in AABB
    for bx in aabb_min.x..=aabb_max.x {
        for by in aabb_min.y..=aabb_max.y {
            for bz in aabb_min.z..=aabb_max.z {
                let block_pos = IVec3::new(bx, by, bz);
                let block = get_block_at(block_pos, chunk_manager, chunks);

                if !block.is_solid() {
                    continue;
                }

                // Test capsule-block collision
                if let Some(penetration) =
                    capsule_aabb_penetration(feet_pos, collider, block_pos, params.epsilon)
                {
                    result.hit = true;
                    result.block_count += 1;

                    // Accumulate penetration (use the largest component in each direction)
                    if penetration.x.abs() > result.penetration.x.abs() {
                        result.penetration.x = penetration.x;
                    }
                    if penetration.y.abs() > result.penetration.y.abs() {
                        result.penetration.y = penetration.y;
                    }
                    if penetration.z.abs() > result.penetration.z.abs() {
                        result.penetration.z = penetration.z;
                    }

                    // Check if this is a ground collision (block below feet)
                    let block_top = by as f32 + 1.0;
                    if penetration.y > 0.0
                        && (feet_pos.y - block_top).abs() < params.ground_tolerance
                    {
                        result.grounded = true;
                        result.ground_normal = Vec3::Y;
                    }

                    // Check if this is a ceiling collision (block above head)
                    let head_y = feet_pos.y + collider.height;
                    let block_bottom = by as f32;
                    if penetration.y < 0.0
                        && (head_y - block_bottom).abs() < params.ground_tolerance
                    {
                        result.head_hit = true;
                    }
                }
            }
        }
    }

    result
}

/// Check for ground beneath the capsule
///
/// Casts a short ray downward from the feet to detect ground.
/// More reliable than collision penetration for ground state.
pub fn check_ground(
    feet_pos: Vec3,
    collider: &CapsuleCollider,
    chunk_manager: &ChunkManager,
    chunks: &Query<&Chunk>,
    params: &CollisionParams,
) -> (bool, f32) {
    // Check the block directly below feet
    let check_y = (feet_pos.y - params.ground_tolerance).floor() as i32;

    // Check in a small area under the capsule (not just center)
    let check_radius = collider.radius * 0.5;
    let check_positions = [
        Vec3::new(feet_pos.x, 0.0, feet_pos.z),
        Vec3::new(feet_pos.x - check_radius, 0.0, feet_pos.z),
        Vec3::new(feet_pos.x + check_radius, 0.0, feet_pos.z),
        Vec3::new(feet_pos.x, 0.0, feet_pos.z - check_radius),
        Vec3::new(feet_pos.x, 0.0, feet_pos.z + check_radius),
    ];

    for pos in check_positions {
        let block_pos = IVec3::new(pos.x.floor() as i32, check_y, pos.z.floor() as i32);

        let block = get_block_at(block_pos, chunk_manager, chunks);
        if block.is_solid() {
            let ground_y = (check_y + 1) as f32; // Top of block
            let distance = feet_pos.y - ground_y;
            if distance.abs() < params.ground_tolerance + 0.1 {
                return (true, ground_y);
            }
        }
    }

    (false, 0.0)
}

/// Check if head would hit a ceiling at the given position
pub fn check_ceiling(
    feet_pos: Vec3,
    collider: &CapsuleCollider,
    chunk_manager: &ChunkManager,
    chunks: &Query<&Chunk>,
) -> bool {
    let head_y = feet_pos.y + collider.height;
    let head_block_y = head_y.ceil() as i32;

    // Check blocks at head level
    let check_positions = [
        IVec3::new(
            feet_pos.x.floor() as i32,
            head_block_y,
            feet_pos.z.floor() as i32,
        ),
        IVec3::new(
            feet_pos.x.ceil() as i32,
            head_block_y,
            feet_pos.z.floor() as i32,
        ),
        IVec3::new(
            feet_pos.x.floor() as i32,
            head_block_y,
            feet_pos.z.ceil() as i32,
        ),
        IVec3::new(
            feet_pos.x.ceil() as i32,
            head_block_y,
            feet_pos.z.ceil() as i32,
        ),
    ];

    for block_pos in check_positions {
        if get_block_at(block_pos, chunk_manager, chunks).is_solid() {
            return true;
        }
    }

    false
}

/// Check if a horizontal move is valid (auto-step detection)
///
/// Returns the new Y position if stepping is possible, None if blocked.
/// Note: For Phase 3 auto-stepping implementation.
#[allow(dead_code)]
pub fn check_step(
    current_pos: Vec3,
    target_pos: Vec3,
    collider: &CapsuleCollider,
    chunk_manager: &ChunkManager,
    chunks: &Query<&Chunk>,
    params: &CollisionParams,
) -> Option<f32> {
    // Check if target position is blocked
    let collision =
        check_capsule_world_collision(target_pos, collider, chunk_manager, chunks, params);

    if !collision.hit {
        // No collision, can move freely
        return Some(target_pos.y);
    }

    // Try stepping up
    for step in 1..=((params.step_height / 0.5).ceil() as i32) {
        let step_height = step as f32 * 0.5;
        let stepped_pos = Vec3::new(target_pos.x, current_pos.y + step_height, target_pos.z);

        // Check if stepped position is clear
        let step_collision =
            check_capsule_world_collision(stepped_pos, collider, chunk_manager, chunks, params);

        if !step_collision.hit {
            // Check there's ground to stand on
            let (has_ground, ground_y) =
                check_ground(stepped_pos, collider, chunk_manager, chunks, params);

            if has_ground && ground_y >= current_pos.y {
                return Some(ground_y);
            }
        }
    }

    // Blocked, can't step
    None
}

/// Resolve collision by moving position out of solid blocks
///
/// Iteratively resolves penetration until clear or max iterations reached.
///
/// # Returns
/// The resolved position and final collision result
pub fn resolve_collision(
    mut feet_pos: Vec3,
    velocity: &mut Vec3,
    collider: &CapsuleCollider,
    chunk_manager: &ChunkManager,
    chunks: &Query<&Chunk>,
    params: &CollisionParams,
) -> (Vec3, CollisionResult) {
    const MAX_ITERATIONS: u32 = 4;
    let mut final_result = CollisionResult::default();

    for _ in 0..MAX_ITERATIONS {
        let result =
            check_capsule_world_collision(feet_pos, collider, chunk_manager, chunks, params);

        if !result.hit {
            final_result.grounded = result.grounded;
            final_result.ground_normal = result.ground_normal;
            break;
        }

        // Apply penetration to push out
        feet_pos += result.penetration;
        final_result.hit = true;
        final_result.block_count += result.block_count;

        // Cancel velocity into the collision surface
        if result.penetration.x.abs() > params.epsilon {
            velocity.x = 0.0;
        }
        if result.penetration.y.abs() > params.epsilon {
            if result.penetration.y > 0.0 {
                // Hit floor, cancel downward velocity
                velocity.y = velocity.y.max(0.0);
                final_result.grounded = true;
                final_result.ground_normal = Vec3::Y;
            } else {
                // Hit ceiling, cancel upward velocity
                velocity.y = velocity.y.min(0.0);
                final_result.head_hit = true;
            }
        }
        if result.penetration.z.abs() > params.epsilon {
            velocity.z = 0.0;
        }
    }

    (feet_pos, final_result)
}

// ============================================================================
// INTERNAL HELPERS
// ============================================================================

/// Calculate penetration vector for capsule-AABB collision
///
/// Returns the minimum translation vector to separate them, or None if no collision.
fn capsule_aabb_penetration(
    feet_pos: Vec3,
    collider: &CapsuleCollider,
    block_pos: IVec3,
    epsilon: f32,
) -> Option<Vec3> {
    // Block AABB
    let block_min = Vec3::new(block_pos.x as f32, block_pos.y as f32, block_pos.z as f32);
    let block_max = block_min + Vec3::ONE;

    // Capsule as a line segment from bottom sphere to top sphere
    let capsule_bottom = feet_pos + Vec3::new(0.0, collider.radius, 0.0);
    let capsule_top = feet_pos + Vec3::new(0.0, collider.height - collider.radius, 0.0);

    // Find closest point on capsule axis to block center
    let block_center = (block_min + block_max) * 0.5;
    let closest_on_axis = closest_point_on_segment(capsule_bottom, capsule_top, block_center);

    // Now treat it as sphere (at closest_on_axis) vs AABB
    let closest_on_aabb = Vec3::new(
        closest_on_axis.x.clamp(block_min.x, block_max.x),
        closest_on_axis.y.clamp(block_min.y, block_max.y),
        closest_on_axis.z.clamp(block_min.z, block_max.z),
    );

    let diff = closest_on_axis - closest_on_aabb;
    let dist_sq = diff.length_squared();
    let radius_sq = collider.radius * collider.radius;

    if dist_sq >= radius_sq - epsilon {
        return None; // No collision
    }

    // Calculate penetration
    let dist = dist_sq.sqrt();
    let penetration_depth = collider.radius - dist;

    if dist < epsilon {
        // Capsule center is inside AABB, push out via shortest axis
        let to_min = closest_on_axis - block_min;
        let to_max = block_max - closest_on_axis;

        let mut min_dist = to_min.x;
        let mut normal = Vec3::NEG_X;

        if to_max.x < min_dist {
            min_dist = to_max.x;
            normal = Vec3::X;
        }
        if to_min.y < min_dist {
            min_dist = to_min.y;
            normal = Vec3::NEG_Y;
        }
        if to_max.y < min_dist {
            min_dist = to_max.y;
            normal = Vec3::Y;
        }
        if to_min.z < min_dist {
            min_dist = to_min.z;
            normal = Vec3::NEG_Z;
        }
        if to_max.z < min_dist {
            normal = Vec3::Z;
        }

        Some(normal * (min_dist + collider.radius + epsilon))
    } else {
        // Normal case: push along the direction from AABB to capsule
        let normal = diff / dist;
        Some(normal * (penetration_depth + epsilon))
    }
}

/// Find the closest point on a line segment to a target point
fn closest_point_on_segment(a: Vec3, b: Vec3, point: Vec3) -> Vec3 {
    let ab = b - a;
    let ap = point - a;
    let ab_len_sq = ab.length_squared();

    if ab_len_sq < 0.0001 {
        return a; // Degenerate segment
    }

    let t = (ap.dot(ab) / ab_len_sq).clamp(0.0, 1.0);
    a + ab * t
}

/// Get block at world position (wrapper for chunk queries)
fn get_block_at(
    world_pos: IVec3,
    chunk_manager: &ChunkManager,
    chunks: &Query<&Chunk>,
) -> BlockType {
    // Calculate chunk position
    let chunk_pos = IVec3::new(
        world_pos.x.div_euclid(CHUNK_SIZE as i32),
        world_pos.y.div_euclid(CHUNK_SIZE as i32),
        world_pos.z.div_euclid(CHUNK_SIZE as i32),
    );

    // Look up chunk entity
    let Some(&entity) = chunk_manager.chunks.get(&chunk_pos) else {
        return BlockType::Air; // Unloaded chunk = air (allows movement)
    };

    // Get chunk component
    let Ok(chunk) = chunks.get(entity) else {
        return BlockType::Air;
    };

    // Calculate local position
    let local_x = world_pos.x.rem_euclid(CHUNK_SIZE as i32) as usize;
    let local_y = world_pos.y.rem_euclid(CHUNK_SIZE as i32) as usize;
    let local_z = world_pos.z.rem_euclid(CHUNK_SIZE as i32) as usize;

    chunk.get_block(local_x, local_y, local_z)
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_closest_point_on_segment() {
        let a = Vec3::new(0.0, 0.0, 0.0);
        let b = Vec3::new(0.0, 2.0, 0.0);

        // Point on segment
        let p1 = closest_point_on_segment(a, b, Vec3::new(0.0, 1.0, 0.0));
        assert!((p1 - Vec3::new(0.0, 1.0, 0.0)).length() < 0.001);

        // Point beyond end
        let p2 = closest_point_on_segment(a, b, Vec3::new(0.0, 5.0, 0.0));
        assert!((p2 - Vec3::new(0.0, 2.0, 0.0)).length() < 0.001);

        // Point before start
        let p3 = closest_point_on_segment(a, b, Vec3::new(0.0, -1.0, 0.0));
        assert!((p3 - Vec3::new(0.0, 0.0, 0.0)).length() < 0.001);

        // Point to the side
        let p4 = closest_point_on_segment(a, b, Vec3::new(1.0, 1.0, 0.0));
        assert!((p4 - Vec3::new(0.0, 1.0, 0.0)).length() < 0.001);
    }

    #[test]
    fn test_capsule_dimensions() {
        let collider = CapsuleCollider::default();
        assert_eq!(collider.radius, 0.3);
        assert_eq!(collider.height, 1.8);
    }

    #[test]
    fn test_collision_params_default() {
        let params = CollisionParams::default();
        assert_eq!(params.step_height, 0.6);
        assert!(params.epsilon > 0.0);
        assert!(params.skin_width > 0.0);
    }

    #[test]
    fn test_capsule_aabb_no_collision() {
        let collider = CapsuleCollider::default();
        let feet_pos = Vec3::new(10.0, 10.0, 10.0);
        let block_pos = IVec3::new(0, 0, 0); // Far away block

        let result = capsule_aabb_penetration(feet_pos, &collider, block_pos, 0.001);
        assert!(result.is_none());
    }

    #[test]
    fn test_capsule_aabb_collision() {
        let collider = CapsuleCollider::default();
        // Place capsule so it overlaps with block at origin
        let feet_pos = Vec3::new(0.5, 0.0, 0.5);
        let block_pos = IVec3::new(0, 0, 0);

        let result = capsule_aabb_penetration(feet_pos, &collider, block_pos, 0.001);
        assert!(result.is_some());
    }

    #[test]
    fn test_collision_result_default() {
        let result = CollisionResult::default();
        assert!(!result.hit);
        assert!(!result.grounded);
        assert!(!result.head_hit);
        assert_eq!(result.block_count, 0);
    }
}
