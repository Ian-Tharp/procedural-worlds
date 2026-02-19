//! Context-aware ambient audio — dynamic adjustment based on biome and player activity
//!
//! Layers on top of the base biome ambient system in [`super::playback`],
//! adding:
//!
//! - **Volume modulation** — ambient volume adjusts based on player activity
//!   state (idle, walking, sprinting, flying). Each biome defines its own
//!   volume curve so that, e.g., forest ambience fades more during sprinting
//!   while desert ambience stays louder.
//!
//! - **Wind audio layer** — a looping wind-rush sound spawned when the player
//!   moves fast enough, with intensity that scales by speed and biome. Wind
//!   is always present when flying, but ground-level wind depends on the
//!   biome's `wind_responsive` flag (forests dampen wind, mountains amplify).
//!
//! - **Biome audio profiles** — per-biome tuning of activity volume scales
//!   and wind responsiveness via [`BiomeAudioProfile`].
//!
//! # Architecture
//!
//! ```text
//! Player (Velocity, Movement)
//!     ↓
//! detect_player_activity
//!     ↓
//! PlayerAudioState resource (activity + speed)
//!     ↓
//! ┌──────────────────────────────────────┐
//! │ adjust_ambient_for_activity          │ → modulates biome ambient volume
//! │ manage_wind_sound                    │ → wind layer for fast movement
//! └──────────────────────────────────────┘
//! ```
//!
//! # Sound Assets
//!
//! Expected in `assets/sounds/`:
//! - `wind_rush.ogg` — looping wind sound for sprinting / flying

use bevy::audio::AudioSink;
use bevy::prelude::*;

use crate::actors::{Movement, Player, Velocity};
use crate::config::audio::AudioConfig;
use crate::generation::biome::BiomeType;

use super::playback::{AmbientSound, CurrentAmbientSound};

// ============================================================================
// CONSTANTS
// ============================================================================

/// Horizontal speed below which the player is considered idle (blocks/sec).
const IDLE_SPEED_THRESHOLD: f32 = 0.5;

/// Horizontal speed at or above which the player is considered sprinting.
/// Sits between the default walk_speed (4.3) and sprint_speed (5.6).
const SPRINT_SPEED_THRESHOLD: f32 = 5.0;

/// Rate at which the smoothed volume scale converges to its target (per second).
/// Higher values produce snappier transitions; lower values produce gentler fades.
const VOLUME_LERP_SPEED: f32 = 3.0;

/// Wind volume as a fraction of the effective ambience volume.
const WIND_VOLUME_SCALE: f32 = 0.5;

/// Minimum horizontal speed (blocks/sec) for wind sounds to begin.
const WIND_SPEED_THRESHOLD: f32 = 4.0;

/// Speed at which wind reaches full intensity (blocks/sec).
const WIND_FULL_SPEED: f32 = 12.0;

// ============================================================================
// PLAYER ACTIVITY STATE
// ============================================================================

/// The player's detected movement state, used to drive audio behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PlayerActivityState {
    /// Stationary or moving very slowly.
    #[default]
    Idle,
    /// Moving at normal walking speed.
    Walking,
    /// Moving faster than walking speed (sprint-like).
    Sprinting,
    /// Flying mode is enabled.
    Flying,
}

impl PlayerActivityState {
    /// Returns `true` if the player is actively moving (not idle).
    pub fn is_moving(&self) -> bool {
        !matches!(self, Self::Idle)
    }
}

// ============================================================================
// BIOME AUDIO PROFILES
// ============================================================================

/// Per-biome configuration for how ambient audio responds to player activity.
///
/// Each biome can tune how much its ambient sound is dampened during
/// different activities, and whether wind sounds are prominent.
#[derive(Clone, Debug)]
pub struct BiomeAudioProfile {
    /// Ambient volume multiplier when idle (0.0–1.0).
    pub idle_volume_scale: f32,
    /// Ambient volume multiplier when walking (0.0–1.0).
    pub walking_volume_scale: f32,
    /// Ambient volume multiplier when sprinting (0.0–1.0).
    pub sprinting_volume_scale: f32,
    /// Ambient volume multiplier when flying (0.0–1.0).
    pub flying_volume_scale: f32,
    /// Whether this biome amplifies wind sounds during fast movement.
    pub wind_responsive: bool,
    /// Base wind intensity for this biome (0.0 = calm, 1.0 = stormy).
    /// Multiplied with speed-based wind to compute final wind volume.
    pub base_wind_intensity: f32,
}

impl BiomeAudioProfile {
    /// Return the ambient volume scale for a given activity state.
    pub fn volume_scale_for(&self, activity: PlayerActivityState) -> f32 {
        match activity {
            PlayerActivityState::Idle => self.idle_volume_scale,
            PlayerActivityState::Walking => self.walking_volume_scale,
            PlayerActivityState::Sprinting => self.sprinting_volume_scale,
            PlayerActivityState::Flying => self.flying_volume_scale,
        }
    }

    /// Return the default audio profile for a biome type.
    pub fn for_biome(biome: BiomeType) -> Self {
        match biome {
            // Plains — open grassland, moderate wind exposure
            BiomeType::Plains => Self {
                idle_volume_scale: 1.0,
                walking_volume_scale: 0.85,
                sprinting_volume_scale: 0.6,
                flying_volume_scale: 0.4,
                wind_responsive: true,
                base_wind_intensity: 0.3,
            },
            // Desert — exposed sand, more wind
            BiomeType::Desert => Self {
                idle_volume_scale: 1.0,
                walking_volume_scale: 0.9,
                sprinting_volume_scale: 0.7,
                flying_volume_scale: 0.45,
                wind_responsive: true,
                base_wind_intensity: 0.5,
            },
            // Forest — canopy dampens wind, ambient fades more during motion
            BiomeType::Forest => Self {
                idle_volume_scale: 1.0,
                walking_volume_scale: 0.8,
                sprinting_volume_scale: 0.55,
                flying_volume_scale: 0.35,
                wind_responsive: false,
                base_wind_intensity: 0.15,
            },
            // Mountains — high altitude, strong wind
            BiomeType::Mountains => Self {
                idle_volume_scale: 1.0,
                walking_volume_scale: 0.85,
                sprinting_volume_scale: 0.6,
                flying_volume_scale: 0.5,
                wind_responsive: true,
                base_wind_intensity: 0.6,
            },
            // Tundra — flat, exposed, harsh persistent wind
            BiomeType::Tundra => Self {
                idle_volume_scale: 1.0,
                walking_volume_scale: 0.9,
                sprinting_volume_scale: 0.65,
                flying_volume_scale: 0.45,
                wind_responsive: true,
                base_wind_intensity: 0.7,
            },
            // Volcanic — rumbling dominates, wind is secondary
            BiomeType::Volcanic => Self {
                idle_volume_scale: 1.0,
                walking_volume_scale: 0.85,
                sprinting_volume_scale: 0.65,
                flying_volume_scale: 0.5,
                wind_responsive: false,
                base_wind_intensity: 0.2,
            },
            BiomeType::Swamp => Self { idle_volume_scale: 1.0, walking_volume_scale: 0.85, sprinting_volume_scale: 0.6, flying_volume_scale: 0.4, wind_responsive: false, base_wind_intensity: 0.1 },
            BiomeType::Savanna => Self { idle_volume_scale: 1.0, walking_volume_scale: 0.85, sprinting_volume_scale: 0.6, flying_volume_scale: 0.4, wind_responsive: true, base_wind_intensity: 0.35 },
            BiomeType::Taiga => Self { idle_volume_scale: 1.0, walking_volume_scale: 0.85, sprinting_volume_scale: 0.6, flying_volume_scale: 0.4, wind_responsive: false, base_wind_intensity: 0.2 },
            BiomeType::Jungle => Self { idle_volume_scale: 1.0, walking_volume_scale: 0.8, sprinting_volume_scale: 0.5, flying_volume_scale: 0.35, wind_responsive: false, base_wind_intensity: 0.1 },
            BiomeType::Badlands => Self { idle_volume_scale: 1.0, walking_volume_scale: 0.9, sprinting_volume_scale: 0.7, flying_volume_scale: 0.45, wind_responsive: true, base_wind_intensity: 0.55 },
            BiomeType::Mushroom => Self { idle_volume_scale: 1.0, walking_volume_scale: 0.9, sprinting_volume_scale: 0.7, flying_volume_scale: 0.5, wind_responsive: false, base_wind_intensity: 0.05 },
        }
    }
}

// ============================================================================
// RESOURCES
// ============================================================================

/// Tracks the player's current activity state for audio modulation.
///
/// Updated each frame by [`detect_player_activity`]. Other audio systems
/// read this to adjust volumes and trigger activity-specific sounds.
#[derive(Resource, Debug)]
pub struct PlayerAudioState {
    /// Current detected activity.
    pub activity: PlayerActivityState,
    /// Horizontal speed in blocks per second.
    pub horizontal_speed: f32,
    /// Activity from the previous frame (for change detection).
    pub previous_activity: PlayerActivityState,
    /// Smoothed volume scale, interpolating toward the target each frame.
    pub current_volume_scale: f32,
    /// Target volume scale based on current activity and biome.
    pub target_volume_scale: f32,
}

impl Default for PlayerAudioState {
    fn default() -> Self {
        Self {
            activity: PlayerActivityState::Idle,
            horizontal_speed: 0.0,
            previous_activity: PlayerActivityState::Idle,
            current_volume_scale: 1.0,
            target_volume_scale: 1.0,
        }
    }
}

/// Pre-loaded audio assets for activity-based sound layers.
#[derive(Resource, Default)]
pub struct ActivitySoundAssets {
    /// Looping wind-rush sound for fast movement.
    pub wind_rush: Option<Handle<AudioSource>>,
}

/// Tracks the wind-rush sound entity and its desired state.
#[derive(Resource, Default)]
pub struct WindSoundState {
    /// Entity currently playing the wind loop (if any).
    pub entity: Option<Entity>,
    /// Whether wind sound should be active based on current conditions.
    pub should_be_active: bool,
}

// ============================================================================
// COMPONENTS
// ============================================================================

/// Marker component for the wind-rush sound entity.
#[derive(Component)]
pub struct WindSound;

// ============================================================================
// EVENTS
// ============================================================================

/// Emitted when the player's activity state transitions.
///
/// Downstream systems can listen for this to trigger one-shot transition
/// sounds (e.g., a whoosh when starting to sprint) or other audio effects.
#[derive(Event, Clone, Debug)]
pub struct ActivityChangedEvent {
    /// The activity state being left.
    pub from: PlayerActivityState,
    /// The activity state being entered.
    pub to: PlayerActivityState,
    /// The biome the player is currently in (if known).
    pub biome: Option<BiomeType>,
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that adds context-aware ambient audio based on biome and player activity.
///
/// Must be added alongside [`super::AudioPlaybackPlugin`], which provides
/// the base biome ambient system that this plugin modulates.
pub struct AmbientAudioPlugin;

impl Plugin for AmbientAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerAudioState>()
            .init_resource::<ActivitySoundAssets>()
            .init_resource::<WindSoundState>()
            .add_event::<ActivityChangedEvent>()
            .add_systems(Startup, load_activity_sound_assets)
            .add_systems(
                Update,
                (
                    detect_player_activity,
                    adjust_ambient_for_activity,
                    manage_wind_sound,
                )
                    .chain(),
            );
    }
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// Load activity-specific sound assets from the asset server.
fn load_activity_sound_assets(
    asset_server: Res<AssetServer>,
    mut assets: ResMut<ActivitySoundAssets>,
) {
    assets.wind_rush = Some(asset_server.load("sounds/wind_rush.ogg"));
    info!("Activity sound assets loading initiated");
}

/// Detect the player's activity state from velocity and movement components.
///
/// Updates [`PlayerAudioState`] each frame and emits [`ActivityChangedEvent`]
/// when the state transitions.
fn detect_player_activity(
    mut audio_state: ResMut<PlayerAudioState>,
    mut activity_events: EventWriter<ActivityChangedEvent>,
    player_query: Query<(&Velocity, &Movement), With<Player>>,
    current_ambient: Res<CurrentAmbientSound>,
) {
    let Ok((velocity, movement)) = player_query.get_single() else {
        return;
    };

    // Horizontal speed (XZ plane) drives activity detection
    let horizontal_speed = Vec2::new(velocity.linear.x, velocity.linear.z).length();
    audio_state.horizontal_speed = horizontal_speed;
    audio_state.previous_activity = audio_state.activity;

    let new_activity = classify_activity(horizontal_speed, movement.flying);

    if new_activity != audio_state.activity {
        activity_events.send(ActivityChangedEvent {
            from: audio_state.activity,
            to: new_activity,
            biome: current_ambient.active_biome,
        });

        info!(
            "Player activity changed: {:?} → {:?} (speed: {:.1})",
            audio_state.activity, new_activity, horizontal_speed
        );

        audio_state.activity = new_activity;
    }
}

/// Adjust the ambient biome sound volume based on current player activity.
///
/// Reads the biome's audio profile and smoothly interpolates the ambient
/// volume toward the target scale for the current activity state. Uses
/// [`AudioSink`] to modify volume on the live audio entity without
/// needing to despawn and respawn it.
fn adjust_ambient_for_activity(
    mut audio_state: ResMut<PlayerAudioState>,
    current_ambient: Res<CurrentAmbientSound>,
    audio_config: Res<AudioConfig>,
    time: Res<Time>,
    ambient_query: Query<&AudioSink, With<AmbientSound>>,
) {
    let Some(biome) = current_ambient.active_biome else {
        return;
    };

    let profile = BiomeAudioProfile::for_biome(biome);
    let target_scale = profile.volume_scale_for(audio_state.activity);
    audio_state.target_volume_scale = target_scale;

    // Smoothly interpolate current volume toward target
    let dt = time.delta_secs();
    let lerp_factor = (VOLUME_LERP_SPEED * dt).min(1.0);
    audio_state.current_volume_scale +=
        (target_scale - audio_state.current_volume_scale) * lerp_factor;

    // Apply the modulated volume to the ambient sound entity
    let effective_volume =
        audio_config.effective_ambience_volume() * audio_state.current_volume_scale;

    for sink in &ambient_query {
        sink.set_volume(effective_volume);
    }
}

/// Manage the wind-rush sound layer based on player speed and biome.
///
/// The wind sound is spawned when the player moves fast enough and the
/// biome supports wind audio (or the player is flying). Volume scales
/// with horizontal speed and the biome's wind intensity. The entity is
/// despawned when the player slows down.
fn manage_wind_sound(
    mut commands: Commands,
    mut wind_state: ResMut<WindSoundState>,
    audio_state: Res<PlayerAudioState>,
    current_ambient: Res<CurrentAmbientSound>,
    audio_config: Res<AudioConfig>,
    activity_assets: Res<ActivitySoundAssets>,
    wind_query: Query<&AudioSink, With<WindSound>>,
) {
    let biome_profile = current_ambient.active_biome.map(BiomeAudioProfile::for_biome);

    let should_be_active = calculate_wind_volume(
        audio_state.horizontal_speed,
        audio_state.activity,
        biome_profile.as_ref(),
        audio_config.effective_ambience_volume(),
    ) > 0.0;

    wind_state.should_be_active = should_be_active;

    if should_be_active {
        let wind_volume = calculate_wind_volume(
            audio_state.horizontal_speed,
            audio_state.activity,
            biome_profile.as_ref(),
            audio_config.effective_ambience_volume(),
        );

        if wind_state.entity.is_none() {
            // Spawn wind sound
            if let Some(source) = &activity_assets.wind_rush {
                let entity = commands
                    .spawn((
                        AudioPlayer(source.clone()),
                        PlaybackSettings {
                            mode: bevy::audio::PlaybackMode::Loop,
                            volume: bevy::audio::Volume::new(wind_volume),
                            ..default()
                        },
                        WindSound,
                    ))
                    .id();
                wind_state.entity = Some(entity);
            }
        } else {
            // Update wind volume on existing entity via AudioSink
            for sink in &wind_query {
                sink.set_volume(wind_volume);
            }
        }
    } else if let Some(entity) = wind_state.entity.take() {
        commands.entity(entity).despawn();
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Determine the activity state from horizontal speed and movement flags.
///
/// Extracted as a free function for testability — uses the same thresholds
/// as [`detect_player_activity`].
pub fn classify_activity(horizontal_speed: f32, flying: bool) -> PlayerActivityState {
    if flying {
        PlayerActivityState::Flying
    } else if horizontal_speed < IDLE_SPEED_THRESHOLD {
        PlayerActivityState::Idle
    } else if horizontal_speed >= SPRINT_SPEED_THRESHOLD {
        PlayerActivityState::Sprinting
    } else {
        PlayerActivityState::Walking
    }
}

/// Calculate the wind volume for a given speed, biome, and base volume.
///
/// Returns 0.0 when speed is below threshold, the biome doesn't support
/// wind (and the player isn't flying), or the base volume is zero.
pub fn calculate_wind_volume(
    horizontal_speed: f32,
    activity: PlayerActivityState,
    biome_profile: Option<&BiomeAudioProfile>,
    base_ambience_volume: f32,
) -> f32 {
    let wind_intensity = biome_profile
        .map(|p| p.base_wind_intensity)
        .unwrap_or(0.0);
    let biome_allows_wind = biome_profile.map(|p| p.wind_responsive).unwrap_or(false);

    // Wind plays when: fast enough AND (biome allows wind OR flying)
    let should_play = horizontal_speed > WIND_SPEED_THRESHOLD
        && (biome_allows_wind || activity == PlayerActivityState::Flying);

    if !should_play {
        return 0.0;
    }

    let speed_factor =
        ((horizontal_speed - WIND_SPEED_THRESHOLD) / (WIND_FULL_SPEED - WIND_SPEED_THRESHOLD))
            .clamp(0.0, 1.0);

    base_ambience_volume * WIND_VOLUME_SCALE * wind_intensity * speed_factor
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ----------------------------------------------------------------
    // PlayerActivityState
    // ----------------------------------------------------------------

    #[test]
    fn test_activity_state_default_is_idle() {
        assert_eq!(PlayerActivityState::default(), PlayerActivityState::Idle);
    }

    #[test]
    fn test_activity_state_is_moving() {
        assert!(!PlayerActivityState::Idle.is_moving());
        assert!(PlayerActivityState::Walking.is_moving());
        assert!(PlayerActivityState::Sprinting.is_moving());
        assert!(PlayerActivityState::Flying.is_moving());
    }

    #[test]
    fn test_activity_state_equality() {
        assert_eq!(PlayerActivityState::Idle, PlayerActivityState::Idle);
        assert_eq!(PlayerActivityState::Walking, PlayerActivityState::Walking);
        assert_ne!(PlayerActivityState::Idle, PlayerActivityState::Walking);
        assert_ne!(PlayerActivityState::Walking, PlayerActivityState::Sprinting);
        assert_ne!(PlayerActivityState::Sprinting, PlayerActivityState::Flying);
    }

    // ----------------------------------------------------------------
    // classify_activity
    // ----------------------------------------------------------------

    #[test]
    fn test_classify_idle() {
        assert_eq!(classify_activity(0.0, false), PlayerActivityState::Idle);
        assert_eq!(classify_activity(0.3, false), PlayerActivityState::Idle);
        assert_eq!(classify_activity(0.49, false), PlayerActivityState::Idle);
    }

    #[test]
    fn test_classify_walking() {
        assert_eq!(
            classify_activity(0.5, false),
            PlayerActivityState::Walking
        );
        assert_eq!(
            classify_activity(2.0, false),
            PlayerActivityState::Walking
        );
        assert_eq!(
            classify_activity(4.9, false),
            PlayerActivityState::Walking
        );
    }

    #[test]
    fn test_classify_sprinting() {
        assert_eq!(
            classify_activity(5.0, false),
            PlayerActivityState::Sprinting
        );
        assert_eq!(
            classify_activity(8.0, false),
            PlayerActivityState::Sprinting
        );
        assert_eq!(
            classify_activity(100.0, false),
            PlayerActivityState::Sprinting
        );
    }

    #[test]
    fn test_classify_flying_overrides_speed() {
        // Flying flag takes priority regardless of horizontal speed
        assert_eq!(classify_activity(0.0, true), PlayerActivityState::Flying);
        assert_eq!(classify_activity(3.0, true), PlayerActivityState::Flying);
        assert_eq!(classify_activity(10.0, true), PlayerActivityState::Flying);
    }

    // ----------------------------------------------------------------
    // BiomeAudioProfile
    // ----------------------------------------------------------------

    #[test]
    fn test_all_biomes_have_valid_profiles() {
        for biome in BiomeType::all() {
            let profile = BiomeAudioProfile::for_biome(*biome);

            // Volume scales must be in [0.0, 1.0]
            assert!(
                (0.0..=1.0).contains(&profile.idle_volume_scale),
                "{:?} idle_volume_scale out of range: {}",
                biome,
                profile.idle_volume_scale
            );
            assert!(
                (0.0..=1.0).contains(&profile.walking_volume_scale),
                "{:?} walking_volume_scale out of range: {}",
                biome,
                profile.walking_volume_scale
            );
            assert!(
                (0.0..=1.0).contains(&profile.sprinting_volume_scale),
                "{:?} sprinting_volume_scale out of range: {}",
                biome,
                profile.sprinting_volume_scale
            );
            assert!(
                (0.0..=1.0).contains(&profile.flying_volume_scale),
                "{:?} flying_volume_scale out of range: {}",
                biome,
                profile.flying_volume_scale
            );

            // Wind intensity must be in [0.0, 1.0]
            assert!(
                (0.0..=1.0).contains(&profile.base_wind_intensity),
                "{:?} base_wind_intensity out of range: {}",
                biome,
                profile.base_wind_intensity
            );
        }
    }

    #[test]
    fn test_idle_volume_is_highest_for_all_biomes() {
        for biome in BiomeType::all() {
            let profile = BiomeAudioProfile::for_biome(*biome);

            assert!(
                profile.idle_volume_scale >= profile.walking_volume_scale,
                "{:?}: idle ({}) should be >= walking ({})",
                biome,
                profile.idle_volume_scale,
                profile.walking_volume_scale
            );
            assert!(
                profile.walking_volume_scale >= profile.sprinting_volume_scale,
                "{:?}: walking ({}) should be >= sprinting ({})",
                biome,
                profile.walking_volume_scale,
                profile.sprinting_volume_scale
            );
            assert!(
                profile.idle_volume_scale >= profile.flying_volume_scale,
                "{:?}: idle ({}) should be >= flying ({})",
                biome,
                profile.idle_volume_scale,
                profile.flying_volume_scale
            );
        }
    }

    #[test]
    fn test_volume_scale_for_activity() {
        let profile = BiomeAudioProfile::for_biome(BiomeType::Plains);
        assert_eq!(
            profile.volume_scale_for(PlayerActivityState::Idle),
            profile.idle_volume_scale
        );
        assert_eq!(
            profile.volume_scale_for(PlayerActivityState::Walking),
            profile.walking_volume_scale
        );
        assert_eq!(
            profile.volume_scale_for(PlayerActivityState::Sprinting),
            profile.sprinting_volume_scale
        );
        assert_eq!(
            profile.volume_scale_for(PlayerActivityState::Flying),
            profile.flying_volume_scale
        );
    }

    #[test]
    fn test_forest_dampens_wind() {
        let forest = BiomeAudioProfile::for_biome(BiomeType::Forest);
        assert!(!forest.wind_responsive, "Forest canopy should dampen wind");
        assert!(
            forest.base_wind_intensity < 0.3,
            "Forest wind intensity should be low, was {}",
            forest.base_wind_intensity
        );
    }

    #[test]
    fn test_mountains_are_windy() {
        let mountains = BiomeAudioProfile::for_biome(BiomeType::Mountains);
        assert!(
            mountains.wind_responsive,
            "Mountains should be wind-responsive"
        );
        assert!(
            mountains.base_wind_intensity >= 0.5,
            "Mountains should have high wind intensity, was {}",
            mountains.base_wind_intensity
        );
    }

    #[test]
    fn test_tundra_windier_than_plains() {
        let tundra = BiomeAudioProfile::for_biome(BiomeType::Tundra);
        let plains = BiomeAudioProfile::for_biome(BiomeType::Plains);

        assert!(
            tundra.base_wind_intensity > plains.base_wind_intensity,
            "Tundra ({}) should be windier than plains ({})",
            tundra.base_wind_intensity,
            plains.base_wind_intensity
        );
    }

    #[test]
    fn test_volcanic_not_wind_responsive() {
        let volcanic = BiomeAudioProfile::for_biome(BiomeType::Volcanic);
        assert!(
            !volcanic.wind_responsive,
            "Volcanic rumbling should dominate over wind"
        );
    }

    // ----------------------------------------------------------------
    // Wind volume calculation
    // ----------------------------------------------------------------

    #[test]
    fn test_wind_volume_below_threshold_is_zero() {
        let profile = BiomeAudioProfile::for_biome(BiomeType::Plains);
        let vol =
            calculate_wind_volume(2.0, PlayerActivityState::Walking, Some(&profile), 0.5);
        assert_eq!(vol, 0.0, "Below speed threshold should produce no wind");
    }

    #[test]
    fn test_wind_volume_non_responsive_biome_non_flying() {
        let profile = BiomeAudioProfile::for_biome(BiomeType::Forest);
        let vol =
            calculate_wind_volume(8.0, PlayerActivityState::Sprinting, Some(&profile), 0.5);
        assert_eq!(
            vol, 0.0,
            "Non-responsive biome without flying should produce no wind"
        );
    }

    #[test]
    fn test_wind_volume_flying_overrides_biome_responsiveness() {
        let profile = BiomeAudioProfile::for_biome(BiomeType::Forest);
        let vol =
            calculate_wind_volume(8.0, PlayerActivityState::Flying, Some(&profile), 0.5);
        assert!(
            vol > 0.0,
            "Flying should produce wind even in non-responsive biome"
        );
    }

    #[test]
    fn test_wind_volume_scales_with_speed() {
        let profile = BiomeAudioProfile::for_biome(BiomeType::Mountains);
        let slow =
            calculate_wind_volume(5.0, PlayerActivityState::Sprinting, Some(&profile), 0.5);
        let fast = calculate_wind_volume(
            10.0,
            PlayerActivityState::Sprinting,
            Some(&profile),
            0.5,
        );

        assert!(fast > slow, "Faster speed should produce louder wind");
    }

    #[test]
    fn test_wind_volume_caps_at_max_speed() {
        let profile = BiomeAudioProfile::for_biome(BiomeType::Mountains);
        let at_max = calculate_wind_volume(
            WIND_FULL_SPEED,
            PlayerActivityState::Sprinting,
            Some(&profile),
            0.5,
        );
        let above_max = calculate_wind_volume(
            WIND_FULL_SPEED + 10.0,
            PlayerActivityState::Sprinting,
            Some(&profile),
            0.5,
        );

        assert!(
            (at_max - above_max).abs() < f32::EPSILON,
            "Wind volume should cap at full speed"
        );
    }

    #[test]
    fn test_wind_volume_no_biome_profile() {
        let vol =
            calculate_wind_volume(8.0, PlayerActivityState::Sprinting, None, 0.5);
        assert_eq!(
            vol, 0.0,
            "No biome profile should produce no wind (zero intensity)"
        );
    }

    #[test]
    fn test_wind_volume_zero_base_volume() {
        let profile = BiomeAudioProfile::for_biome(BiomeType::Mountains);
        let vol =
            calculate_wind_volume(8.0, PlayerActivityState::Sprinting, Some(&profile), 0.0);
        assert_eq!(vol, 0.0, "Zero base volume should produce no wind");
    }

    #[test]
    fn test_wind_volume_responsive_biome_sprinting() {
        let profile = BiomeAudioProfile::for_biome(BiomeType::Tundra);
        let vol =
            calculate_wind_volume(8.0, PlayerActivityState::Sprinting, Some(&profile), 0.5);
        assert!(
            vol > 0.0,
            "Wind-responsive biome with sprinting should produce wind"
        );
    }

    // ----------------------------------------------------------------
    // PlayerAudioState
    // ----------------------------------------------------------------

    #[test]
    fn test_player_audio_state_defaults() {
        let state = PlayerAudioState::default();
        assert_eq!(state.activity, PlayerActivityState::Idle);
        assert_eq!(state.horizontal_speed, 0.0);
        assert_eq!(state.previous_activity, PlayerActivityState::Idle);
        assert_eq!(state.current_volume_scale, 1.0);
        assert_eq!(state.target_volume_scale, 1.0);
    }

    // ----------------------------------------------------------------
    // ActivityChangedEvent
    // ----------------------------------------------------------------

    #[test]
    fn test_activity_changed_event_creation() {
        let event = ActivityChangedEvent {
            from: PlayerActivityState::Idle,
            to: PlayerActivityState::Walking,
            biome: Some(BiomeType::Plains),
        };
        assert_eq!(event.from, PlayerActivityState::Idle);
        assert_eq!(event.to, PlayerActivityState::Walking);
        assert_eq!(event.biome, Some(BiomeType::Plains));
    }

    #[test]
    fn test_activity_changed_event_no_biome() {
        let event = ActivityChangedEvent {
            from: PlayerActivityState::Flying,
            to: PlayerActivityState::Idle,
            biome: None,
        };
        assert!(event.biome.is_none());
    }

    // ----------------------------------------------------------------
    // WindSoundState
    // ----------------------------------------------------------------

    #[test]
    fn test_wind_sound_state_defaults() {
        let state = WindSoundState::default();
        assert!(state.entity.is_none());
        assert!(!state.should_be_active);
    }

    // ----------------------------------------------------------------
    // ActivitySoundAssets
    // ----------------------------------------------------------------

    #[test]
    fn test_activity_sound_assets_default_empty() {
        let assets = ActivitySoundAssets::default();
        assert!(assets.wind_rush.is_none());
    }

    // ----------------------------------------------------------------
    // Plugin registration
    // ----------------------------------------------------------------

    #[test]
    fn test_ambient_audio_plugin_builds() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_plugins(AmbientAudioPlugin);
        // Should not panic
    }
}
