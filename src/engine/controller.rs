//! Player controller - movement and camera systems
//!
//! Handles translating input actions into player movement and camera rotation.
//! Separate from raw input handling (see input.rs).
//!
//! # Architecture
//!
//! ```text
//! InputAction (from input.rs)
//!     ↓
//! PlayerController systems
//!     ↓
//! Velocity component (consumed by physics)
//! ```

use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, PrimaryWindow};
use bevy_egui::EguiContexts;

use crate::actors::{Movement, Player, Velocity};
use crate::physics::PlayerPhysics;

use super::input::{ActionStates, InputAction};

// ============================================================================
// COMPONENTS
// ============================================================================

/// Camera controller component
///
/// Handles mouse look and rotation smoothing.
/// Attached to the camera entity (child of player).
#[derive(Component)]
pub struct CameraController {
    /// Mouse sensitivity for rotation (degrees per pixel)
    pub sensitivity: f32,
    
    /// Smoothing factor for rotation interpolation
    pub rotation_smoothing: f32,
    
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
            sensitivity: 0.1,         // Degrees per pixel
            rotation_smoothing: 50.0, // Very responsive rotation
            target_yaw: 0.0,
            target_pitch: 0.0,
            initialized: false,
        }
    }
}

impl CameraController {
    /// Get the current rotation as a quaternion
    pub fn rotation(&self) -> Quat {
        Quat::from_euler(
            EulerRot::YXZ,
            self.target_yaw.to_radians(),
            self.target_pitch.to_radians(),
            0.0,
        )
    }
    
    /// Get forward direction vector based on camera rotation
    pub fn forward(&self) -> Vec3 {
        self.rotation() * Vec3::NEG_Z
    }
    
    /// Get right direction vector based on camera rotation
    pub fn right(&self) -> Vec3 {
        self.rotation() * Vec3::X
    }
    
    /// Get forward direction projected onto horizontal plane (for walking)
    pub fn horizontal_forward(&self) -> Vec3 {
        let forward = self.forward();
        let horizontal = Vec3::new(forward.x, 0.0, forward.z);
        if horizontal.length_squared() > 0.001 {
            horizontal.normalize()
        } else {
            Vec3::NEG_Z
        }
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
// PLUGIN
// ============================================================================

/// System set for controller input (runs BEFORE physics)
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ControllerInputSet;

/// System set for controller sync (runs AFTER physics)
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ControllerSyncSet;

/// Plugin for player controller systems
pub struct ControllerPlugin;

impl Plugin for ControllerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CursorState>()
            // Input runs BEFORE physics
            .configure_sets(Update, ControllerInputSet.before(crate::physics::PhysicsSet))
            // Sync runs AFTER physics
            .configure_sets(Update, ControllerSyncSet.after(crate::physics::PhysicsSet))
            .add_systems(Startup, setup_cursor_grab)
            .add_systems(
                Update,
                (
                    cursor_grab_system,
                    camera_rotation_system,
                    movement_mode_system,
                    player_movement_system,
                )
                    .chain()
                    .in_set(ControllerInputSet),
            )
            .add_systems(
                Update,
                camera_sync_system.in_set(ControllerSyncSet),
            );
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

/// Handle cursor grab/release
fn cursor_grab_system(
    actions: Res<ActionStates>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut cursor_state: ResMut<CursorState>,
    mut egui_contexts: EguiContexts,
) {
    let Ok(mut window) = windows.get_single_mut() else {
        return;
    };

    // ESC releases cursor
    if actions.just_pressed(InputAction::ReleaseCursor) {
        window.cursor_options.grab_mode = CursorGrabMode::None;
        window.cursor_options.visible = true;
        cursor_state.grabbed = false;
    }

    // Click to recapture - only if not clicking on UI
    if mouse_button.just_pressed(MouseButton::Left) && !cursor_state.grabbed {
        // Check if egui wants this click (pointer is over a UI element)
        let egui_wants_pointer = egui_contexts
            .try_ctx_mut()
            .map(|ctx| ctx.is_pointer_over_area())
            .unwrap_or(false);

        if !egui_wants_pointer {
            window.cursor_options.grab_mode = CursorGrabMode::Locked;
            window.cursor_options.visible = false;
            cursor_state.grabbed = true;
        }
    }
}

/// Camera rotation system - handles mouse look
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

/// Handle flying/noclip mode toggles
fn movement_mode_system(
    actions: Res<ActionStates>,
    mut player_query: Query<&mut Movement, With<Player>>,
    mut physics: Option<ResMut<PlayerPhysics>>,
) {
    // Toggle fly mode
    if actions.just_pressed(InputAction::ToggleFly) {
        for mut movement in &mut player_query {
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
    
    // Toggle noclip mode
    if actions.just_pressed(InputAction::ToggleNoclip) {
        for mut movement in &mut player_query {
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
}

/// Player movement system - translates input actions to velocity
fn player_movement_system(
    actions: Res<ActionStates>,
    camera_query: Query<&CameraController, With<Camera3d>>,
    mut player_query: Query<(&mut Velocity, &Movement), With<Player>>,
) {
    let Ok(camera) = camera_query.get_single() else {
        return;
    };

    for (mut velocity, movement) in &mut player_query {
        // Get direction vectors from camera
        let forward = if movement.flying {
            camera.forward()
        } else {
            camera.horizontal_forward()
        };
        let right = camera.right();
        let up = Vec3::Y;

        // Determine speed
        let base_speed = if movement.flying {
            movement.fly_speed
        } else {
            movement.walk_speed
        };
        let speed = if actions.is_active(InputAction::Sprint) {
            base_speed * 1.3 // Sprint multiplier
        } else {
            base_speed
        };

        // Build movement direction from actions
        let mut move_dir = Vec3::ZERO;
        
        if actions.is_active(InputAction::MoveForward) {
            move_dir += forward;
        }
        if actions.is_active(InputAction::MoveBackward) {
            move_dir -= forward;
        }
        if actions.is_active(InputAction::MoveLeft) {
            move_dir -= right;
        }
        if actions.is_active(InputAction::MoveRight) {
            move_dir += right;
        }

        // Vertical movement when flying
        if movement.flying {
            if actions.is_active(InputAction::Jump) {
                move_dir += up;
            }
            if actions.is_active(InputAction::Crouch) {
                move_dir -= up;
            }
        }

        // Apply velocity
        if move_dir != Vec3::ZERO {
            move_dir = move_dir.normalize();
            
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
        let target_rotation = controller.rotation();
        let rot_factor = 1.0 - (-controller.rotation_smoothing * delta).exp();
        transform.rotation = transform.rotation.slerp(target_rotation, rot_factor);
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_camera_controller_default() {
        let controller = CameraController::default();
        assert_eq!(controller.sensitivity, 0.1);
        assert_eq!(controller.target_yaw, 0.0);
        assert_eq!(controller.target_pitch, 0.0);
    }
    
    #[test]
    fn test_camera_controller_forward() {
        let mut controller = CameraController::default();
        controller.target_yaw = 0.0;
        controller.target_pitch = 0.0;
        
        let forward = controller.forward();
        // Looking along -Z by default
        assert!((forward.z - -1.0).abs() < 0.01);
        assert!(forward.x.abs() < 0.01);
        assert!(forward.y.abs() < 0.01);
    }
    
    #[test]
    fn test_camera_controller_horizontal_forward() {
        let mut controller = CameraController::default();
        controller.target_yaw = 0.0;
        controller.target_pitch = -45.0; // Looking down
        
        let horizontal = controller.horizontal_forward();
        // Should still be along -Z when projected
        assert!((horizontal.z - -1.0).abs() < 0.01);
        assert!(horizontal.x.abs() < 0.01);
        assert_eq!(horizontal.y, 0.0); // No vertical component
    }
}
