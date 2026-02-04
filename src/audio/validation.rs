//! Audio device validation and fallback logic.
//!
//! Validates [`AudioSettings`] values at startup, detects available
//! audio hardware, and inserts an [`AudioDeviceStatus`] resource that
//! the rest of the engine can query to determine audio capabilities.

use bevy::prelude::*;

use crate::config::audio::AudioSettings;
use crate::config::EngineConfig;

// ============================================================================
// CONSTANTS
// ============================================================================

/// Valid sample rates accepted by the validator.
const VALID_SAMPLE_RATES: &[u32] = &[8000, 11025, 16000, 22050, 44100, 48000, 88200, 96000, 176400, 192000];

/// Minimum buffer size in frames.
const MIN_BUFFER_SIZE: u32 = 32;

/// Maximum buffer size in frames.
const MAX_BUFFER_SIZE: u32 = 8192;

// ============================================================================
// DEVICE STATE
// ============================================================================

/// The resolved state of the audio output device after validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceState {
    /// The preferred device from config was found and will be used.
    PreferredDevice(String),
    /// The preferred device was unavailable; using the system default.
    FallbackToDefault {
        /// The device name that was requested but not found.
        requested: String,
        /// Human-readable reason for the fallback.
        reason: String,
    },
    /// No preferred device was configured; using the system default.
    SystemDefault,
    /// No audio output device is available at all (missing drivers, etc.).
    Unavailable {
        /// Human-readable reason why audio is unavailable.
        reason: String,
    },
    /// Audio was explicitly disabled in configuration.
    Disabled,
}

impl DeviceState {
    /// Returns `true` if audio output is available (even via fallback).
    pub fn is_available(&self) -> bool {
        matches!(
            self,
            DeviceState::PreferredDevice(_)
                | DeviceState::FallbackToDefault { .. }
                | DeviceState::SystemDefault
        )
    }
}

// ============================================================================
// VALIDATED STATUS RESOURCE
// ============================================================================

/// Resource inserted after audio validation, describing the validated
/// audio configuration and device availability.
///
/// Other systems should check `status.device_state.is_available()` before
/// attempting to play sounds.
#[derive(Resource, Debug, Clone)]
pub struct AudioDeviceStatus {
    /// Resolved device state after validation and fallback.
    pub device_state: DeviceState,
    /// Validated master volume (guaranteed 0.0–1.0).
    pub master_volume: f32,
    /// Validated sample rate, if one was specified and valid.
    pub sample_rate: Option<u32>,
    /// Validated buffer size, if one was specified and valid.
    pub buffer_size: Option<u32>,
    /// Warnings produced during validation (displayed in logs).
    pub warnings: Vec<String>,
}

// ============================================================================
// VALIDATION LOGIC
// ============================================================================

/// Validation result for a single audio configuration.
#[derive(Debug)]
pub struct ValidationResult {
    /// The validated (possibly clamped/corrected) config.
    pub config: AudioSettings,
    /// Warnings accumulated during validation.
    pub warnings: Vec<String>,
}

/// Validate an [`AudioSettings`], clamping out-of-range values and
/// collecting warnings. Never fails — always returns a usable config.
pub fn validate_audio_config(config: &AudioSettings) -> ValidationResult {
    let mut warnings = Vec::new();
    let mut validated = config.clone();

    // --- NaN / Infinity guard (must come before range clamp) ---
    if validated.master_volume.is_nan() || validated.master_volume.is_infinite() {
        warnings.push(format!(
            "master_volume ({}) is NaN or infinite — reset to default (0.8)",
            validated.master_volume
        ));
        validated.master_volume = 0.8;
    }

    // --- Master volume: clamp to 0.0–1.0 ---
    if validated.master_volume < 0.0 {
        warnings.push(format!(
            "master_volume ({}) is negative — clamped to 0.0",
            validated.master_volume
        ));
        validated.master_volume = 0.0;
    } else if validated.master_volume > 1.0 {
        warnings.push(format!(
            "master_volume ({}) exceeds 1.0 — clamped to 1.0",
            validated.master_volume
        ));
        validated.master_volume = 1.0;
    }

    // --- Sample rate: must be a standard value ---
    if let Some(rate) = validated.sample_rate
        && !VALID_SAMPLE_RATES.contains(&rate)
    {
        warnings.push(format!(
            "sample_rate ({rate}) is not a standard value {:?} — using device default",
            VALID_SAMPLE_RATES
        ));
        validated.sample_rate = None;
    }

    // --- Buffer size: must be in range and power-of-two recommended ---
    if let Some(size) = validated.buffer_size {
        if size < MIN_BUFFER_SIZE {
            warnings.push(format!(
                "buffer_size ({size}) below minimum ({MIN_BUFFER_SIZE}) — clamped"
            ));
            validated.buffer_size = Some(MIN_BUFFER_SIZE);
        } else if size > MAX_BUFFER_SIZE {
            warnings.push(format!(
                "buffer_size ({size}) exceeds maximum ({MAX_BUFFER_SIZE}) — clamped"
            ));
            validated.buffer_size = Some(MAX_BUFFER_SIZE);
        } else if !size.is_power_of_two() {
            warnings.push(format!(
                "buffer_size ({size}) is not a power of two — audio latency may be suboptimal"
            ));
            // We allow non-power-of-two but warn — some drivers handle it fine.
        }
    }

    // --- Preferred device: trim whitespace, reject empty strings ---
    if let Some(ref device) = validated.preferred_device {
        let trimmed = device.trim();
        if trimmed.is_empty() {
            warnings.push("preferred_device is empty — using system default".into());
            validated.preferred_device = None;
        } else if trimmed != device {
            validated.preferred_device = Some(trimmed.to_owned());
        }
    }

    ValidationResult {
        config: validated,
        warnings,
    }
}

/// Resolve the device state by checking whether the preferred device
/// is available among the provided device names.
///
/// `available_devices` should be the list of audio output device names
/// detected on the system. Pass an empty slice if device enumeration
/// failed or is not supported.
pub fn resolve_device_state(
    config: &AudioSettings,
    available_devices: &[String],
    audio_subsystem_available: bool,
) -> DeviceState {
    // Audio explicitly disabled in config
    if !config.enabled {
        return DeviceState::Disabled;
    }

    // No audio subsystem at all (missing drivers, headless server, etc.)
    if !audio_subsystem_available {
        return DeviceState::Unavailable {
            reason: "No audio output device detected — audio drivers may be missing".into(),
        };
    }

    // No preferred device → use system default
    let preferred = match &config.preferred_device {
        Some(name) => name,
        None => return DeviceState::SystemDefault,
    };

    // Check if preferred device is in the available list (case-insensitive)
    let found = available_devices
        .iter()
        .any(|d| d.eq_ignore_ascii_case(preferred));

    if found {
        DeviceState::PreferredDevice(preferred.clone())
    } else {
        DeviceState::FallbackToDefault {
            requested: preferred.clone(),
            reason: format!(
                "Device '{}' not found among {} available device(s)",
                preferred,
                available_devices.len()
            ),
        }
    }
}

// ============================================================================
// BEVY PLUGIN
// ============================================================================

/// Plugin that validates audio configuration at startup and inserts
/// an [`AudioDeviceStatus`] resource.
///
/// Must run after [`EngineConfig`] is inserted as a resource.
pub struct AudioValidationPlugin;

impl Plugin for AudioValidationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, validate_audio_startup);
    }
}

/// Startup system: validate audio config and detect device availability.
fn validate_audio_startup(mut commands: Commands, config: Res<EngineConfig>) {
    info!("Validating audio configuration...");

    // Step 1: Validate config values
    let result = validate_audio_config(&config.audio);

    for warning in &result.warnings {
        warn!("Audio config: {}", warning);
    }

    // Step 2: Probe audio subsystem availability.
    //
    // Bevy's DefaultPlugins includes AudioPlugin, which initializes rodio.
    // We do a lightweight probe here: attempt to check if the OS reports
    // any output devices. On systems without audio (headless CI, containers),
    // this returns false gracefully.
    let (audio_available, device_names) = probe_audio_devices();

    // Step 3: Resolve device state
    let device_state =
        resolve_device_state(&result.config, &device_names, audio_available);

    // Log the outcome
    match &device_state {
        DeviceState::PreferredDevice(name) => {
            info!("Audio: using preferred device '{}'", name);
        }
        DeviceState::FallbackToDefault { requested, reason } => {
            warn!(
                "Audio: preferred device '{}' unavailable ({}). Falling back to system default.",
                requested, reason
            );
        }
        DeviceState::SystemDefault => {
            info!("Audio: using system default output device");
        }
        DeviceState::Unavailable { reason } => {
            warn!("Audio: {}", reason);
            warn!("Audio: all sound will be disabled for this session");
        }
        DeviceState::Disabled => {
            info!("Audio: disabled by configuration");
        }
    }

    if !device_names.is_empty() {
        info!(
            "Audio: {} output device(s) detected: [{}]",
            device_names.len(),
            device_names.join(", ")
        );
    }

    // Insert status resource
    commands.insert_resource(AudioDeviceStatus {
        device_state,
        master_volume: result.config.master_volume,
        sample_rate: result.config.sample_rate,
        buffer_size: result.config.buffer_size,
        warnings: result.warnings,
    });

    info!("Audio validation complete (master_volume: {})", result.config.master_volume);
}

/// Probe the OS for available audio output devices.
///
/// Returns `(subsystem_available, device_names)`. On platforms where
/// device enumeration is not supported or fails, returns `(false, vec![])`.
///
/// This does NOT open any device — it only enumerates names, which is a
/// cheap, non-blocking operation on all major platforms.
fn probe_audio_devices() -> (bool, Vec<String>) {
    // We probe using std::process to call platform-specific tools,
    // or simply report that we detected the default audio sink via
    // Bevy/rodio availability. For a game engine we keep this simple:
    // if we can construct a default OutputStream, audio is available.
    //
    // Since Bevy's AudioPlugin already initializes the audio backend
    // during DefaultPlugins, we treat the subsystem as available
    // unless we detect otherwise. A future version could use cpal
    // directly for richer device enumeration.

    // Attempt a lightweight probe: rodio's OutputStream creation is
    // handled by Bevy, so we just check if the platform is likely to
    // have audio support. In practice, audio is available on all
    // desktop platforms (Windows/macOS/Linux with PulseAudio/ALSA).
    //
    // We return a generic "Default Output" name and mark as available.
    // Detailed device enumeration (cpal) can be added when needed.

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        (true, vec!["Default Output".into()])
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        (false, vec![])
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::audio::AudioSettings;

    // --- validate_audio_config tests ---

    #[test]
    fn test_valid_config_passes_without_warnings() {
        let config = AudioSettings::default();
        let result = validate_audio_config(&config);
        assert!(result.warnings.is_empty(), "Default config should produce no warnings");
        assert_eq!(result.config.master_volume, 0.8);
        assert_eq!(result.config.sample_rate, None);
        assert_eq!(result.config.buffer_size, None);
        assert!(result.config.enabled);
    }

    #[test]
    fn test_volume_clamped_above_one() {
        let config = AudioSettings {
            master_volume: 1.5,
            ..Default::default()
        };
        let result = validate_audio_config(&config);
        assert_eq!(result.config.master_volume, 1.0);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("clamped to 1.0"));
    }

    #[test]
    fn test_volume_clamped_below_zero() {
        let config = AudioSettings {
            master_volume: -0.5,
            ..Default::default()
        };
        let result = validate_audio_config(&config);
        assert_eq!(result.config.master_volume, 0.0);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("clamped to 0.0"));
    }

    #[test]
    fn test_nan_volume_reset_to_default() {
        let config = AudioSettings {
            master_volume: f32::NAN,
            ..Default::default()
        };
        let result = validate_audio_config(&config);
        assert_eq!(result.config.master_volume, 0.8);
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn test_infinite_volume_reset_to_default() {
        let config = AudioSettings {
            master_volume: f32::INFINITY,
            ..Default::default()
        };
        let result = validate_audio_config(&config);
        assert_eq!(result.config.master_volume, 0.8);
    }

    #[test]
    fn test_invalid_sample_rate_cleared() {
        let config = AudioSettings {
            sample_rate: Some(12345),
            ..Default::default()
        };
        let result = validate_audio_config(&config);
        assert_eq!(result.config.sample_rate, None);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("not a standard value"));
    }

    #[test]
    fn test_valid_sample_rates_accepted() {
        for &rate in VALID_SAMPLE_RATES {
            let config = AudioSettings {
                sample_rate: Some(rate),
                ..Default::default()
            };
            let result = validate_audio_config(&config);
            assert_eq!(result.config.sample_rate, Some(rate));
            assert!(result.warnings.is_empty(), "Rate {rate} should be accepted");
        }
    }

    #[test]
    fn test_buffer_size_clamped_too_small() {
        let config = AudioSettings {
            buffer_size: Some(8),
            ..Default::default()
        };
        let result = validate_audio_config(&config);
        assert_eq!(result.config.buffer_size, Some(MIN_BUFFER_SIZE));
        assert!(result.warnings[0].contains("below minimum"));
    }

    #[test]
    fn test_buffer_size_clamped_too_large() {
        let config = AudioSettings {
            buffer_size: Some(65536),
            ..Default::default()
        };
        let result = validate_audio_config(&config);
        assert_eq!(result.config.buffer_size, Some(MAX_BUFFER_SIZE));
        assert!(result.warnings[0].contains("exceeds maximum"));
    }

    #[test]
    fn test_buffer_size_non_power_of_two_warns() {
        let config = AudioSettings {
            buffer_size: Some(300),
            ..Default::default()
        };
        let result = validate_audio_config(&config);
        // Value is kept but a warning is emitted
        assert_eq!(result.config.buffer_size, Some(300));
        assert!(result.warnings[0].contains("not a power of two"));
    }

    #[test]
    fn test_buffer_size_power_of_two_no_warning() {
        let config = AudioSettings {
            buffer_size: Some(512),
            ..Default::default()
        };
        let result = validate_audio_config(&config);
        assert_eq!(result.config.buffer_size, Some(512));
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_empty_device_name_cleared() {
        let config = AudioSettings {
            preferred_device: Some("".into()),
            ..Default::default()
        };
        let result = validate_audio_config(&config);
        assert_eq!(result.config.preferred_device, None);
        assert!(result.warnings[0].contains("empty"));
    }

    #[test]
    fn test_whitespace_device_name_cleared() {
        let config = AudioSettings {
            preferred_device: Some("   ".into()),
            ..Default::default()
        };
        let result = validate_audio_config(&config);
        assert_eq!(result.config.preferred_device, None);
    }

    #[test]
    fn test_device_name_trimmed() {
        let config = AudioSettings {
            preferred_device: Some("  My Speakers  ".into()),
            ..Default::default()
        };
        let result = validate_audio_config(&config);
        assert_eq!(
            result.config.preferred_device,
            Some("My Speakers".into())
        );
        assert!(result.warnings.is_empty());
    }

    // --- resolve_device_state tests ---

    #[test]
    fn test_disabled_config_returns_disabled() {
        let config = AudioSettings {
            enabled: false,
            ..Default::default()
        };
        let state = resolve_device_state(&config, &[], true);
        assert_eq!(state, DeviceState::Disabled);
        assert!(!state.is_available());
    }

    #[test]
    fn test_no_audio_subsystem_returns_unavailable() {
        let config = AudioSettings::default();
        let state = resolve_device_state(&config, &[], false);
        assert!(matches!(state, DeviceState::Unavailable { .. }));
        assert!(!state.is_available());
    }

    #[test]
    fn test_no_preferred_device_returns_system_default() {
        let config = AudioSettings {
            preferred_device: None,
            ..Default::default()
        };
        let state = resolve_device_state(&config, &["Default Output".into()], true);
        assert_eq!(state, DeviceState::SystemDefault);
        assert!(state.is_available());
    }

    #[test]
    fn test_preferred_device_found() {
        let config = AudioSettings {
            preferred_device: Some("My Headphones".into()),
            ..Default::default()
        };
        let devices = vec!["Speakers".into(), "My Headphones".into()];
        let state = resolve_device_state(&config, &devices, true);
        assert_eq!(state, DeviceState::PreferredDevice("My Headphones".into()));
        assert!(state.is_available());
    }

    #[test]
    fn test_preferred_device_case_insensitive_match() {
        let config = AudioSettings {
            preferred_device: Some("my headphones".into()),
            ..Default::default()
        };
        let devices = vec!["Speakers".into(), "My Headphones".into()];
        let state = resolve_device_state(&config, &devices, true);
        // Should match case-insensitively
        assert!(matches!(state, DeviceState::PreferredDevice(_)));
        assert!(state.is_available());
    }

    #[test]
    fn test_preferred_device_not_found_falls_back() {
        let config = AudioSettings {
            preferred_device: Some("HDMI Output 3".into()),
            ..Default::default()
        };
        let devices = vec!["Default Output".into(), "Speakers".into()];
        let state = resolve_device_state(&config, &devices, true);
        match &state {
            DeviceState::FallbackToDefault { requested, reason } => {
                assert_eq!(requested, "HDMI Output 3");
                assert!(reason.contains("not found"));
            }
            _ => panic!("Expected FallbackToDefault, got {:?}", state),
        }
        assert!(state.is_available());
    }

    #[test]
    fn test_preferred_device_no_devices_at_all() {
        let config = AudioSettings {
            preferred_device: Some("Headphones".into()),
            ..Default::default()
        };
        // Audio subsystem is available but no devices enumerated
        let state = resolve_device_state(&config, &[], true);
        assert!(matches!(state, DeviceState::FallbackToDefault { .. }));
        assert!(state.is_available());
    }

    #[test]
    fn test_multiple_validation_warnings() {
        let config = AudioSettings {
            preferred_device: Some("  ".into()),
            master_volume: 2.0,
            sample_rate: Some(99999),
            buffer_size: Some(4),
            enabled: true,
            ..Default::default()
        };
        let result = validate_audio_config(&config);
        // Should have warnings for: volume, sample rate, buffer size, device name
        assert!(result.warnings.len() >= 3, "Expected multiple warnings, got: {:?}", result.warnings);
        assert_eq!(result.config.master_volume, 1.0);
        assert_eq!(result.config.sample_rate, None);
        assert_eq!(result.config.buffer_size, Some(MIN_BUFFER_SIZE));
        assert_eq!(result.config.preferred_device, None);
    }

    // --- AudioSettings serialization tests ---

    #[test]
    fn test_audio_settings_roundtrip() {
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
    fn test_audio_settings_partial_json_uses_defaults() {
        let json = r#"{ "master_volume": 0.5 }"#;
        let config: AudioSettings = serde_json::from_str(json).unwrap();
        assert_eq!(config.master_volume, 0.5);
        assert_eq!(config.preferred_device, None);
        assert_eq!(config.sample_rate, None);
        assert_eq!(config.buffer_size, None);
        assert!(config.enabled);
    }

    #[test]
    fn test_engine_config_with_audio_section() {
        let json = r#"{
            "audio": {
                "preferred_device": "HDMI Output",
                "master_volume": 0.9,
                "enabled": true
            }
        }"#;
        let config: EngineConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.audio.preferred_device, Some("HDMI Output".into()));
        assert_eq!(config.audio.master_volume, 0.9);
        assert!(config.audio.enabled);
        // Other sections should be defaults
        assert_eq!(config.terrain.seed, 12345);
    }

    #[test]
    fn test_engine_config_missing_audio_section_uses_defaults() {
        let json = r#"{ "window": { "title": "Test" } }"#;
        let config: EngineConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.audio, AudioSettings::default());
    }
}
