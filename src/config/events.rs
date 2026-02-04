//! Configuration change events
//!
//! Provides a Bevy event system for notifying other engine systems when
//! configuration settings change at runtime (e.g., via hot-reload or an
//! in-game settings panel).
//!
//! # Architecture
//!
//! ```text
//! config.json modified on disk
//!     ↓
//! ConfigWatcher detects change
//!     ↓
//! poll_config_changes validates & applies
//!     ↓
//! ConfigChanged event emitted
//!     ↓
//! Other systems observe (e.g., audio, rendering, debug overlay)
//! ```
//!
//! # Usage
//!
//! Systems that need to react to config changes should read the event:
//!
//! ```rust,ignore
//! fn my_system(
//!     mut events: EventReader<ConfigChanged>,
//!     config: Res<EngineConfig>,
//! ) {
//!     for event in events.read() {
//!         if event.affects(ConfigSection::Audio) {
//!             // React to audio config changes
//!         }
//!     }
//! }
//! ```
//!
//! # Hot-Reload Safety
//!
//! Not all config sections are safe to change at runtime. The
//! [`ConfigSection`] enum distinguishes between hot-reloadable and
//! startup-only settings. The hot-reload system only applies changes
//! to sections marked as `hot_reloadable()`.

use bevy::prelude::*;

// ============================================================================
// CONFIG SECTIONS
// ============================================================================

/// Identifies which section of the config was modified.
///
/// Used both for change notification and to determine whether a section
/// is safe to hot-reload at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigSection {
    /// Player movement and camera settings (walk speed, sensitivity, etc.)
    Player,
    /// Debug overlay settings (visibility, panels)
    Debug,
    /// Rendering settings (render distance, bloom, fog, shadows)
    Render,
    /// Audio settings (volumes, spatial audio, device)
    Audio,
    /// Input key bindings
    Controls,
    /// Chunk unloading and memory management
    Unload,
    /// World chunk loading distances
    World,
    /// Predictive streaming settings
    Streaming,
    /// Save system settings (directory, format, auto-save interval)
    Save,
    /// Day/night cycle duration
    CycleDuration,
    /// Terrain generation settings (seed, noise parameters, biome settings)
    /// **Not hot-reloadable** — changes require world regeneration.
    Terrain,
    /// Window settings (title, resolution, vsync)
    /// **Not hot-reloadable** — changes require restart.
    Window,
}

impl ConfigSection {
    /// Returns `true` if this config section can be safely hot-reloaded
    /// at runtime without risking engine state corruption.
    ///
    /// Sections that affect world generation, window creation, or other
    /// startup-only systems return `false`.
    pub fn hot_reloadable(&self) -> bool {
        match self {
            // Safe to change at runtime
            ConfigSection::Player => true,
            ConfigSection::Debug => true,
            ConfigSection::Render => true,
            ConfigSection::Audio => true,
            ConfigSection::Controls => true,
            ConfigSection::Unload => true,
            ConfigSection::World => true,
            ConfigSection::Streaming => true,
            ConfigSection::Save => true,
            ConfigSection::CycleDuration => true,

            // NOT safe — require restart or world regeneration
            ConfigSection::Terrain => false,
            ConfigSection::Window => false,
        }
    }

    /// Returns all hot-reloadable sections.
    pub fn all_hot_reloadable() -> &'static [ConfigSection] {
        &[
            ConfigSection::Player,
            ConfigSection::Debug,
            ConfigSection::Render,
            ConfigSection::Audio,
            ConfigSection::Controls,
            ConfigSection::Unload,
            ConfigSection::World,
            ConfigSection::Streaming,
            ConfigSection::Save,
            ConfigSection::CycleDuration,
        ]
    }
}

// ============================================================================
// EVENTS
// ============================================================================

/// Event emitted when engine configuration is reloaded.
///
/// Contains which sections changed so listeners can selectively react.
/// Emitted by `poll_config_changes` after successful validation and
/// application of the new config.
#[derive(Event, Debug, Clone)]
pub struct ConfigChanged {
    /// Which config sections were modified in this reload.
    pub changed_sections: Vec<ConfigSection>,
    /// How many validation issues were found (and auto-corrected).
    pub validation_issues: usize,
    /// Whether any non-reloadable sections had changes (logged as warnings).
    pub had_non_reloadable_changes: bool,
}

impl ConfigChanged {
    /// Returns `true` if the given section was affected by this change.
    pub fn affects(&self, section: ConfigSection) -> bool {
        self.changed_sections.contains(&section)
    }

    /// Returns `true` if any hot-reloadable section changed.
    pub fn has_reloadable_changes(&self) -> bool {
        self.changed_sections.iter().any(|s| s.hot_reloadable())
    }
}

// ============================================================================
// CHANGE DETECTION
// ============================================================================

/// Compare two configs and return which sections differ.
///
/// This performs field-by-field comparison to produce a precise list of
/// changed sections, allowing downstream systems to react only to
/// relevant changes.
pub fn detect_changes(old: &super::EngineConfig, new: &super::EngineConfig) -> Vec<ConfigSection> {
    let mut changes = Vec::new();

    // Window
    if old.window.title != new.window.title
        || old.window.width != new.window.width
        || old.window.height != new.window.height
        || old.window.vsync != new.window.vsync
    {
        changes.push(ConfigSection::Window);
    }

    // Render
    if old.render.render_distance != new.render.render_distance
        || old.render.max_chunks_per_frame != new.render.max_chunks_per_frame
        || old.render.camera_near != new.render.camera_near
        || old.render.camera_far != new.render.camera_far
        || old.render.camera_fov != new.render.camera_fov
        || old.render.shadow_map_resolution != new.render.shadow_map_resolution
        || old.render.shadow_depth_bias != new.render.shadow_depth_bias
        || old.render.shadow_normal_bias != new.render.shadow_normal_bias
        || old.render.shadow_cascade_count != new.render.shadow_cascade_count
        || old.render.shadow_max_distance != new.render.shadow_max_distance
        || old.render.bloom_enabled != new.render.bloom_enabled
        || old.render.bloom_intensity != new.render.bloom_intensity
        || old.render.fog_enabled != new.render.fog_enabled
        || old.render.fog_start != new.render.fog_start
        || old.render.fog_end != new.render.fog_end
        || old.render.atlas_tile_size != new.render.atlas_tile_size
        || old.render.atlas_grid_size != new.render.atlas_grid_size
        || old.render.use_textures != new.render.use_textures
    {
        changes.push(ConfigSection::Render);
    }

    // Terrain
    if old.terrain.seed != new.terrain.seed
        || old.terrain.base_height != new.terrain.base_height
        || old.terrain.height_scale != new.terrain.height_scale
        || old.terrain.frequency != new.terrain.frequency
        || old.terrain.octaves != new.terrain.octaves
        || old.terrain.biome_scale != new.terrain.biome_scale
        || old.terrain.biome_blend_enabled != new.terrain.biome_blend_enabled
        || old.terrain.biome_blend_distance != new.terrain.biome_blend_distance
        || old.terrain.transition_noise_scale != new.terrain.transition_noise_scale
        || old.terrain.transition_noise_amplitude != new.terrain.transition_noise_amplitude
    {
        changes.push(ConfigSection::Terrain);
    }

    // Player
    if old.player.walk_speed != new.player.walk_speed
        || old.player.sprint_speed != new.player.sprint_speed
        || old.player.fly_speed != new.player.fly_speed
        || old.player.jump_velocity != new.player.jump_velocity
        || old.player.mouse_sensitivity != new.player.mouse_sensitivity
    {
        changes.push(ConfigSection::Player);
    }

    // Controls
    if old.controls.move_forward != new.controls.move_forward
        || old.controls.move_backward != new.controls.move_backward
        || old.controls.move_left != new.controls.move_left
        || old.controls.move_right != new.controls.move_right
        || old.controls.jump != new.controls.jump
        || old.controls.crouch != new.controls.crouch
        || old.controls.sprint != new.controls.sprint
        || old.controls.toggle_fly != new.controls.toggle_fly
        || old.controls.toggle_noclip != new.controls.toggle_noclip
        || old.controls.release_cursor != new.controls.release_cursor
    {
        changes.push(ConfigSection::Controls);
    }

    // Debug
    if old.debug.overlay_visible != new.debug.overlay_visible
        || old.debug.show_memory != new.debug.show_memory
        || old.debug.show_input != new.debug.show_input
        || old.debug.show_chunks != new.debug.show_chunks
        || old.debug.show_render != new.debug.show_render
        || old.debug.collect_chunk_metrics != new.debug.collect_chunk_metrics
    {
        changes.push(ConfigSection::Debug);
    }

    // Unload
    if old.unload.unload_distance != new.unload.unload_distance
        || old.unload.save_on_unload != new.unload.save_on_unload
        || old.unload.memory_threshold_mb != new.unload.memory_threshold_mb
        || old.unload.memory_pressure_reduction != new.unload.memory_pressure_reduction
        || old.unload.max_saves_per_frame != new.unload.max_saves_per_frame
    {
        changes.push(ConfigSection::Unload);
    }

    // World
    if old.world.load_distance != new.world.load_distance
        || old.world.vertical_load_up != new.world.vertical_load_up
        || old.world.vertical_load_down != new.world.vertical_load_down
    {
        changes.push(ConfigSection::World);
    }

    // Streaming
    if old.streaming.lookahead_chunks != new.streaming.lookahead_chunks
        || old.streaming.velocity_smoothing != new.streaming.velocity_smoothing
        || old.streaming.min_speed_threshold != new.streaming.min_speed_threshold
        || old.streaming.max_predictive_per_frame != new.streaming.max_predictive_per_frame
    {
        changes.push(ConfigSection::Streaming);
    }

    // Audio
    if old.audio != new.audio {
        changes.push(ConfigSection::Audio);
    }

    // Save
    if old.save.save_dir != new.save.save_dir
        || old.save.auto_save_interval != new.save.auto_save_interval
        || old.save.chunk_format != new.save.chunk_format
    {
        changes.push(ConfigSection::Save);
    }

    // Cycle duration
    if old.cycle_duration_seconds != new.cycle_duration_seconds {
        changes.push(ConfigSection::CycleDuration);
    }

    changes
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identical_configs_produce_no_changes() {
        let config = super::super::EngineConfig::default();
        let changes = detect_changes(&config, &config);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_player_speed_change_detected() {
        let old = super::super::EngineConfig::default();
        let mut new = old.clone();
        new.player.walk_speed = 10.0;
        let changes = detect_changes(&old, &new);
        assert!(changes.contains(&ConfigSection::Player));
        assert_eq!(changes.len(), 1);
    }

    #[test]
    fn test_terrain_change_detected() {
        let old = super::super::EngineConfig::default();
        let mut new = old.clone();
        new.terrain.seed = 99999;
        let changes = detect_changes(&old, &new);
        assert!(changes.contains(&ConfigSection::Terrain));
    }

    #[test]
    fn test_multiple_sections_changed() {
        let old = super::super::EngineConfig::default();
        let mut new = old.clone();
        new.player.fly_speed = 50.0;
        new.debug.overlay_visible = false;
        new.render.render_distance = 8;
        let changes = detect_changes(&old, &new);
        assert_eq!(changes.len(), 3);
        assert!(changes.contains(&ConfigSection::Player));
        assert!(changes.contains(&ConfigSection::Debug));
        assert!(changes.contains(&ConfigSection::Render));
    }

    #[test]
    fn test_terrain_is_not_hot_reloadable() {
        assert!(!ConfigSection::Terrain.hot_reloadable());
    }

    #[test]
    fn test_window_is_not_hot_reloadable() {
        assert!(!ConfigSection::Window.hot_reloadable());
    }

    #[test]
    fn test_player_is_hot_reloadable() {
        assert!(ConfigSection::Player.hot_reloadable());
    }

    #[test]
    fn test_all_hot_reloadable_sections() {
        let sections = ConfigSection::all_hot_reloadable();
        assert!(sections.len() >= 8);
        for s in sections {
            assert!(s.hot_reloadable(), "{:?} should be hot-reloadable", s);
        }
    }

    #[test]
    fn test_config_changed_affects() {
        let event = ConfigChanged {
            changed_sections: vec![ConfigSection::Player, ConfigSection::Audio],
            validation_issues: 0,
            had_non_reloadable_changes: false,
        };
        assert!(event.affects(ConfigSection::Player));
        assert!(event.affects(ConfigSection::Audio));
        assert!(!event.affects(ConfigSection::Terrain));
    }

    #[test]
    fn test_config_changed_has_reloadable_changes() {
        let event = ConfigChanged {
            changed_sections: vec![ConfigSection::Terrain],
            validation_issues: 0,
            had_non_reloadable_changes: true,
        };
        assert!(!event.has_reloadable_changes());

        let event = ConfigChanged {
            changed_sections: vec![ConfigSection::Terrain, ConfigSection::Player],
            validation_issues: 1,
            had_non_reloadable_changes: true,
        };
        assert!(event.has_reloadable_changes());
    }

    #[test]
    fn test_controls_change_detected() {
        let old = super::super::EngineConfig::default();
        let mut new = old.clone();
        new.controls.jump = "KeyJ".into();
        let changes = detect_changes(&old, &new);
        assert!(changes.contains(&ConfigSection::Controls));
    }

    #[test]
    fn test_audio_change_detected() {
        let old = super::super::EngineConfig::default();
        let mut new = old.clone();
        new.audio.master_volume = 0.3;
        let changes = detect_changes(&old, &new);
        assert!(changes.contains(&ConfigSection::Audio));
    }

    #[test]
    fn test_save_change_detected() {
        let old = super::super::EngineConfig::default();
        let mut new = old.clone();
        new.save.chunk_format = "json".into();
        let changes = detect_changes(&old, &new);
        assert!(changes.contains(&ConfigSection::Save));
    }

    #[test]
    fn test_cycle_duration_change_detected() {
        let old = super::super::EngineConfig::default();
        let mut new = old.clone();
        new.cycle_duration_seconds = 300.0;
        let changes = detect_changes(&old, &new);
        assert!(changes.contains(&ConfigSection::CycleDuration));
    }

    #[test]
    fn test_window_change_detected() {
        let old = super::super::EngineConfig::default();
        let mut new = old.clone();
        new.window.width = 1920.0;
        let changes = detect_changes(&old, &new);
        assert!(changes.contains(&ConfigSection::Window));
    }
}
