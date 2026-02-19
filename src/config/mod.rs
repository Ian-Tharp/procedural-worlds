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

pub mod audio;
pub mod events;
pub mod validation;

use bevy::prelude::*;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{mpsc, Mutex};

use crate::actors::{Movement, Player};
use crate::actors::player::projection_from_config;
use crate::editor::debug_overlay::DebugOverlayState;
use crate::engine::controller::CameraController;
use crate::engine::input::{InputAction, InputMap};
use crate::generation::TerrainConfig;
use crate::world::ChunkManager;
use crate::world::save::SaveSystem;
use crate::world::streaming::StreamingConfig;
use crate::world::unloading::UnloadConfig;

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
    /// Chunk unloading and memory management settings
    pub unload: UnloadSettings,
    /// World chunk loading settings (load distances, vertical range)
    pub world: WorldConfig,
    /// Audio settings (volume, spatial audio, device configuration)
    pub audio: audio::AudioSettings,
    /// Save system settings (save directory, auto-save interval, format)
    pub save: SaveConfig,
    /// Predictive chunk streaming settings (velocity-based prefetching)
    pub streaming: StreamingSettings,
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
    /// Camera near plane (default: 0.1)
    pub camera_near: f32,
    /// Camera far plane. 0.0 = auto-calculate from render distance (default: 0.0)
    pub camera_far: f32,
    /// Camera field of view in degrees (default: 75.0)
    pub camera_fov: f32,
    /// Shadow map resolution (default: 2048)
    pub shadow_map_resolution: u32,
    /// Shadow depth bias to reduce acne (default: 0.02)
    pub shadow_depth_bias: f32,
    /// Shadow normal bias to reduce peter-panning (default: 0.6)
    pub shadow_normal_bias: f32,
    /// Number of shadow cascades (default: 4)
    pub shadow_cascade_count: u32,
    /// Maximum shadow distance in blocks (default: 0 = auto from render_distance)
    pub shadow_max_distance: f32,
    /// Enable bloom effect (default: true)
    pub bloom_enabled: bool,
    /// Bloom intensity (default: 0.15)
    pub bloom_intensity: f32,
    /// Enable distance fog (default: true)
    pub fog_enabled: bool,
    /// Fog start distance in blocks (default: 100.0)
    pub fog_start: f32,
    /// Fog end distance in blocks (default: 250.0)
    pub fog_end: f32,
    /// Texture atlas tile size in pixels (default: 16)
    pub atlas_tile_size: u32,
    /// Texture atlas grid size — tiles per row (default: 16, giving 256 tiles max)
    pub atlas_grid_size: u32,
    /// Use textures (false = flat vertex colors like before). Default: true
    pub use_textures: bool,
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
    /// Biome noise frequency — lower values produce larger biomes (default: 0.005)
    pub biome_scale: f64,
    /// Enable smooth biome boundary blending (default: true).
    ///
    /// When enabled, terrain generation parameters (height amplitude, noise
    /// frequency) are interpolated at biome boundaries using distance-weighted
    /// sampling with noise modulation, eliminating abrupt terrain seams and
    /// producing organic-looking boundary edges.
    pub biome_blend_enabled: bool,
    /// Distance in blocks over which biome parameters blend at boundaries (default: 32.0).
    ///
    /// Larger values produce wider, more gradual transitions. A value of 0
    /// effectively disables blending. Typical range: 16–64.
    pub biome_blend_distance: f64,
    /// Noise frequency for transition zone modulation (default: 0.08).
    ///
    /// Controls the scale of the Perlin noise mask that warps biome boundaries.
    /// Higher values create more jagged, detailed boundary edges.
    /// Lower values create smoother, broader boundary undulations.
    pub transition_noise_scale: f64,
    /// Amplitude of the transition noise modulation (default: 0.45).
    ///
    /// Controls how much the noise mask distorts the blend boundary.
    /// At 0.0, transitions are purely distance-based (geometric).
    /// At 1.0, noise can shift the effective boundary by up to one full
    /// blend distance. Typical range: 0.2–0.6.
    pub transition_noise_amplitude: f64,
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

/// Chunk unloading settings
///
/// Controls when and how chunks are removed from memory as the player
/// moves through the world. Modified chunks can be saved to disk first.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct UnloadSettings {
    /// Override unload distance in chunks. If `null`, defaults to `render_distance + 2`.
    pub unload_distance: Option<i32>,
    /// Save modified chunks to disk before unloading (default: true).
    /// Disabling this discards unsaved player modifications!
    pub save_on_unload: bool,
    /// Memory threshold in MB. When process RSS exceeds this, unload distance
    /// shrinks to free memory faster. Default: 2048 (2 GB).
    pub memory_threshold_mb: usize,
    /// Chunk distance reduction under memory pressure. Default: 2.
    pub memory_pressure_reduction: i32,
    /// Maximum save tasks per frame. Default: 4.
    pub max_saves_per_frame: u32,
}

/// World chunk loading configuration
///
/// Controls how far chunks are loaded around the player.
/// The horizontal load distance defaults to `render_distance` if set to `None`.
/// The vertical range controls how many chunk layers above and below the
/// player's current chunk are loaded.
///
/// These values can be adjusted at runtime via F5 (decrease) / F6 (increase).
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct WorldConfig {
    /// Horizontal chunk loading distance (in chunks). If `null`, uses `render.render_distance`.
    pub load_distance: Option<i32>,
    /// Number of chunk layers to load above the player's chunk (default: 4)
    pub vertical_load_up: i32,
    /// Number of chunk layers to load below the player's chunk (default: 2)
    pub vertical_load_down: i32,
}

/// Predictive chunk streaming settings
///
/// Controls velocity-based chunk prefetching to reduce loading stutters
/// when the player moves through the world at speed.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct StreamingSettings {
    /// Chunks to prefetch ahead of the player in their movement direction (default: 3)
    pub lookahead_chunks: i32,
    /// Velocity smoothing factor (0.0-1.0). Higher = more responsive (default: 0.15)
    pub velocity_smoothing: f32,
    /// Minimum horizontal speed in chunks/sec to activate prediction (default: 0.5)
    pub min_speed_threshold: f32,
    /// Maximum predictive tasks per frame (default: 2)
    pub max_predictive_per_frame: u32,
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
    /// Show chunk statistics panel
    pub show_chunks: bool,
    /// Show rendering settings in debug overlay
    pub show_render: bool,
    /// Enable chunk loading performance metrics collection (default: true).
    ///
    /// When disabled, the rolling-window timing instrumentation is skipped,
    /// reducing overhead in release / performance-sensitive scenarios.
    /// The total-chunks-loaded counter is always maintained regardless.
    pub collect_chunk_metrics: bool,
    /// Whether the performance dashboard (F8) is visible on startup (default: false).
    pub dashboard_visible: bool,
    /// Whether the minimal performance overlay (F2) is visible on startup (default: false).
    pub perf_overlay_visible: bool,
    /// Whether the system profiler (F4) is visible on startup (default: false).
    pub profiler_visible: bool,
    /// FPS threshold below which a warning is shown (default: 60.0).
    pub fps_warning_threshold: f64,
    /// FPS threshold below which a critical warning is shown (default: 30.0).
    pub fps_critical_threshold: f64,
    /// Memory threshold (MB) above which a warning is shown (default: 2048.0).
    pub memory_warning_threshold_mb: f64,
    /// Memory threshold (MB) above which a critical warning is shown (default: 4096.0).
    pub memory_critical_threshold_mb: f64,
}

/// Save system configuration
///
/// Controls where world saves are stored, auto-save timing, and the
/// serialization format for chunk data. The `world.json` metadata file
/// is always JSON (human-readable); only chunk data supports binary.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct SaveConfig {
    /// Root directory for save files (default: `"saves/default"`).
    ///
    /// The directory structure will be:
    /// ```text
    /// <save_dir>/
    /// ├── world.json          ← world metadata, player state
    /// └── chunks/
    ///     ├── chunk_0_2_-3.bin (or .json)
    ///     └── ...
    /// ```
    pub save_dir: String,
    /// Seconds between automatic saves (default: 300 = 5 minutes).
    /// Set to 0 to disable auto-save.
    pub auto_save_interval: f32,
    /// Serialization format for chunk data on disk.
    ///
    /// - `"json"` — Human-readable, larger files (~100KB per chunk).
    ///   Good for debugging and modding.
    /// - `"binary"` — Compact binary via bincode (~8KB per chunk).
    ///   Recommended for normal play.
    ///
    /// Default: `"binary"`.
    pub chunk_format: String,
    /// Enable mesh caching to disk to avoid re-meshing chunks on restart.
    ///
    /// When enabled, generated chunk meshes are cached in `.mesh_cache/`
    /// within the save directory. On subsequent loads, cached meshes are
    /// used instead of regenerating from block data, reducing load time.
    ///
    /// Default: `false`.
    pub mesh_cache_enabled: bool,
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
            unload: UnloadSettings::default(),
            world: WorldConfig::default(),
            audio: audio::AudioSettings::default(),
            save: SaveConfig::default(),
            streaming: StreamingSettings::default(),
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
            camera_near: 0.1,
            camera_far: 0.0,
            camera_fov: 75.0,
            shadow_map_resolution: 2048,
            shadow_depth_bias: 0.02,
            shadow_normal_bias: 0.6,
            shadow_cascade_count: 4,
            shadow_max_distance: 0.0,
            bloom_enabled: true,
            bloom_intensity: 0.15,
            fog_enabled: true,
            fog_start: 100.0,
            fog_end: 250.0,
            atlas_tile_size: 64,
            atlas_grid_size: 16,
            use_textures: true,
        }
    }
}

impl Default for TerrainSettings {
    fn default() -> Self {
        Self {
            seed: 12345,
            base_height: 64.0,
            height_scale: 24.0,
            frequency: 0.02,
            octaves: 4,
            biome_scale: 0.005,
            biome_blend_enabled: true,
            biome_blend_distance: 32.0,
            transition_noise_scale: 0.08,
            transition_noise_amplitude: 0.45,
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

impl Default for UnloadSettings {
    fn default() -> Self {
        Self {
            unload_distance: None,
            save_on_unload: true,
            memory_threshold_mb: 2048,
            memory_pressure_reduction: 2,
            max_saves_per_frame: 4,
        }
    }
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            load_distance: None,
            vertical_load_up: 4,
            vertical_load_down: 2,
        }
    }
}

impl Default for StreamingSettings {
    fn default() -> Self {
        Self {
            lookahead_chunks: 3,
            velocity_smoothing: 0.15,
            min_speed_threshold: 0.5,
            max_predictive_per_frame: 2,
        }
    }
}

impl Default for DebugConfig {
    fn default() -> Self {
        Self {
            overlay_visible: true,
            show_memory: true,
            show_input: false,
            show_chunks: true,
            show_render: true,
            collect_chunk_metrics: true,
            dashboard_visible: false,
            perf_overlay_visible: false,
            profiler_visible: false,
            fps_warning_threshold: 60.0,
            fps_critical_threshold: 30.0,
            memory_warning_threshold_mb: 2048.0,
            memory_critical_threshold_mb: 4096.0,
        }
    }
}

impl Default for SaveConfig {
    fn default() -> Self {
        Self {
            save_dir: "saves/default".into(),
            auto_save_interval: 300.0,
            chunk_format: "binary".into(),
            mesh_cache_enabled: false,
        }
    }
}

// ============================================================================
// HOT-RELOAD FILE WATCHER
// ============================================================================

/// Resource that holds the file watcher and change notification receiver.
///
/// The `notify` crate watches `config.json` on a background thread and sends
/// events through a channel. The Bevy system `poll_config_changes` drains
/// this channel each frame and triggers a config reload when modifications
/// are detected.
#[derive(Resource)]
pub struct ConfigWatcher {
    /// Receives file-change events from the `notify` watcher thread.
    /// Wrapped in `Mutex` to satisfy Bevy's `Sync` requirement for resources.
    /// Only locked briefly each frame in `poll_config_changes`.
    receiver: Mutex<mpsc::Receiver<Result<Event, notify::Error>>>,
    /// Kept alive to maintain the watch. Dropping this stops the watcher.
    _watcher: RecommendedWatcher,
}

impl ConfigWatcher {
    /// Create a new file watcher monitoring `config.json`.
    ///
    /// Returns `None` if the watcher fails to initialize (e.g., OS limit on
    /// inotify watches). The game continues without hot-reload in that case.
    pub fn new() -> Option<Self> {
        let (tx, rx) = mpsc::channel();

        let mut watcher = match RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                // Send all events; filtering happens on the consumer side
                let _ = tx.send(res);
            },
            notify::Config::default(),
        ) {
            Ok(w) => w,
            Err(e) => {
                warn!("Failed to create config file watcher: {}. Hot-reload disabled.", e);
                return None;
            }
        };

        let config_path = PathBuf::from(CONFIG_FILE);

        // Canonicalize to absolute path if possible (helps notify on Windows)
        let watch_path = fs::canonicalize(&config_path).unwrap_or(config_path);

        if let Err(e) = watcher.watch(&watch_path, RecursiveMode::NonRecursive) {
            warn!("Failed to watch {}: {}. Hot-reload disabled.", CONFIG_FILE, e);
            return None;
        }

        info!("Config hot-reload enabled — watching {}", CONFIG_FILE);
        Some(Self {
            receiver: Mutex::new(rx),
            _watcher: watcher,
        })
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
///
/// Also sets up config hot-reload: a file watcher monitors `config.json` and
/// automatically re-applies settings when the file changes on disk.
pub struct ConfigPlugin;

impl Plugin for ConfigPlugin {
    fn build(&self, app: &mut App) {
        // Register the config change event
        app.add_event::<events::ConfigChanged>();

        // Apply config once at startup (with validation)
        app.add_systems(
            PostStartup,
            (apply_config_to_resources, apply_config_to_entities).chain(),
        );

        // Initialize file watcher for hot-reload
        if let Some(watcher) = ConfigWatcher::new() {
            app.insert_resource(watcher);
            app.add_systems(
                Update,
                poll_config_changes.run_if(resource_exists::<ConfigWatcher>),
            );
        }
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
    mut unload_config: ResMut<UnloadConfig>,
    mut load_metrics: ResMut<crate::world::ChunkLoadMetrics>,
    mut save_system: ResMut<SaveSystem>,
    mut streaming_config: ResMut<StreamingConfig>,
    mut mesh_cache: ResMut<crate::world::mesh_cache::ChunkMeshCache>,
) {
    info!("Applying engine configuration...");

    // --- Render settings ---
    chunk_manager.render_distance = config.render.render_distance;
    chunk_manager.max_chunks_per_frame = config.render.max_chunks_per_frame;
    info!(
        "Render distance: {} chunks, max {}/frame",
        config.render.render_distance, config.render.max_chunks_per_frame
    );

    // --- World / chunk loading settings ---
    chunk_manager.load_distance = config.world.load_distance;
    chunk_manager.vertical_load_up = config.world.vertical_load_up;
    chunk_manager.vertical_load_down = config.world.vertical_load_down;
    info!(
        "Chunk loading: horizontal={}, vertical=(-{}..+{})",
        chunk_manager.effective_load_distance(),
        config.world.vertical_load_down,
        config.world.vertical_load_up,
    );

    // --- Terrain settings ---
    terrain_config.seed = config.terrain.seed;
    terrain_config.base_height = config.terrain.base_height;
    terrain_config.height_scale = config.terrain.height_scale;
    terrain_config.frequency = config.terrain.frequency;
    terrain_config.octaves = config.terrain.octaves;
    terrain_config.biome_scale = config.terrain.biome_scale;
    terrain_config.blend_enabled = config.terrain.biome_blend_enabled;
    terrain_config.blend_distance = config.terrain.biome_blend_distance;
    terrain_config.transition_noise_scale = config.terrain.transition_noise_scale;
    terrain_config.transition_noise_amplitude = config.terrain.transition_noise_amplitude;
    info!(
        "Terrain seed: {}, biome_scale: {}, blend: {} (distance: {}, noise: {:.2}@{:.3})",
        config.terrain.seed,
        config.terrain.biome_scale,
        config.terrain.biome_blend_enabled,
        config.terrain.biome_blend_distance,
        config.terrain.transition_noise_amplitude,
        config.terrain.transition_noise_scale,
    );

    // --- Debug overlay settings ---
    debug_state.visible = config.debug.overlay_visible;
    debug_state.show_memory = config.debug.show_memory;
    debug_state.show_input_state = config.debug.show_input;
    debug_state.show_chunks = config.debug.show_chunks;
    debug_state.show_render = config.debug.show_render;

    // --- Chunk metrics toggle ---
    load_metrics.enabled = config.debug.collect_chunk_metrics;
    info!(
        "Chunk metrics collection: {}",
        if config.debug.collect_chunk_metrics { "enabled" } else { "disabled" },
    );

    // --- Unload settings ---
    unload_config.unload_distance = config.unload.unload_distance;
    unload_config.save_on_unload = config.unload.save_on_unload;
    unload_config.memory_threshold_bytes = config.unload.memory_threshold_mb * 1024 * 1024;
    unload_config.memory_pressure_reduction = config.unload.memory_pressure_reduction;
    unload_config.max_saves_per_frame = config.unload.max_saves_per_frame;
    info!(
        "Unload: save_on_unload={}, threshold={}MB, distance={:?}",
        config.unload.save_on_unload,
        config.unload.memory_threshold_mb,
        config.unload.unload_distance,
    );

    // --- Save system settings ---
    save_system.save_dir = std::path::PathBuf::from(&config.save.save_dir);
    save_system.auto_save_interval = config.save.auto_save_interval;
    save_system.set_chunk_format_from_str(&config.save.chunk_format);
    info!(
        "Save: dir={:?}, auto_save={}s, format={}",
        config.save.save_dir,
        config.save.auto_save_interval,
        config.save.chunk_format,
    );

    // --- Mesh cache settings ---
    *mesh_cache = crate::world::mesh_cache::ChunkMeshCache::new(
        &std::path::PathBuf::from(&config.save.save_dir),
        config.save.mesh_cache_enabled,
    );
    info!(
        "Mesh cache: enabled={}, dir={:?}",
        mesh_cache.enabled, mesh_cache.cache_dir,
    );

    // --- Streaming settings ---
    streaming_config.lookahead_chunks = config.streaming.lookahead_chunks;
    streaming_config.velocity_smoothing = config.streaming.velocity_smoothing;
    streaming_config.min_speed_threshold = config.streaming.min_speed_threshold;
    streaming_config.max_predictive_per_frame = config.streaming.max_predictive_per_frame;
    info!(
        "Streaming: lookahead={}, smoothing={}, threshold={}",
        config.streaming.lookahead_chunks,
        config.streaming.velocity_smoothing,
        config.streaming.min_speed_threshold,
    );

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
    mut camera_query: Query<(&mut CameraController, &mut Projection), With<Camera3d>>,
    mut player_query: Query<&mut Movement, With<Player>>,
) {
    // --- Camera sensitivity and projection ---
    let proj = projection_from_config(&config);
    for (mut controller, mut camera_proj) in &mut camera_query {
        controller.sensitivity = config.player.mouse_sensitivity;
        *camera_proj = proj.clone();
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
// HOT-RELOAD SYSTEMS
// ============================================================================

/// Polls the file watcher for config changes and reloads when detected.
///
/// Runs every frame in `Update`. The `mpsc::Receiver::try_recv` is non-blocking,
/// so this has near-zero cost when no changes occur. When a modify event is
/// detected, the config is re-read from disk, validated, and only
/// hot-reloadable sections are re-applied. Terrain and window settings are
/// skipped with a warning (they require a restart).
///
/// A [`ConfigChanged`](events::ConfigChanged) event is emitted after
/// successful reload so other systems can react to specific changes.
#[allow(clippy::too_many_arguments)]
fn poll_config_changes(
    watcher: Res<ConfigWatcher>,
    mut config: ResMut<EngineConfig>,
    mut input_map: ResMut<InputMap>,
    mut chunk_manager: ResMut<ChunkManager>,
    mut debug_state: ResMut<DebugOverlayState>,
    mut unload_config: ResMut<UnloadConfig>,
    mut audio_config: ResMut<audio::AudioConfig>,
    mut load_metrics: ResMut<crate::world::ChunkLoadMetrics>,
    mut save_system: ResMut<SaveSystem>,
    mut streaming_config: ResMut<StreamingConfig>,
    mut mesh_cache: ResMut<crate::world::mesh_cache::ChunkMeshCache>,
    mut camera_query: Query<(&mut CameraController, &mut Projection), With<Camera3d>>,
    mut player_query: Query<&mut Movement, With<Player>>,
    mut config_events: EventWriter<events::ConfigChanged>,
) {
    let mut should_reload = false;

    // Lock the receiver briefly to drain pending events
    let receiver = watcher.receiver.lock().unwrap();

    // Drain all pending events — we only care whether *any* modify happened
    while let Ok(event_result) = receiver.try_recv() {
        match event_result {
            Ok(event) => {
                if matches!(
                    event.kind,
                    EventKind::Modify(_) | EventKind::Create(_)
                ) {
                    should_reload = true;
                }
            }
            Err(e) => {
                warn!("Config watcher error: {}", e);
            }
        }
    }

    // Release the lock before doing any I/O or resource mutation
    drop(receiver);

    if !should_reload {
        return;
    }

    // Re-read config from disk
    let path = std::path::Path::new(CONFIG_FILE);
    let raw_config = match fs::read_to_string(path) {
        Ok(contents) => match serde_json::from_str::<EngineConfig>(&contents) {
            Ok(c) => c,
            Err(e) => {
                warn!("Hot-reload: failed to parse {}: {}. Keeping current config.", CONFIG_FILE, e);
                return;
            }
        },
        Err(e) => {
            warn!("Hot-reload: failed to read {}: {}. Keeping current config.", CONFIG_FILE, e);
            return;
        }
    };

    // Validate the new config before applying
    let validation = validation::validate_config(raw_config);
    for issue in &validation.issues {
        match issue.severity {
            validation::Severity::Error => {
                warn!(
                    "Hot-reload validation error [{}.{}]: {}",
                    issue.section, issue.field, issue.message
                );
            }
            validation::Severity::Warning => {
                info!(
                    "Hot-reload validation warning [{}.{}]: {}",
                    issue.section, issue.field, issue.message
                );
            }
        }
    }
    let new_config = validation.config;

    // Detect which sections changed
    let changed_sections = events::detect_changes(&config, &new_config);

    if changed_sections.is_empty() {
        info!("Hot-reload: config.json changed on disk but no effective setting changes detected");
        return;
    }

    // Check for non-reloadable changes and warn
    let had_non_reloadable = changed_sections.iter().any(|s| !s.hot_reloadable());
    if had_non_reloadable {
        let non_reloadable: Vec<_> = changed_sections
            .iter()
            .filter(|s| !s.hot_reloadable())
            .collect();
        warn!(
            "Hot-reload: {:?} settings changed but require a restart to take effect. Skipping.",
            non_reloadable
        );
    }

    let reloadable: Vec<_> = changed_sections
        .iter()
        .filter(|s| s.hot_reloadable())
        .copied()
        .collect();

    if reloadable.is_empty() {
        info!("Hot-reload: only non-reloadable settings changed — no runtime updates applied");
        // Still emit event so systems know something was attempted
        config_events.send(events::ConfigChanged {
            changed_sections,
            validation_issues: validation.issues.len(),
            had_non_reloadable_changes: true,
        });
        return;
    }

    info!(
        "Hot-reload: config.json changed — applying {} section(s): {:?}",
        reloadable.len(),
        reloadable
    );

    // Update the stored config resource with validated values.
    // Preserve terrain and window from the OLD config since those aren't hot-reloadable.
    let old_terrain = config.terrain.clone();
    let old_window = config.window.clone();
    *config = new_config;
    config.terrain = old_terrain;
    config.window = old_window;

    // --- Re-apply only hot-reloadable sections ---

    if reloadable.contains(&events::ConfigSection::Render) {
        chunk_manager.render_distance = config.render.render_distance;
        chunk_manager.max_chunks_per_frame = config.render.max_chunks_per_frame;
    }

    if reloadable.contains(&events::ConfigSection::Debug) {
        debug_state.visible = config.debug.overlay_visible;
        debug_state.show_memory = config.debug.show_memory;
        debug_state.show_input_state = config.debug.show_input;
        debug_state.show_chunks = config.debug.show_chunks;
        debug_state.show_render = config.debug.show_render;
        load_metrics.enabled = config.debug.collect_chunk_metrics;
    }

    if reloadable.contains(&events::ConfigSection::Unload) {
        unload_config.unload_distance = config.unload.unload_distance;
        unload_config.save_on_unload = config.unload.save_on_unload;
        unload_config.memory_threshold_bytes = config.unload.memory_threshold_mb * 1024 * 1024;
        unload_config.memory_pressure_reduction = config.unload.memory_pressure_reduction;
        unload_config.max_saves_per_frame = config.unload.max_saves_per_frame;
    }

    if reloadable.contains(&events::ConfigSection::Save) {
        save_system.save_dir = std::path::PathBuf::from(&config.save.save_dir);
        save_system.auto_save_interval = config.save.auto_save_interval;
        save_system.set_chunk_format_from_str(&config.save.chunk_format);
        *mesh_cache = crate::world::mesh_cache::ChunkMeshCache::new(
            &std::path::PathBuf::from(&config.save.save_dir),
            config.save.mesh_cache_enabled,
        );
    }

    if reloadable.contains(&events::ConfigSection::Streaming) {
        streaming_config.lookahead_chunks = config.streaming.lookahead_chunks;
        streaming_config.velocity_smoothing = config.streaming.velocity_smoothing;
        streaming_config.min_speed_threshold = config.streaming.min_speed_threshold;
        streaming_config.max_predictive_per_frame = config.streaming.max_predictive_per_frame;
    }

    if reloadable.contains(&events::ConfigSection::World) {
        chunk_manager.load_distance = config.world.load_distance;
        chunk_manager.vertical_load_up = config.world.vertical_load_up;
        chunk_manager.vertical_load_down = config.world.vertical_load_down;
    }

    if reloadable.contains(&events::ConfigSection::Controls) {
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
    }

    if reloadable.contains(&events::ConfigSection::Player) {
        let proj = projection_from_config(&config);
        for (mut controller, mut camera_proj) in &mut camera_query {
            controller.sensitivity = config.player.mouse_sensitivity;
            *camera_proj = proj.clone();
        }
        for mut movement in &mut player_query {
            movement.walk_speed = config.player.walk_speed;
            movement.sprint_speed = config.player.sprint_speed;
            movement.fly_speed = config.player.fly_speed;
            movement.jump_velocity = config.player.jump_velocity;
        }
    }

    if reloadable.contains(&events::ConfigSection::Audio) {
        *audio_config = audio::AudioConfig::from_settings(&config.audio);
    }

    // Emit event so other systems can react
    config_events.send(events::ConfigChanged {
        changed_sections,
        validation_issues: validation.issues.len(),
        had_non_reloadable_changes: had_non_reloadable,
    });

    info!(
        "Hot-reload complete: {} section(s) applied, {} validation issue(s)",
        reloadable.len(),
        validation.issues.len(),
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
        assert_eq!(config.terrain.base_height, 64.0);
        assert_eq!(config.terrain.height_scale, 24.0);
        assert_eq!(config.terrain.frequency, 0.02);
        assert_eq!(config.terrain.octaves, 4);

        // Player defaults should match Movement::default()
        assert_eq!(config.player.walk_speed, 4.3);
        assert_eq!(config.player.sprint_speed, 5.6);
        assert_eq!(config.player.fly_speed, 11.0);
        assert_eq!(config.player.jump_velocity, 8.4);

        // Mouse sensitivity should match CameraController::default()
        assert_eq!(config.player.mouse_sensitivity, 0.1);

        // Unload defaults should match UnloadConfig::default()
        assert!(config.unload.save_on_unload);
        assert_eq!(config.unload.unload_distance, None);
        assert_eq!(config.unload.memory_threshold_mb, 2048);
        assert_eq!(config.unload.memory_pressure_reduction, 2);
        assert_eq!(config.unload.max_saves_per_frame, 4);

        // World / chunk loading defaults
        assert_eq!(config.world.load_distance, None);
        assert_eq!(config.world.vertical_load_up, 4);
        assert_eq!(config.world.vertical_load_down, 2);

        // Audio defaults
        assert_eq!(config.audio.master_volume, 0.8);
        assert_eq!(config.audio.ambience_intensity, 0.6);
        assert_eq!(config.audio.distance_falloff, 1.0);
        assert!(config.audio.spatial_audio_enabled);
        assert_eq!(config.audio.music_volume, 0.5);
        assert_eq!(config.audio.sfx_volume, 0.7);
        assert!(config.audio.enabled);
        assert_eq!(config.audio.preferred_device, None);

        // Save system defaults
        assert_eq!(config.save.save_dir, "saves/default");
        assert_eq!(config.save.auto_save_interval, 300.0);
        assert_eq!(config.save.chunk_format, "binary");
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
        assert_eq!(deserialized.unload.save_on_unload, original.unload.save_on_unload);
        assert_eq!(deserialized.unload.memory_threshold_mb, original.unload.memory_threshold_mb);
        assert_eq!(deserialized.world.load_distance, original.world.load_distance);
        assert_eq!(deserialized.world.vertical_load_up, original.world.vertical_load_up);
        assert_eq!(deserialized.world.vertical_load_down, original.world.vertical_load_down);
        assert_eq!(deserialized.terrain.biome_blend_enabled, original.terrain.biome_blend_enabled);
        assert_eq!(deserialized.terrain.biome_blend_distance, original.terrain.biome_blend_distance);
        assert_eq!(deserialized.audio.master_volume, original.audio.master_volume);
        assert_eq!(deserialized.audio.ambience_intensity, original.audio.ambience_intensity);
        assert_eq!(deserialized.audio.distance_falloff, original.audio.distance_falloff);
        assert_eq!(deserialized.audio.spatial_audio_enabled, original.audio.spatial_audio_enabled);
        assert_eq!(deserialized.audio.enabled, original.audio.enabled);
        assert_eq!(deserialized.debug.collect_chunk_metrics, original.debug.collect_chunk_metrics);
        assert_eq!(deserialized.save.save_dir, original.save.save_dir);
        assert_eq!(deserialized.save.auto_save_interval, original.save.auto_save_interval);
        assert_eq!(deserialized.save.chunk_format, original.save.chunk_format);
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
    fn test_biome_blend_config_defaults() {
        let config = EngineConfig::default();
        assert!(config.terrain.biome_blend_enabled, "Blending should be enabled by default");
        assert_eq!(config.terrain.biome_blend_distance, 32.0, "Default blend distance should be 32.0");
    }

    #[test]
    fn test_biome_blend_config_from_partial_json() {
        // Config without blend fields should get defaults
        let json = r#"{ "terrain": { "seed": 99999 } }"#;
        let config: EngineConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.terrain.seed, 99999);
        assert!(config.terrain.biome_blend_enabled);
        assert_eq!(config.terrain.biome_blend_distance, 32.0);
        assert_eq!(config.terrain.transition_noise_scale, 0.08);
        assert_eq!(config.terrain.transition_noise_amplitude, 0.45);

        // Config with blend fields should honor them
        let json = r#"{ "terrain": { "biome_blend_enabled": false, "biome_blend_distance": 64.0 } }"#;
        let config: EngineConfig = serde_json::from_str(json).unwrap();
        assert!(!config.terrain.biome_blend_enabled);
        assert_eq!(config.terrain.biome_blend_distance, 64.0);

        // Config with transition noise fields should honor them
        let json = r#"{ "terrain": { "transition_noise_scale": 0.12, "transition_noise_amplitude": 0.6 } }"#;
        let config: EngineConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.terrain.transition_noise_scale, 0.12);
        assert_eq!(config.terrain.transition_noise_amplitude, 0.6);
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
    fn test_world_config_partial_json() {
        // Partial world config — missing fields use defaults
        let json = r#"{
            "world": {
                "load_distance": 6
            }
        }"#;
        let config: EngineConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.world.load_distance, Some(6));
        assert_eq!(config.world.vertical_load_up, 4);
        assert_eq!(config.world.vertical_load_down, 2);
    }

    #[test]
    fn test_world_config_null_load_distance() {
        // Explicit null means "use render_distance"
        let json = r#"{
            "world": {
                "load_distance": null,
                "vertical_load_up": 6,
                "vertical_load_down": 3
            }
        }"#;
        let config: EngineConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.world.load_distance, None);
        assert_eq!(config.world.vertical_load_up, 6);
        assert_eq!(config.world.vertical_load_down, 3);
    }

    #[test]
    fn test_config_reload_from_modified_json() {
        // Simulate hot-reload: start with defaults, "reload" from modified JSON
        let original = EngineConfig::default();
        assert_eq!(original.player.walk_speed, 4.3);

        let modified_json = r#"{
            "player": {
                "walk_speed": 10.0,
                "sprint_speed": 15.0,
                "fly_speed": 20.0,
                "jump_velocity": 12.0,
                "mouse_sensitivity": 0.2
            },
            "render": {
                "render_distance": 8
            }
        }"#;

        let reloaded: EngineConfig = serde_json::from_str(modified_json).unwrap();
        assert_eq!(reloaded.player.walk_speed, 10.0);
        assert_eq!(reloaded.player.sprint_speed, 15.0);
        assert_eq!(reloaded.render.render_distance, 8);
        // Unmodified fields should have defaults
        assert_eq!(reloaded.terrain.seed, 12345);
        assert_eq!(reloaded.window.title, "Procedural Worlds Engine");
    }

    #[test]
    fn test_save_config_defaults() {
        let config = SaveConfig::default();
        assert_eq!(config.save_dir, "saves/default");
        assert_eq!(config.auto_save_interval, 300.0);
        assert_eq!(config.chunk_format, "binary");
    }

    #[test]
    fn test_save_config_from_partial_json() {
        // Only save_dir specified — others get defaults
        let json = r#"{ "save": { "save_dir": "my_saves/world1" } }"#;
        let config: EngineConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.save.save_dir, "my_saves/world1");
        assert_eq!(config.save.auto_save_interval, 300.0);
        assert_eq!(config.save.chunk_format, "binary");

        // Explicit JSON format
        let json = r#"{ "save": { "chunk_format": "json", "auto_save_interval": 60.0 } }"#;
        let config: EngineConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.save.chunk_format, "json");
        assert_eq!(config.save.auto_save_interval, 60.0);
    }

    #[test]
    fn test_config_watcher_creation() {
        // ConfigWatcher::new() should succeed when config.json exists,
        // or return None gracefully if the file doesn't exist.
        // Either outcome is acceptable — the important thing is no panic.
        let _result = ConfigWatcher::new();
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
