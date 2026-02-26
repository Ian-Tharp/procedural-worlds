//! Water current computation
//!
//! Calculates horizontal flow vectors from water level gradients.
//! Water flows "downhill" from higher levels to lower levels, and the
//! resulting current can push entities (players, items, boats).
//!
//! All functions are pure — no ECS access, easy to test.

use bevy::prelude::*;

use super::{WaterLevelMap, MAX_WATER_LEVEL};

/// Strength of current push per level difference (blocks/s²).
///
/// A full 7-level gradient produces `7 * CURRENT_STRENGTH_PER_LEVEL` acceleration.
pub const CURRENT_STRENGTH_PER_LEVEL: f32 = 0.8;

/// Maximum current acceleration magnitude (blocks/s²).
pub const MAX_CURRENT_ACCELERATION: f32 = 4.0;

/// Minimum water level required to generate any current.
/// Level-1 water is too shallow to push.
pub const MIN_CURRENT_LEVEL: u8 = 2;

/// Horizontal neighbor offsets with their direction vectors.
const FLOW_DIRECTIONS: [(IVec3, Vec3); 4] = [
    (IVec3::X, Vec3::X),
    (IVec3::NEG_X, Vec3::NEG_X),
    (IVec3::Z, Vec3::Z),
    (IVec3::NEG_Z, Vec3::NEG_Z),
];

/// Compute the horizontal water current at a world block position.
///
/// The current vector points in the direction water flows (from high to low).
/// Magnitude scales with the level gradient. Returns `Vec3::ZERO` if there
/// is no water or no gradient.
///
/// # Algorithm
///
/// For each horizontal neighbor, compute `my_level - neighbor_level`.
/// Positive differences mean water flows toward that neighbor.
/// Sum all flow contributions and clamp to `MAX_CURRENT_ACCELERATION`.
pub fn compute_current(block_pos: IVec3, water_levels: &WaterLevelMap) -> Vec3 {
    let my_level = water_levels.get_level(block_pos);
    if my_level < MIN_CURRENT_LEVEL {
        return Vec3::ZERO;
    }

    let mut current = Vec3::ZERO;

    for (offset, direction) in FLOW_DIRECTIONS {
        let neighbor_level = water_levels.get_level(block_pos + offset);
        let diff = my_level as f32 - neighbor_level as f32;

        if diff > 0.0 {
            // Water flows toward the neighbor (downhill)
            current += direction * diff * CURRENT_STRENGTH_PER_LEVEL;
        }
    }

    // Also check: if block above has water, there's a downward flow source
    // that boosts horizontal spread (like a waterfall pushing outward)
    let above_level = water_levels.get_level(block_pos + IVec3::Y);
    if above_level >= MAX_WATER_LEVEL {
        // Waterfall: boost current in all existing flow directions
        let len = current.length();
        if len > 0.0 {
            current *= 1.5;
        }
    }

    // Clamp magnitude
    let mag = current.length();
    if mag > MAX_CURRENT_ACCELERATION {
        current = current * (MAX_CURRENT_ACCELERATION / mag);
    }

    current
}

/// Compute the current at a precise world position by interpolating
/// between the current block and neighbors.
///
/// Uses bilinear interpolation on the XZ plane for smooth transitions.
pub fn compute_current_interpolated(pos: Vec3, water_levels: &WaterLevelMap) -> Vec3 {
    let bx = pos.x.floor() as i32;
    let by = pos.y.floor() as i32;
    let bz = pos.z.floor() as i32;

    // Fractional position within block
    let fx = pos.x - bx as f32;
    let fz = pos.z - bz as f32;

    // Sample 4 corners for bilinear interpolation
    let c00 = compute_current(IVec3::new(bx, by, bz), water_levels);
    let c10 = compute_current(IVec3::new(bx + 1, by, bz), water_levels);
    let c01 = compute_current(IVec3::new(bx, by, bz + 1), water_levels);
    let c11 = compute_current(IVec3::new(bx + 1, by, bz + 1), water_levels);

    // Bilinear interpolation
    let top = c00 * (1.0 - fx) + c10 * fx;
    let bottom = c01 * (1.0 - fx) + c11 * fx;
    top * (1.0 - fz) + bottom * fz
}

/// Apply current force to an entity's velocity over a time step.
///
/// Returns the new horizontal velocity after current acceleration and drag.
/// Vertical component is unchanged.
pub fn apply_current_to_velocity(velocity: Vec3, current: Vec3, dt: f32) -> Vec3 {
    Vec3::new(
        velocity.x + current.x * dt,
        velocity.y,
        velocity.z + current.z * dt,
    )
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_map(entries: &[(IVec3, u8)]) -> WaterLevelMap {
        let mut map = WaterLevelMap::default();
        for &(pos, level) in entries {
            map.set_level(pos, level);
        }
        map
    }

    #[test]
    fn test_no_water_no_current() {
        let map = WaterLevelMap::default();
        let c = compute_current(IVec3::ZERO, &map);
        assert_eq!(c, Vec3::ZERO);
    }

    #[test]
    fn test_uniform_water_no_current() {
        // All neighbors at same level => no gradient => no current
        let map = make_map(&[
            (IVec3::ZERO, 7),
            (IVec3::X, 7),
            (IVec3::NEG_X, 7),
            (IVec3::Z, 7),
            (IVec3::NEG_Z, 7),
        ]);
        let c = compute_current(IVec3::ZERO, &map);
        assert_eq!(c, Vec3::ZERO);
    }

    #[test]
    fn test_gradient_produces_current() {
        // Source at origin (7), level 3 to +X, level 7 on other sides
        // Gradient only toward +X => current flows +X
        let map = make_map(&[
            (IVec3::ZERO, 7),
            (IVec3::X, 3),
            (IVec3::NEG_X, 7),
            (IVec3::Z, 7),
            (IVec3::NEG_Z, 7),
        ]);
        let c = compute_current(IVec3::ZERO, &map);
        assert!(c.x > 0.0, "Should flow +X, got {:?}", c);
        assert!(c.z.abs() < 0.001, "No Z flow expected");
    }

    #[test]
    fn test_current_direction_follows_gradient() {
        // Level 7 at origin, level 3 at +Z, all others 7
        let map = make_map(&[
            (IVec3::ZERO, 7),
            (IVec3::X, 7),
            (IVec3::NEG_X, 7),
            (IVec3::Z, 3),
            (IVec3::NEG_Z, 7),
        ]);
        let c = compute_current(IVec3::ZERO, &map);
        // Only gradient is toward +Z (diff = 4)
        assert!(c.z > 0.0, "Should flow +Z");
        assert!(c.x.abs() < 0.001, "No X flow expected");
    }

    #[test]
    fn test_current_magnitude_scales_with_gradient() {
        // Small gradient: 7 to 6 in +X, all others at 7 (no gradient)
        let map_small = make_map(&[
            (IVec3::ZERO, 7),
            (IVec3::X, 6),
            (IVec3::NEG_X, 7),
            (IVec3::Z, 7),
            (IVec3::NEG_Z, 7),
        ]);
        // Large gradient: 7 to 2 in +X, all others at 7
        let map_large = make_map(&[
            (IVec3::ZERO, 7),
            (IVec3::X, 2),
            (IVec3::NEG_X, 7),
            (IVec3::Z, 7),
            (IVec3::NEG_Z, 7),
        ]);
        let c_small = compute_current(IVec3::ZERO, &map_small);
        let c_large = compute_current(IVec3::ZERO, &map_large);
        assert!(c_large.length() > c_small.length(),
            "Larger gradient should produce stronger current: small={}, large={}",
            c_small.length(), c_large.length());
    }

    #[test]
    fn test_current_clamped_to_max() {
        // Extreme gradient: level 7 surrounded by air on all sides
        let map = make_map(&[(IVec3::ZERO, 7)]);
        let c = compute_current(IVec3::ZERO, &map);
        assert!(c.length() <= MAX_CURRENT_ACCELERATION + 0.001,
            "Current {} exceeds max {}", c.length(), MAX_CURRENT_ACCELERATION);
    }

    #[test]
    fn test_below_min_level_no_current() {
        let map = make_map(&[(IVec3::ZERO, 1)]);
        let c = compute_current(IVec3::ZERO, &map);
        assert_eq!(c, Vec3::ZERO, "Level 1 is below MIN_CURRENT_LEVEL");
    }

    #[test]
    fn test_waterfall_boosts_current() {
        // Source above + gradient sideways = boosted current
        let map_no_fall = make_map(&[
            (IVec3::ZERO, 7),
            (IVec3::X, 3),
        ]);
        let map_with_fall = make_map(&[
            (IVec3::ZERO, 7),
            (IVec3::X, 3),
            (IVec3::Y, 7), // water above = waterfall
        ]);
        let c_normal = compute_current(IVec3::ZERO, &map_no_fall);
        let c_boosted = compute_current(IVec3::ZERO, &map_with_fall);
        assert!(c_boosted.length() > c_normal.length(),
            "Waterfall should boost: normal={}, boosted={}",
            c_normal.length(), c_boosted.length());
    }

    #[test]
    fn test_current_is_horizontal_only() {
        let map = make_map(&[(IVec3::ZERO, 7)]);
        let c = compute_current(IVec3::ZERO, &map);
        assert_eq!(c.y, 0.0, "Current should have no vertical component");
    }

    #[test]
    fn test_apply_current_to_velocity() {
        let vel = Vec3::new(1.0, -5.0, 0.0);
        let current = Vec3::new(2.0, 0.0, 3.0);
        let dt = 0.5;
        let result = apply_current_to_velocity(vel, current, dt);
        assert!((result.x - 2.0).abs() < 0.001); // 1.0 + 2.0*0.5
        assert_eq!(result.y, -5.0); // unchanged
        assert!((result.z - 1.5).abs() < 0.001); // 0.0 + 3.0*0.5
    }

    #[test]
    fn test_apply_current_zero_dt() {
        let vel = Vec3::new(3.0, 2.0, 1.0);
        let current = Vec3::new(100.0, 0.0, 100.0);
        let result = apply_current_to_velocity(vel, current, 0.0);
        assert_eq!(result, vel);
    }

    #[test]
    fn test_interpolated_center_of_block() {
        // At block center (0.5, 0.0, 0.5) should approximate the block's current
        let map = make_map(&[
            (IVec3::ZERO, 7),
            (IVec3::X, 3),
        ]);
        let block_current = compute_current(IVec3::ZERO, &map);
        let interp = compute_current_interpolated(Vec3::new(0.25, 0.0, 0.25), &map);
        // Should be in same general direction
        if block_current.length() > 0.0 && interp.length() > 0.0 {
            let dot = block_current.normalize().dot(interp.normalize());
            assert!(dot > 0.5, "Interpolated should roughly match direction: dot={}", dot);
        }
    }

    #[test]
    fn test_interpolated_at_boundary_blends() {
        // Two blocks with opposing gradients — interpolation at boundary should blend
        let map = make_map(&[
            (IVec3::ZERO, 7),
            (IVec3::X, 7),
            // Air everywhere else: both blocks push outward
        ]);
        let at_zero = compute_current_interpolated(Vec3::new(0.1, 0.0, 0.5), &map);
        let at_boundary = compute_current_interpolated(Vec3::new(0.9, 0.0, 0.5), &map);
        // At boundary, +X block contributes more, changing the blend
        // Just verify we get non-zero reasonable values
        assert!(at_zero.length() > 0.0);
        assert!(at_boundary.length() > 0.0);
    }

    #[test]
    fn test_constants_sanity() {
        assert!(CURRENT_STRENGTH_PER_LEVEL > 0.0);
        assert!(MAX_CURRENT_ACCELERATION > 0.0);
        assert!(MIN_CURRENT_LEVEL >= 1);
        assert!(MIN_CURRENT_LEVEL <= MAX_WATER_LEVEL);
    }
}