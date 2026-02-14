//! Actor system - entities with physical presence in the game world
//!
//! This module provides the component-based "actor" abstraction for Bevy ECS.
//! Instead of OOP inheritance, we use composition:
//! - Marker components identify what an entity IS (Player, Npc, Mob)
//! - Data components define what an entity HAS (Velocity, Collider)
//! - Systems operate on component combinations
//!
//! # Architecture
//!
//! ```text
//! Player Entity
//! ├── Actor (marker: is a game actor)
//! ├── Player (marker: is the player)
//! ├── Transform (position - at feet)
//! ├── Velocity (movement)
//! ├── CapsuleCollider (collision shape)
//! ├── Movement (capabilities)
//! └── Grounded (ground state)
//!     └── Child: Camera3d + CameraController
//! ```

use bevy::prelude::*;

pub mod player;

// Re-export commonly used items
#[allow(unused_imports)]
pub use player::spawn_player_flying;

// These are used but re-exported for public API completeness
#[allow(unused_imports)]
pub use player::{PlayerBundle, PlayerCameraBundle, spawn_player};

/// Plugin for actor systems
pub struct ActorPlugin;

impl Plugin for ActorPlugin {
    fn build(&self, _app: &mut App) {
        // Actor systems live here. Debug UI should query actor state directly
        // (avoid coupling `actors` → `editor`).
    }
}

// ============================================================================
// MARKER COMPONENTS
// ============================================================================

/// Marker: Entity is a game actor with physical presence
///
/// All actors have:
/// - A position in the world (Transform)
/// - Some form of collision (CapsuleCollider, BoxCollider, etc.)
/// - Velocity for physics
#[derive(Component, Default, Debug)]
pub struct Actor;

/// Marker: Entity is the player
///
/// There should only be one entity with this component.
/// Player-specific systems query for this marker.
#[derive(Component, Default, Debug)]
pub struct Player;

/// Marker: Entity is an NPC (non-player character)
///
/// NPCs can have AI, dialogue, and interact with the player.
#[derive(Component, Default, Debug)]
#[allow(dead_code)]
pub struct Npc;

/// Marker: Entity is a mob (mobile entity - enemy or creature)
///
/// Mobs have AI and may be hostile or neutral.
#[derive(Component, Default, Debug)]
#[allow(dead_code)]
pub struct Mob;

// ============================================================================
// DATA COMPONENTS
// ============================================================================

/// Velocity for physics simulation
///
/// Separate from Transform to allow physics systems to accumulate
/// forces before applying to position.
#[derive(Component, Default, Debug, Clone)]
pub struct Velocity {
    /// Linear velocity in blocks per second
    pub linear: Vec3,
}

impl Velocity {
    #[allow(dead_code)]
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self {
            linear: Vec3::new(x, y, z),
        }
    }
}

/// Movement capabilities for an actor
///
/// Defines what the actor CAN do, not what it's currently doing.
#[derive(Component, Debug, Clone)]
pub struct Movement {
    /// Walking speed in blocks per second
    pub walk_speed: f32,
    /// Sprinting speed in blocks per second (for future sprint implementation)
    #[allow(dead_code)]
    pub sprint_speed: f32,
    /// Flying speed in blocks per second
    pub fly_speed: f32,
    /// Initial jump velocity in blocks per second (for future jump improvements)
    #[allow(dead_code)]
    pub jump_velocity: f32,
    /// Maximum height for auto-stepping up blocks (for future step implementation)
    #[allow(dead_code)]
    pub step_height: f32,
    /// Whether flying mode is enabled
    pub flying: bool,
    /// Whether noclip mode is enabled (ignores collision)
    pub noclip: bool,
}

impl Default for Movement {
    fn default() -> Self {
        Self {
            walk_speed: 4.3,    // Minecraft walking
            sprint_speed: 5.6,  // Minecraft sprinting
            fly_speed: 11.0,    // Minecraft creative fly
            jump_velocity: 8.4, // ~1.25 block jump height
            step_height: 0.6,   // Auto-step up to 0.6 blocks
            flying: false,
            noclip: false,
        }
    }
}

/// Ground contact state
///
/// Tracks whether the actor is standing on something solid.
#[derive(Component, Default, Debug, Clone)]
pub struct Grounded {
    /// Whether currently touching ground
    pub is_grounded: bool,
    /// Normal of the surface we're standing on (for future slope handling)
    #[allow(dead_code)]
    pub ground_normal: Vec3,
    /// Time since last grounded (for coyote time)
    pub time_since_grounded: f32,
    /// Time since last jump input (for jump buffering)
    #[allow(dead_code)]
    pub jump_buffer_time: f32,
}

#[allow(dead_code)]
impl Grounded {
    /// Coyote time window - can still jump briefly after leaving ground
    pub const COYOTE_TIME: f32 = 0.1;
    /// Jump buffer window - can press jump slightly before landing
    pub const JUMP_BUFFER: f32 = 0.1;

    /// Can the actor jump right now?
    pub fn can_jump(&self) -> bool {
        self.is_grounded || self.time_since_grounded < Self::COYOTE_TIME
    }

    /// Is there a buffered jump input?
    pub fn has_buffered_jump(&self) -> bool {
        self.jump_buffer_time > 0.0
    }
}

/// Capsule collider for actor collision
///
/// The capsule is oriented vertically (Y-up).
/// Position is at the FEET (bottom of capsule).
///
/// ```text
///       ___
///      /   \    <- top hemisphere (y = height - radius)
///     |     |
///     |     |   <- cylinder
///     |     |
///      \___/    <- bottom hemisphere (y = radius)
///        ^
///        └── position (feet)
/// ```
#[derive(Component, Debug, Clone)]
pub struct CapsuleCollider {
    /// Radius of the capsule (and hemispheres)
    pub radius: f32,
    /// Total height from feet to top
    pub height: f32,
}

impl Default for CapsuleCollider {
    fn default() -> Self {
        Self {
            radius: 0.3, // Player is 0.6 blocks wide
            height: 1.8, // Player is 1.8 blocks tall
        }
    }
}

impl CapsuleCollider {
    /// Eye height offset from feet (for camera placement)
    pub const EYE_HEIGHT: f32 = 1.62;

    /// Create a player-sized capsule
    pub fn player() -> Self {
        Self::default()
    }

    /// Get the Y position of the bottom sphere center (relative to feet)
    #[allow(dead_code)]
    pub fn bottom_sphere_y(&self) -> f32 {
        self.radius
    }

    /// Get the Y position of the top sphere center (relative to feet)
    #[allow(dead_code)]
    pub fn top_sphere_y(&self) -> f32 {
        self.height - self.radius
    }

    /// Get the AABB min corner (relative to feet position)
    #[allow(dead_code)]
    pub fn aabb_min(&self) -> Vec3 {
        Vec3::new(-self.radius, 0.0, -self.radius)
    }

    /// Get the AABB max corner (relative to feet position)
    #[allow(dead_code)]
    pub fn aabb_max(&self) -> Vec3 {
        Vec3::new(self.radius, self.height, self.radius)
    }

    /// Check if a point is inside the capsule (in capsule-local space)
    #[allow(dead_code)]
    pub fn contains_point(&self, local_point: Vec3) -> bool {
        // Clamp Y to the cylinder section
        let clamped_y = local_point.y.clamp(self.radius, self.height - self.radius);
        let closest_on_axis = Vec3::new(0.0, clamped_y, 0.0);
        let distance = local_point.distance(closest_on_axis);
        distance <= self.radius
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capsule_dimensions() {
        let capsule = CapsuleCollider::default();
        assert_eq!(capsule.radius, 0.3);
        assert_eq!(capsule.height, 1.8);
        assert_eq!(capsule.bottom_sphere_y(), 0.3);
        assert_eq!(capsule.top_sphere_y(), 1.5);
    }

    #[test]
    fn test_capsule_aabb() {
        let capsule = CapsuleCollider::default();
        let min = capsule.aabb_min();
        let max = capsule.aabb_max();

        assert_eq!(min, Vec3::new(-0.3, 0.0, -0.3));
        assert_eq!(max, Vec3::new(0.3, 1.8, 0.3));
    }

    #[test]
    fn test_capsule_contains_point() {
        let capsule = CapsuleCollider::default();

        // Center of capsule should be inside
        assert!(capsule.contains_point(Vec3::new(0.0, 0.9, 0.0)));

        // Point at feet center
        assert!(capsule.contains_point(Vec3::new(0.0, 0.3, 0.0)));

        // Point outside radius
        assert!(!capsule.contains_point(Vec3::new(0.5, 0.9, 0.0)));

        // Point above capsule
        assert!(!capsule.contains_point(Vec3::new(0.0, 2.0, 0.0)));

        // Point below capsule
        assert!(!capsule.contains_point(Vec3::new(0.0, -0.1, 0.0)));
    }

    #[test]
    fn test_movement_defaults() {
        let movement = Movement::default();
        assert_eq!(movement.walk_speed, 4.3);
        assert_eq!(movement.sprint_speed, 5.6);
        assert_eq!(movement.fly_speed, 11.0);
        assert_eq!(movement.jump_velocity, 8.4);
        assert_eq!(movement.step_height, 0.6);
        assert!(!movement.flying);
        assert!(!movement.noclip);
    }

    #[test]
    fn test_grounded_can_jump() {
        let mut grounded = Grounded::default();

        // Grounded = can jump
        grounded.is_grounded = true;
        grounded.time_since_grounded = 0.0;
        assert!(grounded.can_jump());

        // Just left ground (coyote time)
        grounded.is_grounded = false;
        grounded.time_since_grounded = 0.05;
        assert!(grounded.can_jump());

        // Too long since grounded
        grounded.time_since_grounded = 0.2;
        assert!(!grounded.can_jump());
    }

    #[test]
    fn test_grounded_jump_buffer() {
        let mut grounded = Grounded::default();

        grounded.jump_buffer_time = 0.0;
        assert!(!grounded.has_buffered_jump());

        grounded.jump_buffer_time = 0.05;
        assert!(grounded.has_buffered_jump());
    }

    #[test]
    fn test_velocity_new() {
        let vel = Velocity::new(1.0, 2.0, 3.0);
        assert_eq!(vel.linear, Vec3::new(1.0, 2.0, 3.0));
    }
}
