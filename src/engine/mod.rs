//! Core engine systems - camera, input, rendering utilities
//!
//! This module handles:
//! - Input action system (keyboard/gamepad abstraction)
//! - Player controller (movement, camera rotation)
//! - Cursor capture (FPS-style)
//! - Flying/noclip mode toggles
//!
//! # Module Structure
//!
//! - `input` - Input mapping and action system
//! - `controller` - Player movement and camera systems
//!
//! # System Order (Phase 3 Architecture)
//!
//! ```text
//! PreUpdate: InputPlugin (raw input → actions)
//!     ↓
//! Update: ControllerInputSet (rotation + movement intent)
//!     ↓
//! Update: PhysicsSet (gravity, collision on Player entity)
//!     ↓
//! Update: ControllerSyncSet (smooth rotation, position via parent hierarchy)
//! ```
//!
//! The camera is a CHILD of the Player entity. Position follows
//! automatically via Bevy's transform propagation.

use bevy::prelude::*;

// Submodules
pub mod controller;
pub mod input;
pub mod lighting;
pub mod memory;
pub mod metrics;
pub mod post_processing;
pub mod profiler;
pub mod raycast;

// Re-export commonly used items for backward compatibility
// These re-exports maintain the public API even if not used internally
#[allow(unused_imports)]
pub use controller::{
    CameraController,
    ControllerInputSet as CameraInputSet,
    ControllerSyncSet as CameraSyncSet,
    CursorState,
};

#[allow(unused_imports)]
pub use input::{
    ActionStates,
    ActionState,
    InputAction,
    InputBinding,
    InputMap,
};

#[allow(unused_imports)]
pub use profiler::{
    ChunkMetricsSnapshot,
    ProfilerPlugin,
    ProfilerState,
};

#[allow(unused_imports)]
pub use metrics::{
    MetricsPlugin,
    MetricWarnings,
    PerformanceMetrics,
    PerformanceOverlay,
    PerformanceThresholds,
    WarningLevel,
};

#[allow(unused_imports)]
pub use raycast::{
    CurrentTarget,
    RaycastPlugin,
    RaycastResult,
};

// ============================================================================
// PLUGIN
// ============================================================================

/// Combined plugin for camera and movement input systems
///
/// This is the main plugin to add - it includes both InputPlugin and ControllerPlugin.
pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        // Add both input and controller plugins
        app.add_plugins((
            input::InputPlugin,
            controller::ControllerPlugin,
        ));
    }
}
