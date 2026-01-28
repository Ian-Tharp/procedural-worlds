//! Core engine systems - camera, input, rendering utilities
//!
//! This module handles:
//! - Camera rotation (mouse look)
//! - Movement input (WASD) → sets player velocity
//! - Cursor capture (FPS-style)
//! - Flying/noclip mode toggles
//!
//! # System Order (Phase 3 Architecture)
//!
//! ```text
//! CameraInputSet (rotation + movement intent)
//!     ↓
//! PhysicsSet (gravity, collision on Player entity)
//!     ↓
//! CameraSyncSet (smooth rotation, position via parent hierarchy)
//! ```
//!
//! The camera is a CHILD of the Player entity. Position follows
//! automatically via Bevy's transform propagation.

use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, PrimaryWindow};

use crate::actors::{Movement, Player, Velocity};
use crate::physics::PlayerPhysics;

// ============================================================================
// PLUGIN
// ============================================================================

/// System set for camera input (runs BEFORE physics)
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CameraInputSet;

/// System set for camera sync (runs AFTER physics)
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CameraSyncSet;

/// Plugin for camera and movement input systems
pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CursorState>()
            // Input runs BEFORE physics
            .configure_sets(Update, CameraInputSet.before(crate::physics::PhysicsSet))
            // Sync runs AFTER physics
            .configure_sets(Update, CameraSyncSet.after(crate::physics::PhysicsSet))
            .add_systems(Startup, setup_cursor_grab)
            .add_systems(
                Update,
                (
                    cursor_grab_system,
                    camera_rotation_system,
                    player_movement_input_system,
                )
                    .chain()
                    .in_set(CameraInputSet),
            )
            .add_systems(
                Update,
                camera_sync_system.in_set(CameraSyncSet),
            );
    }
}

// ============================================================================
// RESOURCES
// ============================================================================

/// Tracks cursor grab state for FPS controls
#[derive(Resource, Default)]
pub struct CursorState {
    /// Whether cursor is currently grabbed (FPS mode)
    pub grabbed: bool,
}

// ============================================================================
// COMPONENTS
// ============================================================================

/// Component for camera rotation control
///
/// Handles mouse look only. Position comes from parent entity (Player).
/// Movement speeds are on the Player's Movement component.
#[derive(Component)]
pub struct CameraController {
    // Mouse settings
    /// Mouse sensitivity for rotation (degrees per pixel)
    pub sensitivity: f32,

    // Smoothing
    /// Smoothing factor for rotation interpolation
    pub rotation_smoothing: f32,

    // Rotation state
    /// Target yaw angle (horizontal rotation) in degrees
    pub target_yaw: f32,
    /// Target pitch angle (vertical rotation) in degrees
    pub target_pitch: f32,

    /// Whether the controller has been initialized
    initialized: bool,
}

impl Default for CameraController {
    fn default() -> Self {
        Self {
            sensitivity: 0.1,        // Degrees per pixel
            rotation_smoothing: 50.0, // Very responsive rotation
            target_yaw: 0.0,
            target_pitch: 0.0,
            initialized: false,
        }
    }
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// Setup initial cursor grab on startup
fn setup_cursor_grab(
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut cursor_state: ResMut<CursorState>,
) {
    if let Ok(mut window) = windows.get_single_mut() {
        window.cursor_options.grab_mode = CursorGrabMode::Locked;
        window.cursor_options.visible = false;
        cursor_state.grabbed = true;
    }
}

/// Handle cursor grab/release with ESC key
fn cursor_grab_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut cursor_state: ResMut<CursorState>,
) {
    let Ok(mut window) = windows.get_single_mut() else {
        return;
    };

    // ESC releases cursor
    if keyboard.just_pressed(KeyCode::Escape) {
        window.cursor_options.grab_mode = CursorGrabMode::None;
        window.cursor_options.visible = true;
        cursor_state.grabbed = false;
    }

    // Click to recapture
    if mouse_button.just_pressed(MouseButton::Left) && !cursor_state.grabbed {
        window.cursor_options.grab_mode = CursorGrabMode::Locked;
        window.cursor_options.visible = false;
        cursor_state.grabbed = true;
    }
}

/// Camera rotation system - handles mouse look only
///
/// Runs BEFORE physics. Updates camera's target_yaw and target_pitch.
/// Position comes from parent (Player entity) via Bevy's hierarchy.
fn camera_rotation_system(
    mut mouse_motion: EventReader<MouseMotion>,
    mut query: Query<(&Transform, &mut CameraController), With<Camera3d>>,
    cursor_state: Res<CursorState>,
) {
    for (transform, mut controller) in &mut query {
        // Initialize rotation from current transform on first frame
        if !controller.initialized {
            let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
            controller.target_yaw = yaw.to_degrees();
            controller.target_pitch = pitch.to_degrees();
            controller.initialized = true;
        }

        // Only process mouse input when cursor is grabbed
        if cursor_state.grabbed {
            let mut mouse_delta = Vec2::ZERO;
            for event in mouse_motion.read() {
                mouse_delta += event.delta;
            }

            if mouse_delta != Vec2::ZERO {
                controller.target_yaw -= mouse_delta.x * controller.sensitivity;
                controller.target_pitch -= mouse_delta.y * controller.sensitivity;
                controller.target_pitch = controller.target_pitch.clamp(-89.0, 89.0);
            }
        } else {
            mouse_motion.clear();
        }
    }
}

/// Player movement input system - WASD sets velocity on Player entity
///
/// Runs BEFORE physics. Reads keyboard input and camera rotation to
/// determine movement direction, then sets the player's velocity.
fn player_movement_input_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    camera_query: Query<&CameraController, With<Camera3d>>,
    mut player_query: Query<(&mut Velocity, &mut Movement, &Transform), With<Player>>,
    mut physics: Option<ResMut<PlayerPhysics>>,
) {

    // Handle flying/noclip toggles (update both Movement and legacy PlayerPhysics)
    if keyboard.just_pressed(KeyCode::KeyF) {
        for (_, mut movement, _) in &mut player_query {
            movement.flying = !movement.flying;
            if movement.flying {
                info!("Flying mode enabled");
            } else {
                info!("Flying mode disabled - gravity active");
            }
        }
        // Sync to legacy resource
        if let Some(ref mut phys) = physics {
            phys.flying = !phys.flying;
        }
    }
    if keyboard.just_pressed(KeyCode::KeyN) {
        for (_, mut movement, _) in &mut player_query {
            movement.noclip = !movement.noclip;
            if movement.noclip {
                info!("Noclip enabled");
            } else {
                info!("Noclip disabled");
            }
        }
        // Sync to legacy resource
        if let Some(ref mut phys) = physics {
            phys.noclip = !phys.noclip;
        }
    }

    // Get camera rotation for movement direction
    let Ok(camera) = camera_query.get_single() else {
        return;
    };

    let target_rotation = Quat::from_euler(
        EulerRot::YXZ,
        camera.target_yaw.to_radians(),
        camera.target_pitch.to_radians(),
        0.0,
    );

    for (mut velocity, movement, _transform) in &mut player_query {
        let forward_3d = target_rotation * Vec3::NEG_Z;
        let right = target_rotation * Vec3::X;
        let up = Vec3::Y;

        // For walking, project forward onto horizontal plane
        let forward = if movement.flying {
            forward_3d
        } else {
            let horizontal = Vec3::new(forward_3d.x, 0.0, forward_3d.z);
            if horizontal.length_squared() > 0.001 {
                horizontal.normalize()
            } else {
                Vec3::NEG_Z
            }
        };

        // Determine speed
        let base_speed = if movement.flying {
            movement.fly_speed
        } else {
            movement.walk_speed
        };
        let speed = if keyboard.pressed(KeyCode::ShiftLeft) {
            base_speed * 1.3 // Sprint multiplier
        } else {
            base_speed
        };

        // Build movement direction
        let mut move_dir = Vec3::ZERO;
        if keyboard.pressed(KeyCode::KeyW) {
            move_dir += forward;
        }
        if keyboard.pressed(KeyCode::KeyS) {
            move_dir -= forward;
        }
        if keyboard.pressed(KeyCode::KeyA) {
            move_dir -= right;
        }
        if keyboard.pressed(KeyCode::KeyD) {
            move_dir += right;
        }

        // Vertical movement when flying
        if movement.flying {
            if keyboard.pressed(KeyCode::Space) {
                move_dir += up;
            }
            if keyboard.pressed(KeyCode::ControlLeft) {
                move_dir -= up;
            }
        }

        // Set velocity (physics will apply this to position)
        if move_dir != Vec3::ZERO {
            move_dir = move_dir.normalize();
            // For horizontal movement, set velocity directly
            // Vertical velocity is handled by physics (gravity, jumping)
            if movement.flying {
                velocity.linear = move_dir * speed;
            } else {
                // Walking: only set horizontal velocity, preserve Y from physics
                velocity.linear.x = move_dir.x * speed;
                velocity.linear.z = move_dir.z * speed;
            }
        } else {
            // No input: stop horizontal movement
            velocity.linear.x = 0.0;
            velocity.linear.z = 0.0;
            if movement.flying {
                velocity.linear.y = 0.0;
            }
        }

        // Apply movement to position directly (physics will handle collision)
        // This is temporary until physics fully uses Velocity component
    }
}

/// Camera sync system - applies smooth rotation
///
/// Runs AFTER physics. Position comes automatically from parent (Player)
/// via Bevy's transform propagation. This system only handles rotation.
fn camera_sync_system(
    mut query: Query<(&mut Transform, &CameraController), With<Camera3d>>,
    time: Res<Time>,
) {
    let delta = time.delta_secs();

    for (mut transform, controller) in &mut query {
        // ================================================================
        // ROTATION (smooth interpolation)
        // ================================================================
        let target_rotation = Quat::from_euler(
            EulerRot::YXZ,
            controller.target_yaw.to_radians(),
            controller.target_pitch.to_radians(),
            0.0,
        );

        let rot_factor = 1.0 - (-controller.rotation_smoothing * delta).exp();
        transform.rotation = transform.rotation.slerp(target_rotation, rot_factor);

        // Position is handled by parent hierarchy - camera is child of Player
        // The camera's local transform is just the eye offset (0, 1.62, 0)
        // GlobalTransform combines this with player's world position automatically
    }
}
