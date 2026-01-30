//! Procedural Worlds Engine
//!
//! An AI-driven voxel game engine for collaborative world-building.
//! Built with Bevy ECS and wgpu rendering.

use bevy::prelude::*;
use bevy_egui::EguiPlugin;

mod actors;
mod config;
mod editor;
mod engine;
mod generation;
mod physics;
mod world;

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
        .add_plugins(engine::CameraPlugin)
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

    // Directional light (sun)
    commands.spawn((
        DirectionalLight {
            illuminance: 15000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.6, 0.4, 0.0)),
    ));

    // Ambient light for softer shadows
    commands.insert_resource(AmbientLight {
        color: Color::srgb(0.4, 0.4, 0.5),
        brightness: 200.0,
    });

    info!("Scene setup complete! Player spawned, terrain will generate around you.");
    info!("Controls: WASD move, Mouse look, F toggle fly, N toggle noclip");
}
