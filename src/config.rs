//! Engine configuration system
//!
//! Loads engine settings from a JSON config file at startup.
//! If no config file exists, creates one with sensible defaults.
//!
//! # Config File
//!
//! The engine looks for `config.json` in the working directory.
//! On first run, a default config file is created automatically.
//!
//! # Architecture
//!
//! ```text
//! config.json (on disk)
//!     ↓
//! EngineConfig (loaded at startup)
//!     ↓
//! ConfigPlugin (PostStartup)
//!     ↓
//! Applies to: InputMap, ChunkManager, TerrainConfig,
//!             DebugOverlayState, CameraController, Movement
//! ```
//!
//! # Adding New Settings
//!
//! 1. Add the field to the appropriate config section
//! 2. Set its default in the section's `Default` impl
//! 3. Apply it in `apply_config_to_resources` or `apply_config_to_entities`

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;

use crate::actors::{Movement, Player};
use crate::editor::debug_overlay::DebugOverlayState;
use crate::engine::controller::CameraController;
use crate::engine::input::{InputAction, InputMap};
use crate::generation::TerrainConfig;
use crate::world::ChunkManager;

// ============================================================================
// CONFIG FILE PATH
// ============================================================================

/// Default config file name
const CONFIG_FILE: &str = "config.json";

// ============================================================================
// CONFIG STRUCTS
// ============================================================================

/// Root engine configuration
///
/// All fields use `#[serde(default)]` for forward compatibility —
/// if the config file is missing new fields, they get default values.
#[derive(Resource, Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct EngineConfig {
    /// Window settings (title, resolution, vsync)
    pub window: WindowConfig,
    /// Rendering settings (render distance, chunk rate limiting)
    pub render: RenderConfig,
    /// Terrain generation settings (seed, noise parameters)
    pub terrain: TerrainSettings,
    /// Player movement settings (speeds, sensitivity)
    pub player: PlayerConfig,
    /// Keyboard/mouse bindings (key names as strings)
    pub controls: ControlsConfig,
    /// Debug overlay settings
    pub debug: DebugConfig,
    /// Duration of a full day/night cycle in seconds (default: 600 = 10 min)
    pub cycle_duration_seconds: f32,
}

/// Window configuration
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct WindowConfig {
    /// Window title
    pub title: String,
    /// Window width in pixels
    pub width: f32,
    /// Window height in pixels
    pub height: f32,
    /// Enable VSync (true = AutoVsync, false = AutoNoVsync)
    pub vsync: bool,
}

/// Rendering configuration
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct RenderConfig {
    /// Render distance in chunks (default: 4)
    pub render_distance: i32,
    /// Maximum chunks to generate per frame (default: 4)
    pub max_chunks_per_frame: u32,
}

/// Terrain generation settings
///
/// These map directly to `generation::TerrainConfig`.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct TerrainSettings {
    /// World seed for terrain generation
    pub seed: u32,
    /// Base terrain height in blocks
    pub base_height: f64,
    /// Height variation scale
    pub height_scale: f64,
    /// Base noise frequency (lower = smoother terrain)
    pub frequency: f64,
    /// Number of noise octaves (more = more detail)
    pub octaves: usize,
}

/// Player movement and camera settings
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct PlayerConfig {
    /// Walking speed in blocks per second
    pub walk_speed: f32,
    /// Sprinting speed in blocks per second
    pub sprint_speed: f32,
    /// Flying speed in blocks per second
    pub fly_speed: f32,
    /// Jump velocity in blocks per second
    pub jump_velocity: f32,
    /// Mouse sensitivity (degrees per pixel)
    pub mouse_sensitivity: f32,
}

/// Keyboard and mouse bindings
///
/// Key names use Bevy's `KeyCode` variant names as strings.
/// Common examples: `"KeyW"`, `"Space"`, `"ShiftLeft"`, `"Escape"`,
/// `"ArrowUp"`, `"Digit1"`, `"F1"`.
///
/// Invalid key names are logged as warnings and ignored.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct ControlsConfig {
    /// Move forward key (default: "KeyW")
    pub move_forward: String,
    /// Move backward key (default: "KeyS")
    pub move_backward: String,
    /// Strafe left key (default: "KeyA")
    pub move_left: String,
    /// Strafe right key (default: "KeyD")
    pub move_right: String,
    /// Jump key (default: "Space")
    pub jump: String,
    /// Crouch key (default: "ControlLeft")
    pub crouch: String,
    /// Sprint key (default: "ShiftLeft")
    pub sprint: String,
    /// Toggle fly mode key (default: "KeyF")
    pub toggle_fly: String,
    /// Toggle noclip mode key (default: "KeyN")
    pub toggle_noclip: String,
    /// Release cursor key (default: "Escape")
    pub release_cursor: String,
}

/// Debug overlay settings
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct DebugConfig {
    /// Whether the debug overlay is visible on startup
    pub overlay_visible: bool,
    /// Show memory usage panel
    pub show_memory: bool,
    /// Show input state panel
    pub show_input: bool,
}

// ============================================================================
// DEFAULT IMPLEMENTATIONS
// ============================================================================

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            window: WindowConfig::default(),
            render: RenderConfig::default(),
            terrain: TerrainSettings::default(),
            player: PlayerConfig::default(),
            controls: ControlsConfig::default(),
            debug: DebugConfig::default(),
            cycle_duration_seconds: 600.0,
        }
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "Procedural Worlds Engine".into(),
            width: 1280.0,
            height: 720.0,
            vsync: true,
        }
    }
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            render_distance: 4,
            max_chunks_per_frame: 4,
        }
    }
}

impl Default for TerrainSettings {
    fn default() -> Self {
        Self {
            seed: 12345,
            base_height: 32.0,
            height_scale: 16.0,
            frequency: 0.02,
            octaves: 4,
        }
    }
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            walk_speed: 4.3,
            sprint_speed: 5.6,
            fly_speed: 11.0,
            jump_velocity: 8.4,
            mouse_sensitivity: 0.1,
        }
    }
}

impl Default for ControlsConfig {
    fn default() -> Self {
        Self {
            move_forward: "KeyW".into(),
            move_backward: "KeyS".into(),
            move_left: "KeyA".into(),
            move_right: "KeyD".into(),
            jump: "Space".into(),
            crouch: "ControlLeft".into(),
            sprint: "ShiftLeft".into(),
            toggle_fly: "KeyF".into(),
            toggle_noclip: "KeyN".into(),
            release_cursor: "Escape".into(),
        }
    }
}

impl Default for DebugConfig {
    fn default() -> Self {
        Self {
            overlay_visible: true,
            show_memory: true,
            show_input: false,
        }
    }
}

// ============================================================================
// LOADING & SAVING
// ============================================================================

impl EngineConfig {
    /// Load config from disk, or create a default config file if none exists.
    ///
    /// If the config file is malformed, logs a warning and returns defaults.
    pub fn load_or_default() -> Self {
        let path = std::path::Path::new(CONFIG_FILE);

        if path.exists() {
            match fs::read_to_string(path) {
                Ok(contents) => match serde_json::from_str::<EngineConfig>(&contents) {
                    Ok(config) => {
                        info!("Loaded engine config from {}", CONFIG_FILE);
                        return config;
                    }
                    Err(e) => {
                        warn!(
                            "Failed to parse {}: {}. Using defaults.",
                            CONFIG_FILE, e
                        );
                    }
                },
                Err(e) => {
                    warn!(
                        "Failed to read {}: {}. Using defaults.",
                        CONFIG_FILE, e
                    );
                }
            }
        } else {
            info!("No config file found. Creating default {}.", CONFIG_FILE);
        }

        // Create and save default config
        let config = EngineConfig::default();
        if let Err(e) = config.save() {
            warn!("Failed to save default config: {}", e);
        }
        config
    }

    /// Save the current config to disk.
    pub fn save(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Serialization failed: {}", e))?;

        fs::write(CONFIG_FILE, json)
            .map_err(|e| format!("Failed to write {}: {}", CONFIG_FILE, e))?;

        Ok(())
    }
}

// ============================================================================
// KEY CODE MAPPING
// ============================================================================

/// Convert a string key name to a Bevy `KeyCode`.
///
/// Uses Bevy's `KeyCode` variant names (e.g., `"KeyW"`, `"Space"`, `"ShiftLeft"`).
/// Returns `None` for unrecognized strings.
fn string_to_keycode(s: &str) -> Option<KeyCode> {
    match s {
        // Letters
        "KeyA" => Some(KeyCode::KeyA),
        "KeyB" => Some(KeyCode::KeyB),
        "KeyC" => Some(KeyCode::KeyC),
        "KeyD" => Some(KeyCode::KeyD),
        "KeyE" => Some(KeyCode::KeyE),
        "KeyF" => Some(KeyCode::KeyF),
        "KeyG" => Some(KeyCode::KeyG),
        "KeyH" => Some(KeyCode::KeyH),
        "KeyI" => Some(KeyCode::KeyI),
        "KeyJ" => Some(KeyCode::KeyJ),
        "KeyK" => Some(KeyCode::KeyK),
        "KeyL" => Some(KeyCode::KeyL),
        "KeyM" => Some(KeyCode::KeyM),
        "KeyN" => Some(KeyCode::KeyN),
        "KeyO" => Some(KeyCode::KeyO),
        "KeyP" => Some(KeyCode::KeyP),
        "KeyQ" => Some(KeyCode::KeyQ),
        "KeyR" => Some(KeyCode::KeyR),
        "KeyS" => Some(KeyCode::KeyS),
        "KeyT" => Some(KeyCode::KeyT),
        "KeyU" => Some(KeyCode::KeyU),
        "KeyV" => Some(KeyCode::KeyV),
        "KeyW" => Some(KeyCode::KeyW),
        "KeyX" => Some(KeyCode::KeyX),
        "KeyY" => Some(KeyCode::KeyY),
        "KeyZ" => Some(KeyCode::KeyZ),

        // Digits
        "Digit0" => Some(KeyCode::Digit0),
        "Digit1" => Some(KeyCode::Digit1),
        "Digit2" => Some(KeyCode::Digit2),
        "Digit3" => Some(KeyCode::Digit3),
        "Digit4" => Some(KeyCode::Digit4),
        "Digit5" => Some(KeyCode::Digit5),
        "Digit6" => Some(KeyCode::Digit6),
        "Digit7" => Some(KeyCode::Digit7),
        "Digit8" => Some(KeyCode::Digit8),
        "Digit9" => Some(KeyCode::Digit9),

        // Arrow keys
        "ArrowUp" => Some(KeyCode::ArrowUp),
        "ArrowDown" => Some(KeyCode::ArrowDown),
        "ArrowLeft" => Some(KeyCode::ArrowLeft),
        "ArrowRight" => Some(KeyCode::ArrowRight),

        // Modifiers
        "ShiftLeft" => Some(KeyCode::ShiftLeft),
        "ShiftRight" => Some(KeyCode::ShiftRight),
        "ControlLeft" => Some(KeyCode::ControlLeft),
        "ControlRight" => Some(KeyCode::ControlRight),
        "AltLeft" => Some(KeyCode::AltLeft),
        "AltRight" => Some(KeyCode::AltRight),
        "SuperLeft" => Some(KeyCode::SuperLeft),
        "SuperRight" => Some(KeyCode::SuperRight),

        // Special keys
        "Space" => Some(KeyCode::Space),
        "Enter" => Some(KeyCode::Enter),
        "Escape" => Some(KeyCode::Escape),
        "Backspace" => Some(KeyCode::Backspace),
        "Tab" => Some(KeyCode::Tab),
        "Delete" => Some(KeyCode::Delete),
        "Insert" => Some(KeyCode::Insert),
        "Home" => Some(KeyCode::Home),
        "End" => Some(KeyCode::End),
        "PageUp" => Some(KeyCode::PageUp),
        "PageDown" => Some(KeyCode::PageDown),
        "CapsLock" => Some(KeyCode::CapsLock),

        // Function keys
        "F1" => Some(KeyCode::F1),
        "F2" => Some(KeyCode::F2),
        "F3" => Some(KeyCode::F3),
        "F4" => Some(KeyCode::F4),
        "F5" => Some(KeyCode::F5),
        "F6" => Some(KeyCode::F6),
        "F7" => Some(KeyCode::F7),
        "F8" => Some(KeyCode::F8),
        "F9" => Some(KeyCode::F9),
        "F10" => Some(KeyCode::F10),
        "F11" => Some(KeyCode::F11),
        "F12" => Some(KeyCode::F12),

        // Punctuation / misc
        "Comma" => Some(KeyCode::Comma),
        "Period" => Some(KeyCode::Period),
        "Semicolon" => Some(KeyCode::Semicolon),
        "Quote" => Some(KeyCode::Quote),
        "BracketLeft" => Some(KeyCode::BracketLeft),
        "BracketRight" => Some(KeyCode::BracketRight),
        "Backquote" => Some(KeyCode::Backquote),
        "Backslash" => Some(KeyCode::Backslash),
        "Minus" => Some(KeyCode::Minus),
        "Equal" => Some(KeyCode::Equal),
        "Slash" => Some(KeyCode::Slash),

        _ => None,
    }
}

/// Helper: bind a config key string to an input action, logging warnings for bad keys.
fn bind_from_config(input_map: &mut InputMap, action: InputAction, key_name: &str) {
    match string_to_keycode(key_name) {
        Some(keycode) => {
            input_map.bind(action, keycode);
        }
        None => {
            warn!(
                "Unknown key binding '{}' for action {:?}. Skipping.",
                key_name, action
            );
        }
    }
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that applies engine configuration to all game systems.
///
/// The `EngineConfig` resource must be inserted before this plugin runs.
/// Use `PostStartup` schedule to ensure all other plugins have initialized
/// their resources and entities first.
pub struct ConfigPlugin;

impl Plugin for ConfigPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostStartup,
            (apply_config_to_resources, apply_config_to_entities).chain(),
        );
    }
}

// ============================================================================
// CONFIG APPLICATION SYSTEMS
// ============================================================================

/// Apply config settings to engine resources (InputMap, ChunkManager, etc.)
///
/// Runs in `PostStartup` after all plugins have initialized their resources.
fn apply_config_to_resources(
    config: Res<EngineConfig>,
    mut input_map: ResMut<InputMap>,
    mut chunk_manager: ResMut<ChunkManager>,
    mut terrain_config: ResMut<TerrainConfig>,
    mut debug_state: ResMut<DebugOverlayState>,
) {
    info!("Applying engine configuration...");

    // --- Render settings ---
    chunk_manager.render_distance = config.render.render_distance;
    chunk_manager.max_chunks_per_frame = config.render.max_chunks_per_frame;
    info!(
        "Render distance: {} chunks, max {}/frame",
        config.render.render_distance, config.render.max_chunks_per_frame
    );

    // --- Terrain settings ---
    terrain_config.seed = config.terrain.seed;
    terrain_config.base_height = config.terrain.base_height;
    terrain_config.height_scale = config.terrain.height_scale;
    terrain_config.frequency = config.terrain.frequency;
    terrain_config.octaves = config.terrain.octaves;
    info!("Terrain seed: {}", config.terrain.seed);

    // --- Debug overlay settings ---
    debug_state.visible = config.debug.overlay_visible;
    debug_state.show_memory = config.debug.show_memory;
    debug_state.show_input_state = config.debug.show_input;

    // --- Input bindings ---
    // Clear default bindings and apply from config
    input_map.clear();

    bind_from_config(&mut input_map, InputAction::MoveForward, &config.controls.move_forward);
    bind_from_config(&mut input_map, InputAction::MoveBackward, &config.controls.move_backward);
    bind_from_config(&mut input_map, InputAction::MoveLeft, &config.controls.move_left);
    bind_from_config(&mut input_map, InputAction::MoveRight, &config.controls.move_right);
    bind_from_config(&mut input_map, InputAction::Jump, &config.controls.jump);
    bind_from_config(&mut input_map, InputAction::Crouch, &config.controls.crouch);
    bind_from_config(&mut input_map, InputAction::Sprint, &config.controls.sprint);
    bind_from_config(&mut input_map, InputAction::ToggleFly, &config.controls.toggle_fly);
    bind_from_config(&mut input_map, InputAction::ToggleNoclip, &config.controls.toggle_noclip);
    bind_from_config(&mut input_map, InputAction::ReleaseCursor, &config.controls.release_cursor);

    info!("Input bindings applied from config");
}

/// Apply config settings to player and camera entities.
///
/// Runs after `apply_config_to_resources` to ensure resources are ready.
/// Requires player and camera entities to have been spawned during `Startup`.
fn apply_config_to_entities(
    config: Res<EngineConfig>,
    mut camera_query: Query<&mut CameraController, With<Camera3d>>,
    mut player_query: Query<&mut Movement, With<Player>>,
) {
    // --- Camera sensitivity ---
    for mut controller in &mut camera_query {
        controller.sensitivity = config.player.mouse_sensitivity;
    }

    // --- Player movement settings ---
    for mut movement in &mut player_query {
        movement.walk_speed = config.player.walk_speed;
        movement.sprint_speed = config.player.sprint_speed;
        movement.fly_speed = config.player.fly_speed;
        movement.jump_velocity = config.player.jump_velocity;
    }

    info!("Player config applied (sensitivity: {}, walk: {}, fly: {})",
        config.player.mouse_sensitivity,
        config.player.walk_speed,
        config.player.fly_speed,
    );
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_matches_existing_defaults() {
        let config = EngineConfig::default();

        // Window defaults should match main.rs
        assert_eq!(config.window.title, "Procedural Worlds Engine");
        assert_eq!(config.window.width, 1280.0);
        assert_eq!(config.window.height, 720.0);
        assert!(config.window.vsync);

        // Render defaults should match ChunkManager::default()
        assert_eq!(config.render.render_distance, 4);
        assert_eq!(config.render.max_chunks_per_frame, 4);

        // Terrain defaults should match TerrainConfig::default()
        assert_eq!(config.terrain.seed, 12345);
        assert_eq!(config.terrain.base_height, 32.0);
        assert_eq!(config.terrain.height_scale, 16.0);
        assert_eq!(config.terrain.frequency, 0.02);
        assert_eq!(config.terrain.octaves, 4);

        // Player defaults should match Movement::default()
        assert_eq!(config.player.walk_speed, 4.3);
        assert_eq!(config.player.sprint_speed, 5.6);
        assert_eq!(config.player.fly_speed, 11.0);
        assert_eq!(config.player.jump_velocity, 8.4);

        // Mouse sensitivity should match CameraController::default()
        assert_eq!(config.player.mouse_sensitivity, 0.1);
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let original = EngineConfig::default();
        let json = serde_json::to_string_pretty(&original).unwrap();
        let deserialized: EngineConfig = serde_json::from_str(&json).unwrap();

        // Spot-check values survived roundtrip
        assert_eq!(deserialized.window.title, original.window.title);
        assert_eq!(deserialized.terrain.seed, original.terrain.seed);
        assert_eq!(deserialized.controls.move_forward, original.controls.move_forward);
        assert_eq!(deserialized.player.walk_speed, original.player.walk_speed);
        assert_eq!(deserialized.debug.overlay_visible, original.debug.overlay_visible);
    }

    #[test]
    fn test_config_partial_json_uses_defaults() {
        // Config with only some fields — missing ones should get defaults
        let partial_json = r#"{
            "window": {
                "title": "Custom Title"
            }
        }"#;

        let config: EngineConfig = serde_json::from_str(partial_json).unwrap();

        assert_eq!(config.window.title, "Custom Title");
        // Missing fields get defaults
        assert_eq!(config.window.width, 1280.0);
        assert_eq!(config.render.render_distance, 4);
        assert_eq!(config.terrain.seed, 12345);
        assert_eq!(config.controls.move_forward, "KeyW");
    }

    #[test]
    fn test_string_to_keycode_letters() {
        assert_eq!(string_to_keycode("KeyA"), Some(KeyCode::KeyA));
        assert_eq!(string_to_keycode("KeyZ"), Some(KeyCode::KeyZ));
        assert_eq!(string_to_keycode("KeyW"), Some(KeyCode::KeyW));
    }

    #[test]
    fn test_string_to_keycode_special() {
        assert_eq!(string_to_keycode("Space"), Some(KeyCode::Space));
        assert_eq!(string_to_keycode("Escape"), Some(KeyCode::Escape));
        assert_eq!(string_to_keycode("Enter"), Some(KeyCode::Enter));
        assert_eq!(string_to_keycode("ShiftLeft"), Some(KeyCode::ShiftLeft));
        assert_eq!(string_to_keycode("ControlLeft"), Some(KeyCode::ControlLeft));
    }

    #[test]
    fn test_string_to_keycode_arrows() {
        assert_eq!(string_to_keycode("ArrowUp"), Some(KeyCode::ArrowUp));
        assert_eq!(string_to_keycode("ArrowDown"), Some(KeyCode::ArrowDown));
        assert_eq!(string_to_keycode("ArrowLeft"), Some(KeyCode::ArrowLeft));
        assert_eq!(string_to_keycode("ArrowRight"), Some(KeyCode::ArrowRight));
    }

    #[test]
    fn test_string_to_keycode_function_keys() {
        assert_eq!(string_to_keycode("F1"), Some(KeyCode::F1));
        assert_eq!(string_to_keycode("F12"), Some(KeyCode::F12));
    }

    #[test]
    fn test_string_to_keycode_unknown() {
        assert_eq!(string_to_keycode(""), None);
        assert_eq!(string_to_keycode("InvalidKey"), None);
        assert_eq!(string_to_keycode("key_w"), None); // Case sensitive
    }

    #[test]
    fn test_default_controls_all_map_to_valid_keycodes() {
        let controls = ControlsConfig::default();

        // Every default binding should resolve to a valid KeyCode
        assert!(string_to_keycode(&controls.move_forward).is_some());
        assert!(string_to_keycode(&controls.move_backward).is_some());
        assert!(string_to_keycode(&controls.move_left).is_some());
        assert!(string_to_keycode(&controls.move_right).is_some());
        assert!(string_to_keycode(&controls.jump).is_some());
        assert!(string_to_keycode(&controls.crouch).is_some());
        assert!(string_to_keycode(&controls.sprint).is_some());
        assert!(string_to_keycode(&controls.toggle_fly).is_some());
        assert!(string_to_keycode(&controls.toggle_noclip).is_some());
        assert!(string_to_keycode(&controls.release_cursor).is_some());
    }
}
