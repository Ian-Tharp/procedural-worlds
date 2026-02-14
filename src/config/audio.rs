//! Audio configuration settings
//!
//! Defines the serializable audio configuration that is stored as part of
//! `config.json` and exposed at runtime via the [`AudioConfig`] resource.
//!
//! # Settings
//!
//! ## Volume Controls
//! - **Master volume** — global gain applied to all audio output (0.0–1.0)
//! - **Biome ambience intensity** — relative loudness of biome-specific
//!   ambient soundscapes (0.0–1.0)
//! - **Music volume** — volume for background music (0.0–1.0)
//! - **SFX volume** — volume for sound effects like block interactions (0.0–1.0)
//!
//! ## Spatial Audio
//! - **3D audio distance falloff** — controls how quickly positional sounds
//!   attenuate with distance. Higher values make sounds drop off faster.
//! - **Spatial audio toggle** — enable/disable 3D audio processing
//!
//! ## Device Configuration
//! - **Preferred device** — select a specific output device by name
//! - **Sample rate** — preferred sample rate for the audio output
//! - **Buffer size** — preferred buffer size in frames
//! - **Enabled** — master toggle for the audio subsystem
//!
//! # Persistence
//!
//! Settings are saved to / loaded from `config.json` automatically through
//! the engine's existing [`EngineConfig`](super::EngineConfig) system,
//! including hot-reload support.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// CONFIG STRUCT (serialized in config.json)
// ============================================================================

/// Audio settings section of `config.json`.
///
/// Combines volume controls, spatial audio settings, and device configuration
/// into a single unified section. All fields use `#[serde(default)]` so the
/// config file remains forward-compatible when new audio fields are added later.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct AudioSettings {
    // ── Volume Controls ──
    /// Master volume multiplier (0.0 = mute, 1.0 = full). Default: 0.8
    pub master_volume: f32,
    /// Biome ambience intensity (0.0 = silent, 1.0 = full). Default: 0.6
    pub ambience_intensity: f32,
    /// Music volume multiplier (0.0 = mute, 1.0 = full). Default: 0.5
    pub music_volume: f32,
    /// Sound effects volume multiplier (0.0 = mute, 1.0 = full). Default: 0.7
    pub sfx_volume: f32,

    // ── Spatial Audio ──
    /// 3D audio distance falloff exponent. Higher = faster attenuation.
    /// Typical range: 0.5–3.0.  Default: 1.0 (inverse distance)
    pub distance_falloff: f32,
    /// Enable 3D spatial audio processing. Default: true
    pub spatial_audio_enabled: bool,

    // ── Device Configuration ──
    /// Enable the audio subsystem entirely. Default: true.
    /// When false, no audio devices are opened and all sound is silent.
    pub enabled: bool,
    /// Preferred audio output device name (`null` = system default).
    ///
    /// If the specified device is not found at startup, the engine falls
    /// back to the system default and logs a warning.
    pub preferred_device: Option<String>,
    /// Preferred sample rate in Hz (`null` = device default).
    /// Common values: 44100, 48000, 96000.
    pub sample_rate: Option<u32>,
    /// Preferred buffer size in frames (`null` = device default).
    /// Lower values reduce latency but increase CPU usage.
    /// Common values: 256, 512, 1024.
    pub buffer_size: Option<u32>,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            master_volume: 0.8,
            ambience_intensity: 0.6,
            music_volume: 0.5,
            sfx_volume: 0.7,
            distance_falloff: 1.0,
            spatial_audio_enabled: true,
            enabled: true,
            preferred_device: None,
            sample_rate: None,
            buffer_size: None,
        }
    }
}

// ============================================================================
// RUNTIME RESOURCE
// ============================================================================

/// Runtime audio configuration resource.
///
/// This is the live, in-memory representation of audio settings that systems
/// read each frame. Changes made via the settings panel are written here
/// *and* persisted back to `config.json`.
///
/// The resource is initialised from [`AudioSettings`] during
/// [`AudioConfigPlugin`] setup and kept in sync via hot-reload.
#[derive(Resource, Clone, Debug)]
pub struct AudioConfig {
    // ── Volume Controls ──
    /// Master volume (0.0–1.0)
    pub master_volume: f32,
    /// Biome ambience intensity (0.0–1.0)
    pub ambience_intensity: f32,
    /// Music volume (0.0–1.0)
    pub music_volume: f32,
    /// Sound effects volume (0.0–1.0)
    pub sfx_volume: f32,

    // ── Spatial Audio ──
    /// Distance falloff exponent for 3D audio
    pub distance_falloff: f32,
    /// Whether spatial audio processing is enabled
    pub spatial_audio_enabled: bool,

    // ── Device Configuration ──
    /// Whether the audio subsystem is enabled
    pub enabled: bool,
    /// Preferred audio output device name
    pub preferred_device: Option<String>,
    /// Preferred sample rate in Hz
    pub sample_rate: Option<u32>,
    /// Preferred buffer size in frames
    pub buffer_size: Option<u32>,

    // ── Internal State ──
    /// Dirty flag — set when the settings panel changes a value,
    /// cleared after persisting to disk.
    pub dirty: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self::from_settings(&AudioSettings::default())
    }
}

impl AudioConfig {
    /// Create an [`AudioConfig`] resource from serialized settings.
    pub fn from_settings(s: &AudioSettings) -> Self {
        Self {
            master_volume: s.master_volume.clamp(0.0, 1.0),
            ambience_intensity: s.ambience_intensity.clamp(0.0, 1.0),
            music_volume: s.music_volume.clamp(0.0, 1.0),
            sfx_volume: s.sfx_volume.clamp(0.0, 1.0),
            distance_falloff: s.distance_falloff.clamp(0.1, 5.0),
            spatial_audio_enabled: s.spatial_audio_enabled,
            enabled: s.enabled,
            preferred_device: s.preferred_device.clone(),
            sample_rate: s.sample_rate,
            buffer_size: s.buffer_size,
            dirty: false,
        }
    }

    /// Copy current runtime values back into a serializable [`AudioSettings`].
    pub fn to_settings(&self) -> AudioSettings {
        AudioSettings {
            master_volume: self.master_volume,
            ambience_intensity: self.ambience_intensity,
            music_volume: self.music_volume,
            sfx_volume: self.sfx_volume,
            distance_falloff: self.distance_falloff,
            spatial_audio_enabled: self.spatial_audio_enabled,
            enabled: self.enabled,
            preferred_device: self.preferred_device.clone(),
            sample_rate: self.sample_rate,
            buffer_size: self.buffer_size,
        }
    }

    /// Compute the effective ambience volume (master × ambience).
    pub fn effective_ambience_volume(&self) -> f32 {
        self.master_volume * self.ambience_intensity
    }

    /// Effective music volume (master × music).
    pub fn effective_music_volume(&self) -> f32 {
        self.master_volume * self.music_volume
    }

    /// Effective SFX volume (master × sfx).
    pub fn effective_sfx_volume(&self) -> f32 {
        self.master_volume * self.sfx_volume
    }
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that initialises the [`AudioConfig`] resource from engine config
/// and keeps it synchronised.
pub struct AudioConfigPlugin;

impl Plugin for AudioConfigPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AudioConfig>()
            .init_resource::<AudioSettingsPanelState>()
            .add_systems(PostStartup, apply_audio_config_from_engine)
            .add_systems(
                Update,
                (
                    toggle_audio_settings_panel,
                    audio_settings_panel_ui,
                    persist_audio_config.run_if(audio_config_dirty),
                ),
            );
    }
}

// ============================================================================
// SETTINGS PANEL STATE
// ============================================================================

/// Tracks whether the audio settings panel is open.
#[derive(Resource, Default)]
pub struct AudioSettingsPanelState {
    /// Whether the panel window is visible. Toggle with F9.
    pub visible: bool,
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// Initialise [`AudioConfig`] from [`EngineConfig`] at startup.
fn apply_audio_config_from_engine(
    engine_config: Res<super::EngineConfig>,
    mut audio_config: ResMut<AudioConfig>,
) {
    *audio_config = AudioConfig::from_settings(&engine_config.audio);
    info!(
        "Audio config applied: master={:.0}%, ambience={:.0}%, sfx={:.0}%, spatial={}, device={:?}",
        audio_config.master_volume * 100.0,
        audio_config.ambience_intensity * 100.0,
        audio_config.sfx_volume * 100.0,
        audio_config.spatial_audio_enabled,
        audio_config.preferred_device,
    );
}

/// Toggle audio settings panel with F9.
fn toggle_audio_settings_panel(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut panel_state: ResMut<AudioSettingsPanelState>,
) {
    if keyboard.just_pressed(KeyCode::F9) {
        panel_state.visible = !panel_state.visible;
        info!(
            "Audio settings panel: {}",
            if panel_state.visible {
                "opened"
            } else {
                "closed"
            }
        );
    }
}

/// Render the audio settings panel using egui.
fn audio_settings_panel_ui(
    mut contexts: bevy_egui::EguiContexts,
    mut audio_config: ResMut<AudioConfig>,
    panel_state: Res<AudioSettingsPanelState>,
) {
    if !panel_state.visible {
        return;
    }

    bevy_egui::egui::Window::new("🔊 Audio Settings")
        .default_width(300.0)
        .resizable(false)
        .collapsible(true)
        .show(contexts.ctx_mut(), |ui| {
            ui.spacing_mut().slider_width = 180.0;

            // ── Master Volume ──
            ui.heading("Volume");
            ui.separator();

            let mut changed = false;

            ui.horizontal(|ui| {
                ui.label("Master:");
                let slider =
                    bevy_egui::egui::Slider::new(&mut audio_config.master_volume, 0.0..=1.0)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0));
                if ui.add(slider).changed() {
                    changed = true;
                }
            });

            ui.horizontal(|ui| {
                ui.label("Music:");
                let slider =
                    bevy_egui::egui::Slider::new(&mut audio_config.music_volume, 0.0..=1.0)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0));
                if ui.add(slider).changed() {
                    changed = true;
                }
            });

            ui.horizontal(|ui| {
                ui.label("SFX:");
                let slider = bevy_egui::egui::Slider::new(&mut audio_config.sfx_volume, 0.0..=1.0)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0));
                if ui.add(slider).changed() {
                    changed = true;
                }
            });

            ui.add_space(8.0);

            // ── Ambience ──
            ui.heading("Ambience");
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Biome Intensity:");
                let slider =
                    bevy_egui::egui::Slider::new(&mut audio_config.ambience_intensity, 0.0..=1.0)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0));
                if ui.add(slider).changed() {
                    changed = true;
                }
            });

            ui.add_space(8.0);

            // ── Spatial Audio ──
            ui.heading("3D Audio");
            ui.separator();

            if ui
                .checkbox(
                    &mut audio_config.spatial_audio_enabled,
                    "Enable Spatial Audio",
                )
                .changed()
            {
                changed = true;
            }

            ui.horizontal(|ui| {
                ui.label("Distance Falloff:");
                let slider =
                    bevy_egui::egui::Slider::new(&mut audio_config.distance_falloff, 0.1..=3.0)
                        .custom_formatter(|v, _| format!("{:.2}×", v));
                if ui.add(slider).changed() {
                    changed = true;
                }
            });

            ui.add_space(8.0);

            // ── Effective Volumes (read-only) ──
            ui.separator();
            ui.label(
                bevy_egui::egui::RichText::new("Effective Volumes")
                    .strong()
                    .size(12.0),
            );
            ui.horizontal(|ui| {
                ui.label("Ambience:");
                ui.monospace(format!(
                    "{:.0}%",
                    audio_config.effective_ambience_volume() * 100.0
                ));
            });
            ui.horizontal(|ui| {
                ui.label("Music:");
                ui.monospace(format!(
                    "{:.0}%",
                    audio_config.effective_music_volume() * 100.0
                ));
            });
            ui.horizontal(|ui| {
                ui.label("SFX:");
                ui.monospace(format!(
                    "{:.0}%",
                    audio_config.effective_sfx_volume() * 100.0
                ));
            });

            ui.add_space(4.0);

            // ── Reset to defaults ──
            ui.separator();
            if ui.button("Reset to Defaults").clicked() {
                *audio_config = AudioConfig::default();
                changed = true;
            }

            ui.small("F9 to toggle this panel");

            if changed {
                audio_config.dirty = true;
            }
        });
}

/// Run condition: returns `true` when the audio config has unsaved changes.
fn audio_config_dirty(audio_config: Res<AudioConfig>) -> bool {
    audio_config.dirty
}

/// Persist dirty audio config changes back to `config.json`.
fn persist_audio_config(
    mut audio_config: ResMut<AudioConfig>,
    mut engine_config: ResMut<super::EngineConfig>,
) {
    // Copy runtime values into the engine config
    engine_config.audio = audio_config.to_settings();

    // Save the entire engine config to disk
    match engine_config.save() {
        Ok(()) => {
            info!("Audio settings saved to config.json");
        }
        Err(e) => {
            warn!("Failed to save audio config: {}", e);
        }
    }

    audio_config.dirty = false;
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_settings_defaults() {
        let settings = AudioSettings::default();
        assert_eq!(settings.master_volume, 0.8);
        assert_eq!(settings.ambience_intensity, 0.6);
        assert_eq!(settings.distance_falloff, 1.0);
        assert!(settings.spatial_audio_enabled);
        assert_eq!(settings.music_volume, 0.5);
        assert_eq!(settings.sfx_volume, 0.7);
        assert!(settings.enabled);
        assert_eq!(settings.preferred_device, None);
        assert_eq!(settings.sample_rate, None);
        assert_eq!(settings.buffer_size, None);
    }

    #[test]
    fn test_audio_config_from_settings() {
        let settings = AudioSettings {
            master_volume: 0.5,
            ambience_intensity: 0.3,
            distance_falloff: 2.0,
            spatial_audio_enabled: false,
            music_volume: 0.4,
            sfx_volume: 0.6,
            enabled: true,
            preferred_device: Some("My Speakers".into()),
            sample_rate: Some(48000),
            buffer_size: Some(1024),
        };
        let config = AudioConfig::from_settings(&settings);
        assert_eq!(config.master_volume, 0.5);
        assert_eq!(config.ambience_intensity, 0.3);
        assert_eq!(config.distance_falloff, 2.0);
        assert!(!config.spatial_audio_enabled);
        assert_eq!(config.music_volume, 0.4);
        assert_eq!(config.sfx_volume, 0.6);
        assert!(config.enabled);
        assert_eq!(config.preferred_device, Some("My Speakers".into()));
        assert_eq!(config.sample_rate, Some(48000));
        assert_eq!(config.buffer_size, Some(1024));
        assert!(!config.dirty);
    }

    #[test]
    fn test_audio_config_clamping() {
        let settings = AudioSettings {
            master_volume: 1.5,       // over max
            ambience_intensity: -0.2, // under min
            distance_falloff: 10.0,   // over max (clamped to 5.0)
            spatial_audio_enabled: true,
            music_volume: 2.0,
            sfx_volume: -1.0,
            enabled: true,
            preferred_device: None,
            sample_rate: None,
            buffer_size: None,
        };
        let config = AudioConfig::from_settings(&settings);
        assert_eq!(config.master_volume, 1.0);
        assert_eq!(config.ambience_intensity, 0.0);
        assert_eq!(config.distance_falloff, 5.0);
        assert_eq!(config.music_volume, 1.0);
        assert_eq!(config.sfx_volume, 0.0);
    }

    #[test]
    fn test_audio_config_roundtrip() {
        let original = AudioSettings {
            master_volume: 0.75,
            ambience_intensity: 0.4,
            distance_falloff: 1.5,
            spatial_audio_enabled: true,
            music_volume: 0.6,
            sfx_volume: 0.8,
            enabled: true,
            preferred_device: Some("Headphones".into()),
            sample_rate: Some(44100),
            buffer_size: Some(512),
        };
        let config = AudioConfig::from_settings(&original);
        let restored = config.to_settings();
        assert_eq!(original, restored);
    }

    #[test]
    fn test_audio_settings_serialization_roundtrip() {
        let original = AudioSettings::default();
        let json = serde_json::to_string_pretty(&original).unwrap();
        let deserialized: AudioSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_audio_settings_partial_json() {
        // Only master_volume specified — rest should get defaults
        let json = r#"{ "master_volume": 0.5 }"#;
        let settings: AudioSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.master_volume, 0.5);
        assert_eq!(settings.ambience_intensity, 0.6); // default
        assert_eq!(settings.distance_falloff, 1.0); // default
        assert!(settings.spatial_audio_enabled); // default
        assert!(settings.enabled); // default
        assert_eq!(settings.preferred_device, None); // default
    }

    #[test]
    fn test_audio_config_in_engine_config() {
        // Ensure AudioSettings integrates with EngineConfig
        let json = r#"{
            "audio": {
                "master_volume": 0.3,
                "ambience_intensity": 0.9,
                "distance_falloff": 2.5,
                "preferred_device": "HDMI Output"
            }
        }"#;
        let config: super::super::EngineConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.audio.master_volume, 0.3);
        assert_eq!(config.audio.ambience_intensity, 0.9);
        assert_eq!(config.audio.distance_falloff, 2.5);
        assert_eq!(config.audio.preferred_device, Some("HDMI Output".into()));
        // Default for unspecified fields
        assert!(config.audio.spatial_audio_enabled);
        assert!(config.audio.enabled);
    }

    #[test]
    fn test_effective_volumes() {
        let config = AudioConfig {
            master_volume: 0.5,
            ambience_intensity: 0.8,
            distance_falloff: 1.0,
            spatial_audio_enabled: true,
            music_volume: 0.6,
            sfx_volume: 1.0,
            enabled: true,
            preferred_device: None,
            sample_rate: None,
            buffer_size: None,
            dirty: false,
        };
        assert!((config.effective_ambience_volume() - 0.4).abs() < f32::EPSILON);
        assert!((config.effective_music_volume() - 0.3).abs() < f32::EPSILON);
        assert!((config.effective_sfx_volume() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_audio_config_dirty_flag() {
        let mut config = AudioConfig::default();
        assert!(!config.dirty);
        config.dirty = true;
        assert!(config.dirty);
        config.dirty = false;
        assert!(!config.dirty);
    }

    #[test]
    fn test_audio_config_plugin_builds() {
        // Verify the plugin can be added to a minimal app without panicking
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<super::super::EngineConfig>();
        app.add_plugins(AudioConfigPlugin);
        // Should not panic
    }

    #[test]
    fn test_audio_settings_device_config_roundtrip() {
        let config = AudioSettings {
            preferred_device: Some("My Speakers".into()),
            master_volume: 0.65,
            sample_rate: Some(48000),
            buffer_size: Some(1024),
            enabled: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: AudioSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_engine_config_missing_audio_section_uses_defaults() {
        let json = r#"{ "window": { "title": "Test" } }"#;
        let config: super::super::EngineConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.audio, AudioSettings::default());
    }

    // ----------------------------------------------------------------
    // Audio mixer panel integration
    // ----------------------------------------------------------------

    #[test]
    fn test_audio_settings_panel_state_default_hidden() {
        let state = AudioSettingsPanelState::default();
        assert!(
            !state.visible,
            "Audio settings panel should be hidden by default"
        );
    }

    #[test]
    fn test_audio_settings_panel_toggle() {
        let mut state = AudioSettingsPanelState::default();
        assert!(!state.visible);
        state.visible = true;
        assert!(state.visible);
        state.visible = false;
        assert!(!state.visible);
    }

    #[test]
    fn test_volume_slider_range_produces_valid_effective_volumes() {
        // Simulate sweeping all sliders across their full range
        for master_pct in 0..=10 {
            for channel_pct in 0..=10 {
                let master = master_pct as f32 / 10.0;
                let channel = channel_pct as f32 / 10.0;
                let config = AudioConfig {
                    master_volume: master,
                    ambience_intensity: channel,
                    sfx_volume: channel,
                    music_volume: channel,
                    ..AudioConfig::default()
                };
                let eff_amb = config.effective_ambience_volume();
                let eff_sfx = config.effective_sfx_volume();
                let eff_mus = config.effective_music_volume();

                // Effective volumes must stay in [0.0, 1.0]
                assert!(
                    (0.0..=1.0).contains(&eff_amb),
                    "Ambient out of range: {} (master={}, channel={})",
                    eff_amb,
                    master,
                    channel,
                );
                assert!(
                    (0.0..=1.0).contains(&eff_sfx),
                    "SFX out of range: {} (master={}, channel={})",
                    eff_sfx,
                    master,
                    channel,
                );
                assert!(
                    (0.0..=1.0).contains(&eff_mus),
                    "Music out of range: {} (master={}, channel={})",
                    eff_mus,
                    master,
                    channel,
                );
            }
        }
    }

    #[test]
    fn test_config_dirty_flag_set_on_slider_change_simulation() {
        // Simulate the UI flow: user changes a slider → dirty = true → persist → dirty = false
        let mut config = AudioConfig::default();
        assert!(!config.dirty);

        // Step 1: User drags master slider
        config.master_volume = 0.5;
        config.dirty = true;
        assert!(config.dirty);

        // Step 2: Persistence system saves and clears dirty flag
        let settings = config.to_settings();
        assert_eq!(settings.master_volume, 0.5);
        config.dirty = false;
        assert!(!config.dirty);

        // Step 3: User drags SFX slider
        config.sfx_volume = 0.3;
        config.dirty = true;
        assert!(config.dirty);
        assert_eq!(config.to_settings().sfx_volume, 0.3);
    }

    #[test]
    fn test_reset_to_defaults_restores_all_volumes() {
        let config = AudioConfig {
            master_volume: 0.1,
            sfx_volume: 0.2,
            ambience_intensity: 0.3,
            music_volume: 0.4,
            ..AudioConfig::default()
        };

        // Verify custom values are set
        assert_eq!(config.master_volume, 0.1);
        assert_eq!(config.sfx_volume, 0.2);

        // Simulate "Reset to Defaults" button (replaces the entire config)
        let config = AudioConfig::default();

        assert_eq!(config.master_volume, 0.8);
        assert_eq!(config.sfx_volume, 0.7);
        assert_eq!(config.ambience_intensity, 0.6);
        assert_eq!(config.music_volume, 0.5);
    }

    #[test]
    fn test_to_settings_preserves_slider_values() {
        // Ensure that the values the UI writes to AudioConfig survive
        // the round-trip through to_settings → from_settings, which is
        // the path used by the persist system.
        let config = AudioConfig {
            master_volume: 0.42,
            ambience_intensity: 0.33,
            sfx_volume: 0.77,
            music_volume: 0.61,
            distance_falloff: 1.8,
            spatial_audio_enabled: false,
            enabled: true,
            preferred_device: None,
            sample_rate: None,
            buffer_size: None,
            dirty: true, // dirty flag should NOT survive the round-trip
        };

        let settings = config.to_settings();
        let restored = AudioConfig::from_settings(&settings);

        assert_eq!(restored.master_volume, 0.42);
        assert_eq!(restored.ambience_intensity, 0.33);
        assert_eq!(restored.sfx_volume, 0.77);
        assert_eq!(restored.music_volume, 0.61);
        assert_eq!(restored.distance_falloff, 1.8);
        assert!(!restored.spatial_audio_enabled);
        // dirty flag is NOT serialized — from_settings always sets it to false
        assert!(!restored.dirty);
    }
}
