//! Quick-Start Scenario Builder
//!
//! Provides predefined world scenarios with pre-configured terrain, spawn,
//! and gameplay settings. Users can select a scenario to quickly start a
//! new game without manually configuring every parameter.
//!
//! # Available Scenarios
//!
//! - **Peaceful Plains**: Gentle terrain, abundant trees, easy exploration
//! - **Mountain Explorer**: Dramatic peaks, challenging terrain, rare resources
//! - **Desert Survival**: Harsh sands, scarce vegetation, high difficulty
//! - **Volcanic Challenge**: Dangerous volcanic terrain, extreme conditions
//! - **Mushroom Paradise**: Rare mushroom biome, unique exploration
//! - **Frozen Tundra**: Snow-covered landscape, survival focus
//! - **Random World**: Standard random generation with default settings
//!
//! # Usage
//!
//! ```rust,ignore
//! use crate::persistence::scenario::{Scenario, create_save_from_scenario};
//!
//! // Get a predefined scenario
//! let scenario = Scenario::peaceful_plains();
//!
//! // Create a new save directory with the scenario's settings
//! create_save_from_scenario("saves/my_world", &scenario)?;
//! ```

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

use crate::config::{EngineConfig, TerrainSettings};
use crate::world::save::{PlayerSaveData, TerrainSaveData, WorldSaveData};

// ============================================================================
// SCENARIO DEFINITION
// ============================================================================

/// Difficulty level affecting player settings and world generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Difficulty {
    /// Relaxed gameplay — higher health regen, more resources
    Peaceful,
    /// Balanced difficulty — standard settings
    Normal,
    /// Challenging — reduced resources, tougher environment
    Hard,
    /// Extreme — for experienced players
    Extreme,
}

impl Default for Difficulty {
    fn default() -> Self {
        Self::Normal
    }
}

impl Difficulty {
    /// Human-readable display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            Difficulty::Peaceful => "Peaceful",
            Difficulty::Normal => "Normal",
            Difficulty::Hard => "Hard",
            Difficulty::Extreme => "Extreme",
        }
    }

    /// Speed multiplier applied to player movement.
    pub fn movement_multiplier(&self) -> f32 {
        match self {
            Difficulty::Peaceful => 1.1,
            Difficulty::Normal => 1.0,
            Difficulty::Hard => 0.95,
            Difficulty::Extreme => 0.9,
        }
    }
}

/// Spawn location preset for the player.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpawnSettings {
    /// Starting X coordinate (world blocks).
    pub x: f32,
    /// Starting Y coordinate (world blocks) — typically above terrain.
    pub y: f32,
    /// Starting Z coordinate (world blocks).
    pub z: f32,
    /// Whether the player starts in flying mode.
    pub flying: bool,
    /// Whether the player starts in noclip mode (creative exploration).
    pub noclip: bool,
}

impl Default for SpawnSettings {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 80.0,
            z: 0.0,
            flying: false,
            noclip: false,
        }
    }
}

impl SpawnSettings {
    /// Create spawn settings for creative/exploration mode.
    pub fn creative() -> Self {
        Self {
            x: 0.0,
            y: 100.0,
            z: 0.0,
            flying: true,
            noclip: false,
        }
    }

    /// Create spawn settings at a specific location.
    pub fn at(x: f32, y: f32, z: f32) -> Self {
        Self {
            x,
            y,
            z,
            flying: false,
            noclip: false,
        }
    }
}

/// A complete world scenario with terrain, spawn, and gameplay settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scenario {
    /// Unique identifier for this scenario.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Short description of the scenario.
    pub description: String,
    /// Icon/emoji for UI display.
    pub icon: String,
    /// Terrain generation settings.
    pub terrain: TerrainSettings,
    /// Player spawn settings.
    pub spawn: SpawnSettings,
    /// Difficulty level.
    pub difficulty: Difficulty,
    /// Day/night cycle duration in seconds (0 = disabled).
    pub cycle_duration_seconds: f32,
    /// Auto-save interval in seconds (0 = disabled).
    pub auto_save_interval: f32,
}

impl Default for Scenario {
    fn default() -> Self {
        Self::random_world()
    }
}

// ============================================================================
// PREDEFINED SCENARIOS
// ============================================================================

impl Scenario {
    /// Standard random world with default settings.
    pub fn random_world() -> Self {
        Self {
            id: "random".into(),
            name: "Random World".into(),
            description: "A procedurally generated world with standard settings.".into(),
            icon: "🌍".into(),
            terrain: TerrainSettings::default(),
            spawn: SpawnSettings::default(),
            difficulty: Difficulty::Normal,
            cycle_duration_seconds: 600.0,
            auto_save_interval: 300.0,
        }
    }

    /// Peaceful Plains — gentle terrain, easy exploration.
    pub fn peaceful_plains() -> Self {
        Self {
            id: "peaceful_plains".into(),
            name: "Peaceful Plains".into(),
            description: "Gentle rolling hills with abundant trees. Perfect for relaxed building.".into(),
            icon: "🌾".into(),
            terrain: TerrainSettings {
                seed: 42,
                base_height: 64.0,
                height_scale: 8.0, // Low variation
                frequency: 0.015, // Smooth terrain
                octaves: 3,
                biome_scale: 0.003, // Large biomes
                biome_blend_enabled: true,
                biome_blend_distance: 48.0,
                transition_noise_scale: 0.06,
                transition_noise_amplitude: 0.3,
            },
            spawn: SpawnSettings::at(0.0, 72.0, 0.0),
            difficulty: Difficulty::Peaceful,
            cycle_duration_seconds: 900.0, // Longer days
            auto_save_interval: 300.0,
        }
    }

    /// Mountain Explorer — dramatic peaks, challenging terrain.
    pub fn mountain_explorer() -> Self {
        Self {
            id: "mountain_explorer".into(),
            name: "Mountain Explorer".into(),
            description: "Towering peaks and deep valleys. Bring your climbing skills!".into(),
            icon: "⛰️".into(),
            terrain: TerrainSettings {
                seed: 7777,
                base_height: 48.0,
                height_scale: 48.0, // Extreme variation
                frequency: 0.03, // Steeper terrain
                octaves: 6, // More detail
                biome_scale: 0.008, // Smaller biomes
                biome_blend_enabled: true,
                biome_blend_distance: 24.0,
                transition_noise_scale: 0.1,
                transition_noise_amplitude: 0.5,
            },
            spawn: SpawnSettings {
                x: 0.0,
                y: 120.0, // Higher spawn for mountain views
                z: 0.0,
                flying: true, // Start flying to find a safe landing
                noclip: false,
            },
            difficulty: Difficulty::Hard,
            cycle_duration_seconds: 600.0,
            auto_save_interval: 180.0, // More frequent saves
        }
    }

    /// Desert Survival — harsh sands, scarce vegetation.
    pub fn desert_survival() -> Self {
        Self {
            id: "desert_survival".into(),
            name: "Desert Survival".into(),
            description: "Endless sand dunes and scorching heat. Water is life.".into(),
            icon: "🏜️".into(),
            terrain: TerrainSettings {
                seed: 1234,
                base_height: 56.0,
                height_scale: 16.0,
                frequency: 0.012, // Smooth dunes
                octaves: 4,
                biome_scale: 0.001, // Very large biomes (mostly desert)
                biome_blend_enabled: false, // Sharp biome edges
                biome_blend_distance: 16.0,
                transition_noise_scale: 0.08,
                transition_noise_amplitude: 0.4,
            },
            spawn: SpawnSettings::at(0.0, 68.0, 0.0),
            difficulty: Difficulty::Hard,
            cycle_duration_seconds: 480.0, // Faster day/night
            auto_save_interval: 120.0,
        }
    }

    /// Volcanic Challenge — dangerous volcanic terrain.
    pub fn volcanic_challenge() -> Self {
        Self {
            id: "volcanic_challenge".into(),
            name: "Volcanic Challenge".into(),
            description: "Lava flows and obsidian peaks. Not for the faint of heart.".into(),
            icon: "🌋".into(),
            terrain: TerrainSettings {
                seed: 6666,
                base_height: 40.0,
                height_scale: 40.0,
                frequency: 0.035, // Jagged terrain
                octaves: 5,
                biome_scale: 0.002,
                biome_blend_enabled: true,
                biome_blend_distance: 20.0,
                transition_noise_scale: 0.12,
                transition_noise_amplitude: 0.6,
            },
            spawn: SpawnSettings {
                x: 0.0,
                y: 100.0,
                z: 0.0,
                flying: true, // Start flying to avoid lava
                noclip: false,
            },
            difficulty: Difficulty::Extreme,
            cycle_duration_seconds: 400.0,
            auto_save_interval: 60.0, // Frequent saves for dangerous world
        }
    }

    /// Frozen Tundra — snow-covered survival.
    pub fn frozen_tundra() -> Self {
        Self {
            id: "frozen_tundra".into(),
            name: "Frozen Tundra".into(),
            description: "Ice and snow as far as the eye can see. Stay warm!".into(),
            icon: "❄️".into(),
            terrain: TerrainSettings {
                seed: 9999,
                base_height: 60.0,
                height_scale: 12.0,
                frequency: 0.018,
                octaves: 4,
                biome_scale: 0.002, // Large frozen biomes
                biome_blend_enabled: true,
                biome_blend_distance: 40.0,
                transition_noise_scale: 0.07,
                transition_noise_amplitude: 0.35,
            },
            spawn: SpawnSettings::at(0.0, 75.0, 0.0),
            difficulty: Difficulty::Normal,
            cycle_duration_seconds: 720.0, // Long winter nights
            auto_save_interval: 300.0,
        }
    }

    /// Mushroom Paradise — rare mushroom biome exploration.
    pub fn mushroom_paradise() -> Self {
        Self {
            id: "mushroom_paradise".into(),
            name: "Mushroom Paradise".into(),
            description: "A strange land of giant mushrooms and unusual terrain.".into(),
            icon: "🍄".into(),
            terrain: TerrainSettings {
                seed: 4200,
                base_height: 58.0,
                height_scale: 14.0,
                frequency: 0.022,
                octaves: 5,
                biome_scale: 0.004,
                biome_blend_enabled: true,
                biome_blend_distance: 32.0,
                transition_noise_scale: 0.09,
                transition_noise_amplitude: 0.45,
            },
            spawn: SpawnSettings::creative(), // Creative mode for exploration
            difficulty: Difficulty::Peaceful,
            cycle_duration_seconds: 800.0,
            auto_save_interval: 300.0,
        }
    }

    /// Jungle Adventure — dense vegetation, exploration focus.
    pub fn jungle_adventure() -> Self {
        Self {
            id: "jungle_adventure".into(),
            name: "Jungle Adventure".into(),
            description: "Dense tropical forests teeming with life. Adventure awaits!".into(),
            icon: "🌴".into(),
            terrain: TerrainSettings {
                seed: 3141,
                base_height: 62.0,
                height_scale: 20.0,
                frequency: 0.025,
                octaves: 5,
                biome_scale: 0.003,
                biome_blend_enabled: true,
                biome_blend_distance: 36.0,
                transition_noise_scale: 0.08,
                transition_noise_amplitude: 0.4,
            },
            spawn: SpawnSettings::at(0.0, 85.0, 0.0),
            difficulty: Difficulty::Normal,
            cycle_duration_seconds: 600.0,
            auto_save_interval: 240.0,
        }
    }

    /// Return all predefined scenarios.
    pub fn all() -> Vec<Scenario> {
        vec![
            Self::random_world(),
            Self::peaceful_plains(),
            Self::mountain_explorer(),
            Self::desert_survival(),
            Self::volcanic_challenge(),
            Self::frozen_tundra(),
            Self::mushroom_paradise(),
            Self::jungle_adventure(),
        ]
    }

    /// Find a scenario by its ID.
    pub fn by_id(id: &str) -> Option<Scenario> {
        Self::all().into_iter().find(|s| s.id == id)
    }

    /// Create a custom scenario with a specific seed.
    pub fn custom_seed(seed: u32) -> Self {
        let mut scenario = Self::random_world();
        scenario.id = format!("custom_{}", seed);
        scenario.name = format!("Custom World (Seed: {})", seed);
        scenario.description = "A custom world with your chosen seed.".into();
        scenario.terrain.seed = seed;
        scenario
    }
}

// ============================================================================
// SAVE CREATION
// ============================================================================

/// Create a new save directory from a scenario.
///
/// This function:
/// 1. Creates the save directory and chunks subdirectory
/// 2. Writes a `world.json` with the scenario's settings
/// 3. Writes a `scenario.json` reference for later identification
///
/// The world will use the scenario's terrain settings when chunks are
/// generated, and the player will spawn at the scenario's spawn location.
///
/// # Arguments
///
/// * `save_dir` - Path to the save directory (e.g., `"saves/my_world"`)
/// * `scenario` - The scenario to use for world generation
///
/// # Returns
///
/// `Ok(())` on success, or an `io::Error` if directory/file creation fails.
///
/// # Example
///
/// ```rust,ignore
/// use crate::persistence::scenario::{Scenario, create_save_from_scenario};
///
/// let scenario = Scenario::peaceful_plains();
/// create_save_from_scenario("saves/peaceful_world", &scenario)?;
/// ```
pub fn create_save_from_scenario(save_dir: impl AsRef<Path>, scenario: &Scenario) -> io::Result<()> {
    let save_dir = save_dir.as_ref();
    let chunk_dir = save_dir.join("chunks");

    // Create directories
    fs::create_dir_all(&chunk_dir)?;

    // Build world save data from scenario
    let world_data = WorldSaveData {
        version: 1,
        timestamp: epoch_timestamp(),
        player: PlayerSaveData {
            position: [scenario.spawn.x, scenario.spawn.y, scenario.spawn.z],
            flying: scenario.spawn.flying,
            noclip: scenario.spawn.noclip,
        },
        terrain: TerrainSaveData {
            seed: scenario.terrain.seed,
            base_height: scenario.terrain.base_height,
            height_scale: scenario.terrain.height_scale,
            frequency: scenario.terrain.frequency,
            octaves: scenario.terrain.octaves,
            biome_scale: scenario.terrain.biome_scale,
        },
        saved_chunks: Vec::new(),
    };

    // Write world.json
    let world_json = serde_json::to_string_pretty(&world_data)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(save_dir.join("world.json"), world_json)?;

    // Write scenario.json for reference
    let scenario_json = serde_json::to_string_pretty(scenario)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(save_dir.join("scenario.json"), scenario_json)?;

    Ok(())
}

/// Apply scenario settings to an EngineConfig.
///
/// Modifies the config's terrain, player, and save settings based on
/// the scenario's parameters. Useful when loading a scenario at runtime.
pub fn apply_scenario_to_config(config: &mut EngineConfig, scenario: &Scenario) {
    // Terrain settings
    config.terrain = scenario.terrain.clone();

    // Difficulty-based player adjustments
    let mult = scenario.difficulty.movement_multiplier();
    config.player.walk_speed *= mult;
    config.player.sprint_speed *= mult;
    config.player.fly_speed *= mult;

    // Timing settings
    config.cycle_duration_seconds = scenario.cycle_duration_seconds;
    config.save.auto_save_interval = scenario.auto_save_interval;
}

/// Load the scenario from a save directory, if one exists.
///
/// Looks for `scenario.json` in the save directory. Returns `None` if
/// the file doesn't exist (e.g., for pre-scenario saves).
pub fn load_scenario_from_save(save_dir: impl AsRef<Path>) -> Option<Scenario> {
    let scenario_file = save_dir.as_ref().join("scenario.json");
    if !scenario_file.exists() {
        return None;
    }

    let json = fs::read_to_string(&scenario_file).ok()?;
    serde_json::from_str(&json).ok()
}

// ============================================================================
// HELPERS
// ============================================================================

/// Generate a simple epoch timestamp string.
fn epoch_timestamp() -> String {
    use std::time::SystemTime;
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => format!("epoch:{}", d.as_secs()),
        Err(_) => "unknown".to_string(),
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_save_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "pw_scenario_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_scenario_defaults() {
        let scenario = Scenario::default();
        assert_eq!(scenario.id, "random");
        assert_eq!(scenario.difficulty, Difficulty::Normal);
    }

    #[test]
    fn test_all_predefined_scenarios() {
        let scenarios = Scenario::all();
        assert!(scenarios.len() >= 7, "Should have at least 7 predefined scenarios");

        // Each should have unique ID
        let ids: Vec<_> = scenarios.iter().map(|s| &s.id).collect();
        let unique_ids: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(ids.len(), unique_ids.len(), "All scenario IDs should be unique");
    }

    #[test]
    fn test_scenario_by_id() {
        assert!(Scenario::by_id("peaceful_plains").is_some());
        assert!(Scenario::by_id("mountain_explorer").is_some());
        assert!(Scenario::by_id("nonexistent").is_none());
    }

    #[test]
    fn test_custom_seed_scenario() {
        let scenario = Scenario::custom_seed(12345);
        assert_eq!(scenario.terrain.seed, 12345);
        assert!(scenario.id.contains("12345"));
    }

    #[test]
    fn test_difficulty_multipliers() {
        assert!(Difficulty::Peaceful.movement_multiplier() > 1.0);
        assert_eq!(Difficulty::Normal.movement_multiplier(), 1.0);
        assert!(Difficulty::Hard.movement_multiplier() < 1.0);
        assert!(Difficulty::Extreme.movement_multiplier() < Difficulty::Hard.movement_multiplier());
    }

    #[test]
    fn test_spawn_settings_presets() {
        let default = SpawnSettings::default();
        assert!(!default.flying);
        assert!(!default.noclip);

        let creative = SpawnSettings::creative();
        assert!(creative.flying);

        let custom = SpawnSettings::at(100.0, 200.0, 300.0);
        assert_eq!(custom.x, 100.0);
        assert_eq!(custom.y, 200.0);
        assert_eq!(custom.z, 300.0);
    }

    #[test]
    fn test_create_save_from_scenario() {
        let dir = temp_save_dir();
        let scenario = Scenario::peaceful_plains();

        let result = create_save_from_scenario(&dir, &scenario);
        assert!(result.is_ok());

        // Check files exist
        assert!(dir.join("world.json").exists());
        assert!(dir.join("scenario.json").exists());
        assert!(dir.join("chunks").is_dir());

        cleanup(&dir);
    }

    #[test]
    fn test_load_scenario_from_save() {
        let dir = temp_save_dir();
        let original = Scenario::mountain_explorer();

        create_save_from_scenario(&dir, &original).unwrap();

        let loaded = load_scenario_from_save(&dir);
        assert!(loaded.is_some());

        let loaded = loaded.unwrap();
        assert_eq!(loaded.id, "mountain_explorer");
        assert_eq!(loaded.terrain.seed, original.terrain.seed);

        cleanup(&dir);
    }

    #[test]
    fn test_load_scenario_missing() {
        let dir = temp_save_dir();
        fs::create_dir_all(&dir).unwrap();

        // No scenario.json exists
        let loaded = load_scenario_from_save(&dir);
        assert!(loaded.is_none());

        cleanup(&dir);
    }

    #[test]
    fn test_apply_scenario_to_config() {
        let mut config = EngineConfig::default();
        let scenario = Scenario::peaceful_plains();

        let original_walk_speed = config.player.walk_speed;
        apply_scenario_to_config(&mut config, &scenario);

        // Peaceful difficulty increases movement speed
        assert!(config.player.walk_speed > original_walk_speed);
        assert_eq!(config.terrain.seed, scenario.terrain.seed);
        assert_eq!(config.cycle_duration_seconds, scenario.cycle_duration_seconds);
    }

    #[test]
    fn test_scenario_serialization_roundtrip() {
        let original = Scenario::volcanic_challenge();
        let json = serde_json::to_string_pretty(&original).unwrap();
        let parsed: Scenario = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.id, original.id);
        assert_eq!(parsed.name, original.name);
        assert_eq!(parsed.terrain.seed, original.terrain.seed);
        assert_eq!(parsed.difficulty, original.difficulty);
    }

    #[test]
    fn test_all_scenarios_have_icons() {
        for scenario in Scenario::all() {
            assert!(
                !scenario.icon.is_empty(),
                "Scenario {} should have an icon",
                scenario.id
            );
        }
    }

    #[test]
    fn test_all_scenarios_have_descriptions() {
        for scenario in Scenario::all() {
            assert!(
                scenario.description.len() > 10,
                "Scenario {} should have a meaningful description",
                scenario.id
            );
        }
    }
}
