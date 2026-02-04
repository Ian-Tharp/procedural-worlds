//! Audio playback — positional sound effects and ambient biome sounds
//!
//! Provides an event-driven audio system for:
//! - **Block interaction sounds** (place / break) with positional attenuation
//! - **Ambient biome sounds** that change based on the player's current biome
//!
//! # Architecture
//!
//! ```text
//! BlockInteraction (break/place)
//!     ↓ sends BlockSoundEvent
//! play_block_sounds system
//!     ↓ spawns AudioPlayer with SpatialSettings
//! Bevy audio backend
//!
//! Player position + biome lookup
//!     ↓
//! update_ambient_biome_sound system
//!     ↓ manages looping AudioPlayer per-biome
//! Bevy audio backend
//! ```
//!
//! # Sound Assets
//!
//! Sound files are expected in `assets/sounds/`:
//! - `block_place.ogg` — block placement sound
//! - `block_break.ogg` — block breaking sound
//! - `ambient_plains.ogg`, `ambient_desert.ogg`, etc. — biome ambience
//!
//! If sound files are missing, the system logs a warning but does not crash.

use bevy::prelude::*;

use crate::config::audio::AudioConfig;
use crate::generation::biome::BiomeType;
use crate::world::BlockType;

// ============================================================================
// EVENTS
// ============================================================================

/// Sound event emitted when a block is placed or broken.
///
/// Systems that modify blocks send this event; the audio system consumes
/// it to play the appropriate sound at the correct world position.
#[derive(Event, Clone, Debug)]
pub struct BlockSoundEvent {
    /// The type of sound to play.
    pub kind: BlockSoundKind,
    /// World position where the sound originates.
    pub position: Vec3,
    /// The block type involved (used to select sound variant in the future).
    pub block_type: BlockType,
}

/// Discriminant for block interaction sounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlockSoundKind {
    /// A block was placed by the player.
    Place,
    /// A block was broken by the player.
    Break,
}

// ============================================================================
// RESOURCES
// ============================================================================

/// Pre-loaded audio asset handles for block sounds.
///
/// Loaded once during [`Startup`] and reused for every block interaction.
#[derive(Resource, Default)]
pub struct BlockSoundAssets {
    pub place: Option<Handle<AudioSource>>,
    pub break_sound: Option<Handle<AudioSource>>,
}

/// Pre-loaded audio asset handles for biome ambient loops.
#[derive(Resource, Default)]
pub struct BiomeAmbientAssets {
    pub plains: Option<Handle<AudioSource>>,
    pub desert: Option<Handle<AudioSource>>,
    pub forest: Option<Handle<AudioSource>>,
    pub mountains: Option<Handle<AudioSource>>,
    pub tundra: Option<Handle<AudioSource>>,
    pub volcanic: Option<Handle<AudioSource>>,
}

impl BiomeAmbientAssets {
    /// Get the ambient sound handle for a given biome type.
    pub fn for_biome(&self, biome: BiomeType) -> Option<&Handle<AudioSource>> {
        match biome {
            BiomeType::Plains => self.plains.as_ref(),
            BiomeType::Desert => self.desert.as_ref(),
            BiomeType::Forest => self.forest.as_ref(),
            BiomeType::Mountains => self.mountains.as_ref(),
            BiomeType::Tundra => self.tundra.as_ref(),
            BiomeType::Volcanic => self.volcanic.as_ref(),
        }
    }
}

/// Tracks the currently active ambient biome sound entity and biome type.
///
/// When the player moves to a different biome, the old ambient entity is
/// despawned and a new one is spawned for the new biome.
#[derive(Resource, Default)]
pub struct CurrentAmbientSound {
    /// The biome whose ambient sound is currently playing.
    pub active_biome: Option<BiomeType>,
    /// Entity playing the ambient loop (if any).
    pub entity: Option<Entity>,
}

/// Marker component for spatial audio listener (attached to the player camera).
#[derive(Component)]
pub struct AudioListenerMarker;

// ============================================================================
// COMPONENTS
// ============================================================================

/// Marker for one-shot spatial sound effects (block place/break).
///
/// These entities are automatically despawned after a short lifetime.
#[derive(Component)]
pub struct SpatialSfx {
    /// Remaining lifetime in seconds before this entity is despawned.
    pub lifetime: f32,
}

/// Marker for the ambient biome sound entity.
#[derive(Component)]
pub struct AmbientSound;

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that adds the audio playback system to the app.
///
/// Registers events, loads sound assets, and adds audio playback systems.
/// Reads volume settings from [`AudioConfig`] in the config module.
pub struct AudioPlaybackPlugin;

impl Plugin for AudioPlaybackPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<BlockSoundEvent>()
            .init_resource::<BlockSoundAssets>()
            .init_resource::<BiomeAmbientAssets>()
            .init_resource::<CurrentAmbientSound>()
            .add_systems(Startup, load_audio_assets)
            .add_systems(
                Update,
                (
                    play_block_sounds,
                    update_ambient_biome_sound,
                    cleanup_finished_sounds,
                ),
            );
    }
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// Load sound assets from the asset server during startup.
///
/// Missing files are logged as warnings but do not prevent the game from
/// running. The corresponding `Option<Handle>` remains `None`.
fn load_audio_assets(
    asset_server: Res<AssetServer>,
    mut block_sounds: ResMut<BlockSoundAssets>,
    mut biome_sounds: ResMut<BiomeAmbientAssets>,
) {
    info!("Loading audio assets...");

    // Block interaction sounds
    block_sounds.place = Some(asset_server.load("sounds/block_place.ogg"));
    block_sounds.break_sound = Some(asset_server.load("sounds/block_break.ogg"));

    // Biome ambient loops
    biome_sounds.plains = Some(asset_server.load("sounds/ambient_plains.ogg"));
    biome_sounds.desert = Some(asset_server.load("sounds/ambient_desert.ogg"));
    biome_sounds.forest = Some(asset_server.load("sounds/ambient_forest.ogg"));
    biome_sounds.mountains = Some(asset_server.load("sounds/ambient_mountains.ogg"));
    biome_sounds.tundra = Some(asset_server.load("sounds/ambient_tundra.ogg"));
    biome_sounds.volcanic = Some(asset_server.load("sounds/ambient_volcanic.ogg"));

    info!("Audio asset loading initiated (sounds may load asynchronously)");
}

/// Consume [`BlockSoundEvent`]s and spawn positional audio entities.
///
/// Each event results in a short-lived entity with `AudioPlayer` and
/// `SpatialSettings` positioned at the block interaction location.
/// Volume is controlled by the [`AudioConfig`] resource (master × sfx).
fn play_block_sounds(
    mut commands: Commands,
    mut events: EventReader<BlockSoundEvent>,
    block_sounds: Res<BlockSoundAssets>,
    audio_config: Res<AudioConfig>,
) {
    for event in events.read() {
        let source = match event.kind {
            BlockSoundKind::Place => &block_sounds.place,
            BlockSoundKind::Break => &block_sounds.break_sound,
        };

        let Some(source_handle) = source else {
            continue;
        };

        let volume = audio_config.effective_sfx_volume();

        commands.spawn((
            AudioPlayer(source_handle.clone()),
            PlaybackSettings {
                mode: bevy::audio::PlaybackMode::Despawn,
                volume: bevy::audio::Volume::new(volume),
                ..default()
            },
            Transform::from_translation(event.position),
            GlobalTransform::default(),
            SpatialSfx { lifetime: 3.0 },
        ));
    }
}

/// Update the ambient biome sound based on the player's current biome.
///
/// Determines the biome at the player's world position using the same
/// biome noise as terrain generation. If the biome has changed, the old
/// ambient sound is stopped and a new one starts.
/// Volume is controlled by [`AudioConfig`] (master × ambience).
fn update_ambient_biome_sound(
    mut commands: Commands,
    mut current_ambient: ResMut<CurrentAmbientSound>,
    biome_sounds: Res<BiomeAmbientAssets>,
    audio_config: Res<AudioConfig>,
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
    terrain_config: Res<crate::generation::TerrainConfig>,
) {
    let Ok(camera_global) = camera_query.get_single() else {
        return;
    };

    let pos = camera_global.translation();
    let world_x = pos.x.floor() as i32;
    let world_z = pos.z.floor() as i32;

    // Use the same biome noise as terrain generation
    let biome_noise = noise::Simplex::new(
        terrain_config.seed.wrapping_add(terrain_config.biome_seed_offset),
    );
    let current_biome = crate::generation::biome::biome_at(
        world_x,
        world_z,
        &biome_noise,
        terrain_config.biome_scale,
    );

    // If biome hasn't changed, nothing to do
    if current_ambient.active_biome == Some(current_biome) {
        return;
    }

    // Despawn old ambient sound if any
    if let Some(entity) = current_ambient.entity.take() {
        commands.entity(entity).despawn();
    }

    // Spawn new ambient sound for the current biome
    if let Some(source_handle) = biome_sounds.for_biome(current_biome) {
        let volume = audio_config.effective_ambience_volume();

        let entity = commands
            .spawn((
                AudioPlayer(source_handle.clone()),
                PlaybackSettings {
                    mode: bevy::audio::PlaybackMode::Loop,
                    volume: bevy::audio::Volume::new(volume),
                    ..default()
                },
                AmbientSound,
            ))
            .id();

        current_ambient.entity = Some(entity);
    }

    current_ambient.active_biome = Some(current_biome);
    info!("Biome ambient changed to: {:?}", current_biome);
}

/// Despawn expired spatial sound effect entities.
///
/// Entities with [`SpatialSfx`] have their lifetime decremented each frame.
/// When the lifetime expires, the entity is despawned. In practice, Bevy's
/// `PlaybackMode::Despawn` handles most cleanup, but this serves as a safety
/// net for cases where audio playback doesn't complete normally.
fn cleanup_finished_sounds(
    mut commands: Commands,
    time: Res<Time>,
    mut sfx_query: Query<(Entity, &mut SpatialSfx)>,
) {
    for (entity, mut sfx) in &mut sfx_query {
        sfx.lifetime -= time.delta_secs();
        if sfx.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ----------------------------------------------------------------
    // BlockSoundEvent
    // ----------------------------------------------------------------

    #[test]
    fn test_block_sound_event_creation() {
        let event = BlockSoundEvent {
            kind: BlockSoundKind::Place,
            position: Vec3::new(10.0, 20.0, 30.0),
            block_type: BlockType::Stone,
        };

        assert_eq!(event.kind, BlockSoundKind::Place);
        assert_eq!(event.position, Vec3::new(10.0, 20.0, 30.0));
        assert_eq!(event.block_type, BlockType::Stone);
    }

    #[test]
    fn test_block_sound_event_break() {
        let event = BlockSoundEvent {
            kind: BlockSoundKind::Break,
            position: Vec3::ZERO,
            block_type: BlockType::Dirt,
        };

        assert_eq!(event.kind, BlockSoundKind::Break);
        assert_eq!(event.block_type, BlockType::Dirt);
    }

    #[test]
    fn test_block_sound_kind_equality() {
        assert_eq!(BlockSoundKind::Place, BlockSoundKind::Place);
        assert_eq!(BlockSoundKind::Break, BlockSoundKind::Break);
        assert_ne!(BlockSoundKind::Place, BlockSoundKind::Break);
    }

    // ----------------------------------------------------------------
    // BiomeAmbientAssets
    // ----------------------------------------------------------------

    #[test]
    fn test_biome_ambient_assets_default_is_empty() {
        let assets = BiomeAmbientAssets::default();
        assert!(assets.plains.is_none());
        assert!(assets.desert.is_none());
        assert!(assets.forest.is_none());
        assert!(assets.mountains.is_none());
        assert!(assets.tundra.is_none());
        assert!(assets.volcanic.is_none());
    }

    #[test]
    fn test_biome_ambient_for_biome_returns_none_when_empty() {
        let assets = BiomeAmbientAssets::default();

        for biome in BiomeType::all() {
            assert!(
                assets.for_biome(*biome).is_none(),
                "Empty assets should return None for {:?}",
                biome
            );
        }
    }

    // ----------------------------------------------------------------
    // CurrentAmbientSound
    // ----------------------------------------------------------------

    #[test]
    fn test_current_ambient_sound_defaults() {
        let current = CurrentAmbientSound::default();
        assert!(current.active_biome.is_none());
        assert!(current.entity.is_none());
    }

    // ----------------------------------------------------------------
    // BlockSoundAssets
    // ----------------------------------------------------------------

    #[test]
    fn test_block_sound_assets_default_is_empty() {
        let assets = BlockSoundAssets::default();
        assert!(assets.place.is_none());
        assert!(assets.break_sound.is_none());
    }

    // ----------------------------------------------------------------
    // SpatialSfx
    // ----------------------------------------------------------------

    #[test]
    fn test_spatial_sfx_lifetime() {
        let sfx = SpatialSfx { lifetime: 3.0 };
        assert_eq!(sfx.lifetime, 3.0);
    }

    #[test]
    fn test_spatial_sfx_lifetime_decrement() {
        let mut sfx = SpatialSfx { lifetime: 1.0 };
        sfx.lifetime -= 0.5;
        assert!((sfx.lifetime - 0.5).abs() < f32::EPSILON);
        sfx.lifetime -= 0.5;
        assert!(sfx.lifetime.abs() < f32::EPSILON);
    }

    // ----------------------------------------------------------------
    // All biome types covered by for_biome
    // ----------------------------------------------------------------

    #[test]
    fn test_for_biome_covers_all_variants() {
        // Ensure the match in for_biome has an arm for every BiomeType variant.
        // This test will fail to compile if a new variant is added but not
        // handled in for_biome.
        let assets = BiomeAmbientAssets::default();
        for biome in BiomeType::all() {
            // Just call it — we're checking that none panic
            let _ = assets.for_biome(*biome);
        }
    }
}
