//! Procedural Worlds Engine
//!
//! An AI-driven voxel game engine for collaborative world-building.
//! Built with Bevy ECS and wgpu rendering.

use bevy::prelude::*;
use bevy_egui::EguiPlugin;

use procedural_worlds::actors;
use procedural_worlds::config;
use procedural_worlds::editor;
use procedural_worlds::engine;
use procedural_worlds::physics;
use procedural_worlds::world;

fn main() {
    // Load engine configuration (creates default config.json if none exists)
    let engine_config = config::EngineConfig::load_or_default();

    // Select present mode from config
    let present_mode = if engine_config.window.vsync {
        bevy::window::PresentMode::AutoVsync
    } else {
        bevy::window::PresentMode::AutoNoVsync
    };

    App::new()
        // Configure default plugins with config-driven window settings
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: engine_config.window.title.clone(),
                resolution: (engine_config.window.width, engine_config.window.height).into(),
                present_mode,
                ..default()
            }),
            ..default()
        }))
        // Editor UI plugin
        .add_plugins(EguiPlugin)
        // Insert engine config as a resource (before plugins that read it)
        .insert_resource(engine_config)
        // Our custom plugins
        .add_plugins(editor::EditorPlugin)
        .add_plugins(editor::DebugOverlayPlugin)
        .add_plugins(editor::ChunkDebugPlugin)
        .add_plugins(editor::HudPlugin)
        .add_plugins(engine::CameraPlugin)
        .add_plugins(engine::RaycastPlugin)
        .add_plugins(engine::lighting::DayNightPlugin)
        .add_plugins(engine::post_processing::PostProcessingPlugin)
        .add_plugins(world::WorldPlugin)
        .add_plugins(physics::PhysicsPlugin)
        .add_plugins(actors::ActorPlugin)
        // Config plugin applies settings to resources/entities in PostStartup
        .add_plugins(config::ConfigPlugin)
        // Startup systems
        .add_systems(Startup, setup_scene)
        .run();
}

/// Initial scene setup - creates player entity with camera, and lighting
fn setup_scene(mut commands: Commands) {
    info!("Procedural Worlds Engine v{}", env!("CARGO_PKG_VERSION"));
    info!("Setting up initial scene...");

    // Spawn player entity with camera as child
    // Position is FEET position, camera is offset by eye height (1.62)
    // Start in walking mode with gravity
    let player_feet_y = 64.0 - actors::CapsuleCollider::EYE_HEIGHT; // Eyes at 64
    let player_id = actors::spawn_player(
        &mut commands,
        Vec3::new(32.0, player_feet_y, 32.0),
    );
    info!("Spawned player entity: {:?}", player_id);

    // NOTE: DirectionalLight (sun) and AmbientLight are now managed by
    // DayNightPlugin — see engine::lighting

    info!("Scene setup complete! Player spawned, terrain will generate around you.");
    info!("Controls: WASD move, Mouse look, F toggle fly, N toggle noclip");
}
