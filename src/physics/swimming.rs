//! Swimming physics modifiers
//!
//! When the player has the `Swimming` marker component (from the water module),
//! physics behavior changes:
//! - Gravity is reduced (buoyancy counteracts it)
//! - Movement speed is reduced (water resistance)
//! - Terminal velocity is lower (drag)
//! - Jump out of water uses a different velocity
//!
//! All functions here are pure — they compute modified values without
//! accessing ECS state, making them easy to test.

/// Gravity multiplier when swimming (buoyancy reduces effective gravity).
pub const SWIMMING_GRAVITY_MULTIPLIER: f32 = 0.25;

/// Movement speed multiplier when swimming (water resistance).
pub const SWIMMING_SPEED_MULTIPLIER: f32 = 0.5;

/// Terminal velocity when swimming (drag limits fall speed in water).
pub const SWIMMING_TERMINAL_VELOCITY: f32 = 12.0;

/// Buoyancy force applied per tick when submerged (blocks/s²).
/// Pushes the player upward when below the water surface.
pub const BUOYANCY_ACCELERATION: f32 = 14.0;

/// Velocity applied when the player "jumps" while swimming (swim upward).
pub const SWIM_UP_VELOCITY: f32 = 4.5;

/// Water drag coefficient applied to horizontal velocity each frame.
/// `v_new = v_old * (1 - WATER_DRAG * dt)` — applied multiplicatively.
pub const WATER_DRAG: f32 = 3.0;

/// Compute effective gravity when swimming.
///
/// Returns `base_gravity * SWIMMING_GRAVITY_MULTIPLIER`.
pub fn swimming_gravity(base_gravity: f32) -> f32 {
    base_gravity * SWIMMING_GRAVITY_MULTIPLIER
}

/// Compute effective movement speed when swimming.
///
/// Returns `base_speed * SWIMMING_SPEED_MULTIPLIER`.
pub fn swimming_speed(base_speed: f32) -> f32 {
    base_speed * SWIMMING_SPEED_MULTIPLIER
}

/// Compute effective terminal velocity when swimming.
///
/// Always returns the swimming terminal velocity constant, ignoring the
/// base value since water drag is so much stronger than air resistance.
pub fn swimming_terminal_velocity() -> f32 {
    SWIMMING_TERMINAL_VELOCITY
}

/// Apply water drag to a horizontal velocity component over a time step.
///
/// Uses exponential decay: `v * e^(-WATER_DRAG * dt)`.
/// Returns the new velocity after drag is applied.
pub fn apply_water_drag(velocity: f32, dt: f32) -> f32 {
    velocity * (-WATER_DRAG * dt).exp()
}

/// Compute the net vertical acceleration when submerged.
///
/// When the player is below the surface, buoyancy pushes up while
/// gravity pulls down. Returns the combined acceleration (positive = up).
///
/// `depth_fraction` is 0.0 at the surface, 1.0 when fully submerged.
/// Buoyancy scales linearly with submersion depth.
pub fn net_vertical_acceleration(base_gravity: f32, depth_fraction: f32) -> f32 {
    let gravity_down = swimming_gravity(base_gravity);
    let buoyancy_up = BUOYANCY_ACCELERATION * depth_fraction.clamp(0.0, 1.0);
    buoyancy_up - gravity_down
}

/// Clamp vertical velocity to swimming terminal velocity.
///
/// Limits both upward and downward velocity to `SWIMMING_TERMINAL_VELOCITY`.
pub fn clamp_swimming_velocity_y(velocity_y: f32) -> f32 {
    velocity_y.clamp(-SWIMMING_TERMINAL_VELOCITY, SWIMMING_TERMINAL_VELOCITY)
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swimming_gravity_reduces_base() {
        let base = 32.0;
        let result = swimming_gravity(base);
        assert_eq!(result, 8.0); // 32 * 0.25
        assert!(result < base);
    }

    #[test]
    fn test_swimming_gravity_zero() {
        assert_eq!(swimming_gravity(0.0), 0.0);
    }

    #[test]
    fn test_swimming_speed_halves_base() {
        let base = 10.0;
        assert_eq!(swimming_speed(base), 5.0);
    }

    #[test]
    fn test_swimming_speed_zero() {
        assert_eq!(swimming_speed(0.0), 0.0);
    }

    #[test]
    fn test_swimming_terminal_velocity_is_constant() {
        assert_eq!(swimming_terminal_velocity(), 12.0);
    }

    #[test]
    fn test_swimming_terminal_velocity_less_than_air() {
        assert!(swimming_terminal_velocity() < crate::physics::TERMINAL_VELOCITY);
    }

    #[test]
    fn test_water_drag_reduces_velocity() {
        let v = 10.0;
        let result = apply_water_drag(v, 0.016); // ~60fps
        assert!(result < v);
        assert!(result > 0.0);
    }

    #[test]
    fn test_water_drag_zero_velocity() {
        assert_eq!(apply_water_drag(0.0, 0.016), 0.0);
    }

    #[test]
    fn test_water_drag_zero_dt() {
        assert_eq!(apply_water_drag(10.0, 0.0), 10.0);
    }

    #[test]
    fn test_water_drag_negative_velocity() {
        let result = apply_water_drag(-5.0, 0.016);
        assert!(result < 0.0);
        assert!(result > -5.0); // magnitude decreased
    }

    #[test]
    fn test_water_drag_large_dt_approaches_zero() {
        let result = apply_water_drag(100.0, 10.0);
        assert!(result.abs() < 0.001, "Large dt should drag velocity near zero, got {}", result);
    }

    #[test]
    fn test_net_vertical_acceleration_fully_submerged() {
        let acc = net_vertical_acceleration(32.0, 1.0);
        // buoyancy(14) - swimming_gravity(8) = 6
        assert!((acc - 6.0).abs() < 0.001, "Expected 6.0, got {}", acc);
    }

    #[test]
    fn test_net_vertical_acceleration_at_surface() {
        let acc = net_vertical_acceleration(32.0, 0.0);
        // buoyancy(0) - swimming_gravity(8) = -8
        assert!((acc - (-8.0)).abs() < 0.001, "Expected -8.0, got {}", acc);
    }

    #[test]
    fn test_net_vertical_acceleration_half_submerged() {
        let acc = net_vertical_acceleration(32.0, 0.5);
        // buoyancy(7) - swimming_gravity(8) = -1
        assert!((acc - (-1.0)).abs() < 0.001, "Expected -1.0, got {}", acc);
    }

    #[test]
    fn test_net_vertical_acceleration_clamps_depth() {
        let normal = net_vertical_acceleration(32.0, 1.0);
        let over = net_vertical_acceleration(32.0, 5.0);
        assert_eq!(normal, over, "Depth fraction should clamp at 1.0");
    }

    #[test]
    fn test_net_vertical_acceleration_clamps_negative_depth() {
        let at_surface = net_vertical_acceleration(32.0, 0.0);
        let negative = net_vertical_acceleration(32.0, -1.0);
        assert_eq!(at_surface, negative, "Negative depth should clamp to 0.0");
    }

    #[test]
    fn test_clamp_swimming_velocity_y_within_range() {
        assert_eq!(clamp_swimming_velocity_y(5.0), 5.0);
        assert_eq!(clamp_swimming_velocity_y(-5.0), -5.0);
    }

    #[test]
    fn test_clamp_swimming_velocity_y_exceeds_positive() {
        assert_eq!(clamp_swimming_velocity_y(50.0), SWIMMING_TERMINAL_VELOCITY);
    }

    #[test]
    fn test_clamp_swimming_velocity_y_exceeds_negative() {
        assert_eq!(clamp_swimming_velocity_y(-50.0), -SWIMMING_TERMINAL_VELOCITY);
    }

    #[test]
    fn test_swim_up_velocity_is_reasonable() {
        // Should be less than jump velocity (can't jump as high from water)
        assert!(SWIM_UP_VELOCITY < crate::physics::JUMP_VELOCITY);
        // But still positive and meaningful
        assert!(SWIM_UP_VELOCITY > 1.0);
    }

    #[test]
    fn test_constants_sanity() {
        assert!(SWIMMING_GRAVITY_MULTIPLIER > 0.0);
        assert!(SWIMMING_GRAVITY_MULTIPLIER < 1.0);
        assert!(SWIMMING_SPEED_MULTIPLIER > 0.0);
        assert!(SWIMMING_SPEED_MULTIPLIER < 1.0);
        assert!(WATER_DRAG > 0.0);
        assert!(BUOYANCY_ACCELERATION > 0.0);
    }
}