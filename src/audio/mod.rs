//! Audio subsystem — playback, configuration, and device validation
//!
//! This module provides the complete audio system for the Procedural Worlds
//! Engine, combining three key capabilities:
//!
//! - **Playback** ([`playback`]) — Event-driven sound effects (block
//!   place/break) with positional attenuation, and ambient biome soundscapes
//!   that change as the player moves through different biomes.
//!
//! - **Configuration** ([`crate::config::audio`]) — Serializable audio
//!   settings persisted in `config.json`, with a runtime settings panel
//!   (F9) and hot-reload support.
//!
//! - **Validation** ([`validation`]) — Startup-time validation of audio
//!   configuration values, device detection, and graceful fallback when
//!   the preferred output device is unavailable.
//!
//! # Architecture
//!
//! ```text
//! config.json → AudioSettings (deserialized)
//!                    ↓
//!           AudioConfigPlugin (PostStartup)
//!                    ↓
//!           AudioConfig resource (runtime)
//!                    ↓
//!    ┌───────────────┴───────────────┐
//!    ▼                               ▼
//! AudioPlaybackPlugin          AudioValidationPlugin
//!  - BlockSoundEvent            - Device probing
//!  - Biome ambient sounds       - Config validation
//!  - Volume from AudioConfig    - AudioDeviceStatus
//! ```
//!
//! # Usage
//!
//! Add [`AudioPlugin`] to your Bevy app to enable the complete audio system:
//!
//! ```rust,ignore
//! app.add_plugins(audio::AudioPlugin);
//! ```
//!
//! The audio configuration plugin ([`crate::config::audio::AudioConfigPlugin`])
//! is registered separately from the main config plugin to maintain clean
//! separation of concerns.

pub mod ambient;
pub mod playback;
pub mod validation;

// Re-export key types for convenience
pub use ambient::{
    ActivityChangedEvent, ActivitySoundAssets, AmbientAudioPlugin, BiomeAudioProfile,
    PlayerActivityState, PlayerAudioState, WindSound, WindSoundState,
};
pub use playback::{
    AmbientSound, AudioListenerMarker, AudioPlaybackPlugin, BiomeAmbientAssets, BlockSoundAssets,
    BlockSoundEvent, BlockSoundKind, CurrentAmbientSound, SpatialSfx,
};
pub use validation::{
    AudioDeviceStatus, AudioValidationPlugin, DeviceState, ValidationResult, resolve_device_state,
    validate_audio_config,
};

use bevy::prelude::*;

// ============================================================================
// COMBINED PLUGIN
// ============================================================================

/// Combined audio plugin that registers all audio subsystems.
///
/// This is the recommended way to add audio to the engine. It includes:
/// - [`AudioPlaybackPlugin`] — sound effects and ambient biome sounds
/// - [`AudioValidationPlugin`] — device validation and fallback
///
/// Note: [`AudioConfigPlugin`](crate::config::audio::AudioConfigPlugin)
/// should be added separately alongside the engine's config system.
pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(AudioPlaybackPlugin)
            .add_plugins(AudioValidationPlugin)
            .add_plugins(AmbientAudioPlugin);
    }
}
