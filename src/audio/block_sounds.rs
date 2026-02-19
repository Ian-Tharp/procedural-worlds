//! Block interaction sound system — footsteps, placement, and breaking sounds
//!
//! Provides a category-based sound system for block interactions:
//! - **Sound categories** — blocks are grouped by material (Stone, Dirt, Sand, etc.)
//! - **Footstep sounds** — emitted when the player walks/sprints on blocks
//! - **Procedural sound player** — logs sounds to console (placeholder for real audio)
//!
//! # Architecture
//!
//! ```text
//! Player (Velocity, Movement, Grounded)
//!     ↓
//! footstep_sound system (timer-based)
//!     ↓
//! BlockInteractionSoundEvent { action: Step }
//!     ↓
//! play_block_interaction_sounds
//!     ↓
//! BlockSoundPlayer::play() → info!() log (placeholder)
//! ```

use bevy::prelude::*;

use crate::actors::{Grounded, Movement, Player, Velocity};
use crate::world::BlockType;

// ============================================================================
// SOUND CATEGORIES
// ============================================================================

/// Material-based sound category for block types.
///
/// Each category maps to a distinct set of sounds (place, break, step).
/// Structured so that real audio assets can be loaded per-category later.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlockSoundCategory {
    Stone,
    Dirt,
    Sand,
    Wood,
    Metal,
    Glass,
    Gravel,
    Snow,
    Wet,
}

impl std::fmt::Display for BlockSoundCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stone => write!(f, "Stone"),
            Self::Dirt => write!(f, "Dirt"),
            Self::Sand => write!(f, "Sand"),
            Self::Wood => write!(f, "Wood"),
            Self::Metal => write!(f, "Metal"),
            Self::Glass => write!(f, "Glass"),
            Self::Gravel => write!(f, "Gravel"),
            Self::Snow => write!(f, "Snow"),
            Self::Wet => write!(f, "Wet"),
        }
    }
}

/// Map a block type to its sound category.
///
/// Every `BlockType` variant maps to a category. Unknown or new block
/// types fall back to `Stone`.
pub fn sound_category(block: BlockType) -> BlockSoundCategory {
    match block {
        BlockType::Stone
        | BlockType::Obsidian
        | BlockType::VolcanicRock
        | BlockType::Sandstone
        | BlockType::CopperOre
        | BlockType::IronOre
        | BlockType::SilverOre
        | BlockType::GoldOre => BlockSoundCategory::Stone,

        BlockType::Dirt
        | BlockType::Grass => BlockSoundCategory::Dirt,

        BlockType::Sand | BlockType::SandDunes => BlockSoundCategory::Sand,

        BlockType::Wood | BlockType::Cactus | BlockType::Leaves => BlockSoundCategory::Wood,

        BlockType::Snow | BlockType::Ice => BlockSoundCategory::Snow,

        BlockType::Water => BlockSoundCategory::Wet,

        // Air and any future unmatched variants default to Stone
        _ => BlockSoundCategory::Stone,
    }
}

// ============================================================================
// EVENTS
// ============================================================================

/// Action type for block interaction sounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlockAction {
    /// A block was placed.
    Place,
    /// A block was broken.
    Break,
    /// The player stepped on a block.
    Step,
}

impl std::fmt::Display for BlockAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Place => write!(f, "Place"),
            Self::Break => write!(f, "Break"),
            Self::Step => write!(f, "Step"),
        }
    }
}

/// Event emitted for block interaction sounds (footsteps, etc.).
///
/// This is separate from `playback::BlockSoundEvent` which handles
/// the existing place/break audio pipeline. This event drives the
/// category-based sound system, primarily for footsteps.
#[derive(Event, Clone, Debug)]
pub struct BlockInteractionSoundEvent {
    /// The type of block involved.
    pub block_type: BlockType,
    /// World position where the sound originates.
    pub position: Vec3,
    /// What kind of interaction produced this sound.
    pub action: BlockAction,
}

// ============================================================================
// RESOURCES
// ============================================================================

/// Timer that controls footstep sound emission intervals.
///
/// Fires a step sound event when the player is grounded and moving,
/// at intervals that vary based on walking vs sprinting.
#[derive(Resource, Debug)]
pub struct FootstepTimer {
    /// Time accumulated since last step sound (seconds).
    pub last_step: f32,
    /// Current interval between step sounds (seconds).
    pub interval: f32,
}

impl Default for FootstepTimer {
    fn default() -> Self {
        Self {
            last_step: 0.0,
            interval: WALK_STEP_INTERVAL,
        }
    }
}

/// Walking step interval in seconds.
const WALK_STEP_INTERVAL: f32 = 0.45;
/// Sprinting step interval in seconds.
const SPRINT_STEP_INTERVAL: f32 = 0.3;
/// Minimum horizontal speed for footstep sounds (blocks/sec).
const FOOTSTEP_SPEED_THRESHOLD: f32 = 0.5;
/// Sprint speed threshold — matches ambient.rs classification.
const SPRINT_SPEED_THRESHOLD: f32 = 5.0;

/// Procedural sound player — placeholder that logs sounds to console.
///
/// Designed so that real audio assets (`.ogg`/`.wav` files) can be
/// plugged in later by replacing the `play` method body with asset loading.
#[derive(Resource, Debug)]
pub struct BlockSoundPlayer {
    /// Whether sounds are enabled (toggle for debugging/preferences).
    pub sound_enabled: bool,
}

impl Default for BlockSoundPlayer {
    fn default() -> Self {
        Self {
            sound_enabled: true,
        }
    }
}

impl BlockSoundPlayer {
    /// Play a block interaction sound at the given position.
    ///
    /// Currently logs to console. Replace this body with real audio
    /// playback (load asset by category/action, spawn AudioPlayer entity)
    /// when sound files are available.
    pub fn play(&self, category: BlockSoundCategory, action: BlockAction, position: Vec3) {
        if !self.sound_enabled {
            return;
        }

        let (pitch, volume) = Self::variation(Self::seed_from_position(position));

        info!(
            "🔊 [{}] {} at ({:.0}, {:.0}, {:.0}) | pitch: {:.2}, vol: {:.2}",
            category, action, position.x, position.y, position.z, pitch, volume
        );
    }

    /// Generate slight pitch and volume variation from a seed.
    ///
    /// Returns `(pitch_multiplier, volume_multiplier)`:
    /// - Pitch: 0.8–1.2
    /// - Volume: 0.7–1.0
    pub fn variation(seed: u32) -> (f32, f32) {
        // Simple hash-based pseudo-random variation
        let hash = seed.wrapping_mul(2654435761);
        let pitch_rand = (hash % 1000) as f32 / 1000.0; // 0.0–1.0
        let vol_rand = (hash.wrapping_shr(10) % 1000) as f32 / 1000.0;

        let pitch = 0.8 + pitch_rand * 0.4; // 0.8–1.2
        let volume = 0.7 + vol_rand * 0.3; // 0.7–1.0

        (pitch, volume)
    }

    /// Derive a seed from a world position for variation.
    fn seed_from_position(position: Vec3) -> u32 {
        let x = (position.x * 100.0) as i32;
        let y = (position.y * 100.0) as i32;
        let z = (position.z * 100.0) as i32;
        (x.wrapping_mul(73856093) ^ y.wrapping_mul(19349663) ^ z.wrapping_mul(83492791)) as u32
    }
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that adds block interaction sounds (footsteps, category-based logging).
///
/// Works alongside [`super::AudioPlaybackPlugin`] which handles the existing
/// place/break sound file playback. This plugin adds:
/// - Footstep detection and sound emission
/// - Category-based procedural sound logging
pub struct BlockSoundPlugin;

impl Plugin for BlockSoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<BlockInteractionSoundEvent>()
            .init_resource::<FootstepTimer>()
            .init_resource::<BlockSoundPlayer>()
            .add_systems(
                Update,
                (
                    footstep_sound,
                    play_block_interaction_sounds,
                )
                    .chain(),
            );
    }
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// Emit footstep sound events when the player is grounded and moving.
///
/// Uses [`FootstepTimer`] to control emission rate. The interval changes
/// based on whether the player is walking or sprinting.
fn footstep_sound(
    time: Res<Time>,
    mut timer: ResMut<FootstepTimer>,
    mut events: EventWriter<BlockInteractionSoundEvent>,
    player_query: Query<(&Velocity, &Movement, &Grounded, &GlobalTransform), With<Player>>,
) {
    let Ok((velocity, movement, grounded, transform)) = player_query.get_single() else {
        return;
    };

    // Only emit footsteps when grounded and not flying
    if !grounded.is_grounded || movement.flying {
        timer.last_step = 0.0;
        return;
    }

    let horizontal_speed = Vec2::new(velocity.linear.x, velocity.linear.z).length();

    if horizontal_speed < FOOTSTEP_SPEED_THRESHOLD {
        timer.last_step = 0.0;
        return;
    }

    // Adjust interval based on speed
    timer.interval = if horizontal_speed >= SPRINT_SPEED_THRESHOLD {
        SPRINT_STEP_INTERVAL
    } else {
        WALK_STEP_INTERVAL
    };

    timer.last_step += time.delta_secs();

    if timer.last_step >= timer.interval {
        timer.last_step -= timer.interval;

        // Emit step event at player's feet position
        let pos = transform.translation();
        let foot_pos = Vec3::new(pos.x, pos.y - 0.1, pos.z);

        events.send(BlockInteractionSoundEvent {
            // Default to Stone for footsteps — in a full implementation,
            // this would raycast down to find the actual block type.
            block_type: BlockType::Stone,
            position: foot_pos,
            action: BlockAction::Step,
        });
    }
}

/// Consume block interaction sound events and play them via the sound player.
fn play_block_interaction_sounds(
    mut events: EventReader<BlockInteractionSoundEvent>,
    sound_player: Res<BlockSoundPlayer>,
) {
    for event in events.read() {
        let category = sound_category(event.block_type);
        sound_player.play(category, event.action, event.position);
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sound_category_mapping() {
        let all_blocks = [
            BlockType::Air,
            BlockType::Stone,
            BlockType::Dirt,
            BlockType::Grass,
            BlockType::Sand,
            BlockType::Water,
            BlockType::Wood,
            BlockType::Leaves,
            BlockType::Sandstone,
            BlockType::Snow,
            BlockType::Ice,
            BlockType::Obsidian,
            BlockType::VolcanicRock,
            BlockType::Cactus,
            BlockType::SandDunes,
            BlockType::CopperOre,
            BlockType::IronOre,
            BlockType::SilverOre,
            BlockType::GoldOre,
        ];

        for block in &all_blocks {
            let _category = sound_category(*block);
        }

        assert_eq!(sound_category(BlockType::Stone), BlockSoundCategory::Stone);
        assert_eq!(sound_category(BlockType::Dirt), BlockSoundCategory::Dirt);
        assert_eq!(sound_category(BlockType::Sand), BlockSoundCategory::Sand);
        assert_eq!(sound_category(BlockType::Wood), BlockSoundCategory::Wood);
        assert_eq!(sound_category(BlockType::Snow), BlockSoundCategory::Snow);
        assert_eq!(sound_category(BlockType::Water), BlockSoundCategory::Wet);
        assert_eq!(sound_category(BlockType::Grass), BlockSoundCategory::Dirt);
        assert_eq!(sound_category(BlockType::Obsidian), BlockSoundCategory::Stone);
        assert_eq!(sound_category(BlockType::Cactus), BlockSoundCategory::Wood);
        assert_eq!(sound_category(BlockType::Ice), BlockSoundCategory::Snow);
    }

    #[test]
    fn test_sound_category_coverage() {
        use std::collections::HashSet;

        let all_blocks = [
            BlockType::Stone,
            BlockType::Dirt,
            BlockType::Grass,
            BlockType::Sand,
            BlockType::Water,
            BlockType::Wood,
            BlockType::Leaves,
            BlockType::Sandstone,
            BlockType::Snow,
            BlockType::Ice,
            BlockType::Obsidian,
            BlockType::VolcanicRock,
            BlockType::Cactus,
            BlockType::SandDunes,
            BlockType::CopperOre,
            BlockType::IronOre,
            BlockType::SilverOre,
            BlockType::GoldOre,
        ];

        let categories: HashSet<BlockSoundCategory> = all_blocks
            .iter()
            .map(|b| sound_category(*b))
            .collect();

        assert!(
            categories.len() >= 3,
            "Expected at least 3 categories in use, got {}",
            categories.len()
        );
    }

    #[test]
    fn test_footstep_timer() {
        let timer = FootstepTimer::default();
        assert_eq!(timer.last_step, 0.0);
        assert_eq!(timer.interval, WALK_STEP_INTERVAL);
        assert_eq!(WALK_STEP_INTERVAL, 0.45);
        assert_eq!(SPRINT_STEP_INTERVAL, 0.3);

        let mut timer = FootstepTimer::default();
        timer.last_step = 0.44;
        assert!(timer.last_step < timer.interval);

        timer.last_step = 0.45;
        assert!(timer.last_step >= timer.interval);
    }

    #[test]
    fn test_pitch_volume_variation() {
        for seed in 0..100 {
            let (pitch, volume) = BlockSoundPlayer::variation(seed);
            assert!(
                (0.8..=1.2).contains(&pitch),
                "Pitch {pitch} out of range for seed {seed}"
            );
            assert!(
                (0.7..=1.0).contains(&volume),
                "Volume {volume} out of range for seed {seed}"
            );
        }
    }

    #[test]
    fn test_block_sound_player_disabled() {
        let player = BlockSoundPlayer {
            sound_enabled: false,
        };
        player.play(
            BlockSoundCategory::Stone,
            BlockAction::Place,
            Vec3::new(10.0, 20.0, 30.0),
        );
    }

    #[test]
    fn test_block_action_display() {
        assert_eq!(format!("{}", BlockAction::Place), "Place");
        assert_eq!(format!("{}", BlockAction::Break), "Break");
        assert_eq!(format!("{}", BlockAction::Step), "Step");
    }

    #[test]
    fn test_block_sound_category_display() {
        assert_eq!(format!("{}", BlockSoundCategory::Stone), "Stone");
        assert_eq!(format!("{}", BlockSoundCategory::Wet), "Wet");
    }

    #[test]
    fn test_block_sound_plugin_builds() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_plugins(BlockSoundPlugin);
    }
}
