//! Player-specific components and spawning
//!
//! The player is an Actor with:
//! - CapsuleCollider for physics
//! - Camera as a child entity (at eye height)
//! - Input handling for movement

use bevy::prelude::*;

use super::{Actor, CapsuleCollider, Grounded, Movement, Player, Velocity};
use crate::engine::CameraController;

/// Bundle for spawning a player entity
///
/// The player's Transform.translation is the FEET position.
/// The camera is spawned as a child at eye height offset.
#[derive(Bundle)]
pub struct PlayerBundle {
    // Markers
    pub actor: Actor,
    pub player: Player,

    // Spatial (required for parent-child hierarchy)
    pub transform: Transform,
    pub global_transform: GlobalTransform,
    pub visibility: Visibility,
    pub inherited_visibility: InheritedVisibility,
    pub view_visibility: ViewVisibility,

    // Physics
    pub velocity: Velocity,
    pub collider: CapsuleCollider,
    pub grounded: Grounded,

    // Movement capabilities
    pub movement: Movement,
}

impl Default for PlayerBundle {
    fn default() -> Self {
        Self {
            actor: Actor,
            player: Player,
            transform: Transform::from_xyz(32.0, 50.0, 32.0),
            global_transform: GlobalTransform::default(),
            visibility: Visibility::default(),
            inherited_visibility: InheritedVisibility::default(),
            view_visibility: ViewVisibility::default(),
            velocity: Velocity::default(),
            collider: CapsuleCollider::player(),
            grounded: Grounded::default(),
            movement: Movement::default(),
        }
    }
}

impl PlayerBundle {
    /// Create a player at a specific position
    #[allow(dead_code)]
    pub fn at_position(x: f32, y: f32, z: f32) -> Self {
        Self {
            transform: Transform::from_xyz(x, y, z),
            ..default()
        }
    }

    /// Create a player with flying enabled
    #[allow(dead_code)]
    pub fn flying() -> Self {
        Self {
            movement: Movement {
                flying: true,
                ..default()
            },
            ..default()
        }
    }
}

/// Camera bundle for the player's viewpoint
///
/// Spawned as a child of the player entity.
#[derive(Bundle)]
pub struct PlayerCameraBundle {
    pub camera: Camera3d,
    pub transform: Transform,
    pub controller: CameraController,
}

impl Default for PlayerCameraBundle {
    fn default() -> Self {
        Self {
            camera: Camera3d::default(),
            // Offset from player feet to eye level
            transform: Transform::from_xyz(0.0, CapsuleCollider::EYE_HEIGHT, 0.0),
            controller: CameraController::default(),
        }
    }
}

/// Spawn a complete player with camera
///
/// Returns the player entity ID.
///
/// # Example
/// ```ignore
/// fn setup(mut commands: Commands) {
///     let player = spawn_player(&mut commands, Vec3::new(32.0, 50.0, 32.0));
/// }
/// ```
pub fn spawn_player(commands: &mut Commands, position: Vec3) -> Entity {
    commands
        .spawn(PlayerBundle {
            transform: Transform::from_translation(position),
            ..default()
        })
        .with_children(|parent| {
            parent.spawn(PlayerCameraBundle::default());
        })
        .id()
}

/// Spawn a player in flying mode (for creative/editor use)
#[allow(dead_code)]
pub fn spawn_player_flying(commands: &mut Commands, position: Vec3) -> Entity {
    commands
        .spawn(PlayerBundle {
            transform: Transform::from_translation(position),
            movement: Movement {
                flying: true,
                ..default()
            },
            ..default()
        })
        .with_children(|parent| {
            parent.spawn(PlayerCameraBundle::default());
        })
        .id()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_player_bundle_default() {
        let bundle = PlayerBundle::default();
        assert_eq!(bundle.transform.translation, Vec3::new(32.0, 50.0, 32.0));
        assert!(!bundle.movement.flying);
        assert!(!bundle.movement.noclip);
    }

    #[test]
    fn test_player_bundle_at_position() {
        let bundle = PlayerBundle::at_position(100.0, 200.0, 300.0);
        assert_eq!(bundle.transform.translation, Vec3::new(100.0, 200.0, 300.0));
    }

    #[test]
    fn test_player_bundle_flying() {
        let bundle = PlayerBundle::flying();
        assert!(bundle.movement.flying);
    }

    #[test]
    fn test_player_camera_bundle_offset() {
        let bundle = PlayerCameraBundle::default();
        assert_eq!(bundle.transform.translation.y, CapsuleCollider::EYE_HEIGHT);
        assert_eq!(bundle.transform.translation.x, 0.0);
        assert_eq!(bundle.transform.translation.z, 0.0);
    }
}
