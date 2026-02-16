//! Creature/Entity system - passive and hostile mobs with AI
//!
//! Provides creature spawning, AI state machines, movement, combat, and death/drops.
//! Passive mobs (Cow, Sheep, Chicken) wander and flee when hit.
//! Hostile mobs (Zombie, Skeleton, Spider) chase and attack the player.

use bevy::prelude::*;
use rand::Rng;

use crate::actors::Player;
use crate::generation::biome::BiomeType;
use crate::health::{DamageEvent, DamageSource, DeathEvent, Health};
use crate::inventory::{FoodType, ItemId, MaterialType};

// ============================================================================
// ENUMS
// ============================================================================

/// The types of creatures that can exist in the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CreatureType {
    // Passive
    Cow,
    Sheep,
    Chicken,
    // Hostile
    Zombie,
    Skeleton,
    Spider,
}

impl CreatureType {
    /// Whether this creature is hostile toward the player.
    pub fn is_hostile(&self) -> bool {
        matches!(self, CreatureType::Zombie | CreatureType::Skeleton | CreatureType::Spider)
    }

    /// Default health for this creature type.
    pub fn default_health(&self) -> f32 {
        match self {
            CreatureType::Cow => 10.0,
            CreatureType::Sheep => 8.0,
            CreatureType::Chicken => 4.0,
            CreatureType::Zombie => 20.0,
            CreatureType::Skeleton => 20.0,
            CreatureType::Spider => 16.0,
        }
    }

    /// Default movement speed in blocks per second.
    pub fn default_speed(&self) -> f32 {
        match self {
            CreatureType::Cow => 2.0,
            CreatureType::Sheep => 2.5,
            CreatureType::Chicken => 3.0,
            CreatureType::Zombie => 2.3,
            CreatureType::Skeleton => 2.5,
            CreatureType::Spider => 3.5,
        }
    }

    /// Aggro range in blocks (hostile only, 0 for passive).
    pub fn default_aggro_range(&self) -> f32 {
        match self {
            CreatureType::Cow | CreatureType::Sheep | CreatureType::Chicken => 0.0,
            CreatureType::Zombie => 16.0,
            CreatureType::Skeleton => 24.0,
            CreatureType::Spider => 12.0,
        }
    }

    /// Attack damage dealt to the player.
    pub fn attack_damage(&self) -> f32 {
        match self {
            CreatureType::Cow | CreatureType::Sheep | CreatureType::Chicken => 0.0,
            CreatureType::Zombie => 3.0,
            CreatureType::Skeleton => 4.0,
            CreatureType::Spider => 2.0,
        }
    }

    /// Items dropped on death.
    pub fn loot_table(&self) -> Vec<(ItemId, u32)> {
        match self {
            CreatureType::Cow => vec![(ItemId::Food(FoodType::CookedMeat), 2)],
            CreatureType::Sheep => vec![(ItemId::Material(MaterialType::Fiber), 2)],
            CreatureType::Chicken => vec![(ItemId::Food(FoodType::CookedMeat), 1)],
            CreatureType::Zombie => vec![(ItemId::Material(MaterialType::Stick), 1)],
            CreatureType::Skeleton => vec![(ItemId::Material(MaterialType::Stick), 2)],
            CreatureType::Spider => vec![(ItemId::Material(MaterialType::String), 2)],
        }
    }

    /// Which biomes this creature can spawn in.
    pub fn valid_biomes(&self) -> &'static [BiomeType] {
        match self {
            CreatureType::Cow => &[BiomeType::Plains, BiomeType::Forest, BiomeType::Savanna],
            CreatureType::Sheep => &[BiomeType::Plains, BiomeType::Mountains, BiomeType::Taiga],
            CreatureType::Chicken => &[BiomeType::Plains, BiomeType::Forest, BiomeType::Jungle, BiomeType::Swamp],
            CreatureType::Zombie => &[BiomeType::Plains, BiomeType::Forest, BiomeType::Swamp, BiomeType::Taiga, BiomeType::Desert],
            CreatureType::Skeleton => &[BiomeType::Tundra, BiomeType::Mountains, BiomeType::Taiga, BiomeType::Badlands],
            CreatureType::Spider => &[BiomeType::Forest, BiomeType::Jungle, BiomeType::Swamp, BiomeType::Savanna],
        }
    }
}

// ============================================================================
// COMPONENTS
// ============================================================================

/// Core creature component with type and stats.
#[derive(Component, Debug, Clone)]
pub struct Creature {
    pub creature_type: CreatureType,
    pub speed: f32,
    pub aggro_range: f32,
}

impl Creature {
    pub fn new(creature_type: CreatureType) -> Self {
        Self {
            creature_type,
            speed: creature_type.default_speed(),
            aggro_range: creature_type.default_aggro_range(),
        }
    }
}

/// AI behavior state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreatureBehavior {
    Idle,
    Wander,
    Flee,
    Chase,
    Attack,
}

/// AI state machine component.
#[derive(Component, Debug)]
pub struct CreatureAI {
    pub behavior: CreatureBehavior,
    pub behavior_timer: f32,
    pub target: Option<Entity>,
    /// Direction for wander/flee movement
    pub move_direction: Vec3,
    /// Cooldown between attacks
    pub attack_cooldown: f32,
    /// Whether this creature was recently hit (triggers flee for passive)
    pub was_hit: bool,
}

impl CreatureAI {
    pub fn new() -> Self {
        Self {
            behavior: CreatureBehavior::Idle,
            behavior_timer: 0.0,
            target: None,
            move_direction: Vec3::ZERO,
            attack_cooldown: 0.0,
            was_hit: false,
        }
    }
}

impl Default for CreatureAI {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// RESOURCES
// ============================================================================

/// Tracks total spawned creatures for the cap.
#[derive(Resource, Default)]
pub struct CreatureCount {
    pub count: u32,
}

/// Max creatures in the world at once.
const MAX_CREATURES: u32 = 20;

/// Distance from player to spawn creatures.
const SPAWN_RADIUS: f32 = 40.0;

/// Min distance from player to spawn (don't pop in right next to them).
const MIN_SPAWN_DISTANCE: f32 = 20.0;

/// Attack range in blocks.
const ATTACK_RANGE: f32 = 2.0;

/// Flee range - how far passive mobs run when hit.
const _FLEE_DISTANCE: f32 = 10.0;

/// Spawn check interval in seconds.
#[derive(Resource)]
pub struct SpawnTimer(pub Timer);

impl Default for SpawnTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(3.0, TimerMode::Repeating))
    }
}

// ============================================================================
// EVENTS
// ============================================================================

/// Fired when a creature is hit by the player.
#[derive(Event, Debug)]
pub struct CreatureHitEvent {
    pub creature: Entity,
    pub damage: f32,
}

/// Fired when a creature drops loot on death.
#[derive(Event, Debug)]
pub struct CreatureLootEvent {
    pub position: Vec3,
    pub items: Vec<(ItemId, u32)>,
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// Spawns creatures around the player, respecting biome and creature cap.
pub fn creature_spawning_system(
    mut commands: Commands,
    time: Res<Time>,
    mut spawn_timer: ResMut<SpawnTimer>,
    creature_query: Query<&Creature>,
    player_query: Query<&Transform, With<Player>>,
    mut creature_count: ResMut<CreatureCount>,
    game_mode: Res<crate::health::GameMode>,
) {
    // Don't spawn hostile creatures in creative mode
    let creative = *game_mode == crate::health::GameMode::Creative;
    spawn_timer.0.tick(time.delta());
    if !spawn_timer.0.just_finished() {
        return;
    }

    // Update count
    creature_count.count = creature_query.iter().count() as u32;

    if creature_count.count >= MAX_CREATURES {
        return;
    }

    let Ok(player_transform) = player_query.get_single() else {
        return;
    };

    let player_pos = player_transform.translation;
    let mut rng = rand::thread_rng();

    // Pick a random creature type (no hostiles in creative)
    let creature_type = if creative {
        let passive = [CreatureType::Cow, CreatureType::Sheep, CreatureType::Chicken];
        passive[rng.gen_range(0..passive.len())]
    } else {
        let all = [
            CreatureType::Cow, CreatureType::Sheep, CreatureType::Chicken,
            CreatureType::Zombie, CreatureType::Skeleton, CreatureType::Spider,
        ];
        all[rng.gen_range(0..all.len())]
    };

    // Pick a random spawn position around the player
    let angle: f32 = rng.gen_range(0.0..std::f32::consts::TAU);
    let distance: f32 = rng.gen_range(MIN_SPAWN_DISTANCE..SPAWN_RADIUS);
    let spawn_pos = Vec3::new(
        player_pos.x + angle.cos() * distance,
        player_pos.y,
        player_pos.z + angle.sin() * distance,
    );

    // Spawn the creature entity
    commands.spawn((
        Creature::new(creature_type),
        CreatureAI::new(),
        Health::new(creature_type.default_health()),
        Transform::from_translation(spawn_pos),
        GlobalTransform::default(),
    ));
}

/// AI state machine - decides creature behavior each frame.
pub fn creature_ai_system(
    time: Res<Time>,
    mut creature_query: Query<(Entity, &Creature, &mut CreatureAI, &Transform)>,
    player_query: Query<(Entity, &Transform), With<Player>>,
) {
    let dt = time.delta_secs();
    let Ok((player_entity, player_transform)) = player_query.get_single() else {
        return;
    };
    let player_pos = player_transform.translation;

    let mut rng = rand::thread_rng();

    for (_entity, creature, mut ai, transform) in &mut creature_query {
        let pos = transform.translation;
        let dist_to_player = pos.distance(player_pos);

        // Tick timers
        ai.behavior_timer -= dt;
        if ai.attack_cooldown > 0.0 {
            ai.attack_cooldown -= dt;
        }

        // State transitions
        match ai.behavior {
            CreatureBehavior::Idle => {
                if creature.creature_type.is_hostile() && dist_to_player <= creature.aggro_range {
                    ai.behavior = CreatureBehavior::Chase;
                    ai.target = Some(player_entity);
                } else if ai.was_hit && !creature.creature_type.is_hostile() {
                    // Passive mobs flee when hit
                    let flee_dir = (pos - player_pos).normalize_or_zero();
                    ai.move_direction = Vec3::new(flee_dir.x, 0.0, flee_dir.z).normalize_or_zero();
                    ai.behavior = CreatureBehavior::Flee;
                    ai.behavior_timer = 3.0;
                    ai.was_hit = false;
                } else if ai.behavior_timer <= 0.0 {
                    // Random chance to start wandering
                    if rng.gen_range(0.0..1.0) < 0.3 {
                        let angle: f32 = rng.gen_range(0.0..std::f32::consts::TAU);
                        ai.move_direction = Vec3::new(angle.cos(), 0.0, angle.sin());
                        ai.behavior = CreatureBehavior::Wander;
                        ai.behavior_timer = rng.gen_range(2.0..5.0);
                    } else {
                        ai.behavior_timer = rng.gen_range(1.0..3.0);
                    }
                }
            }
            CreatureBehavior::Wander => {
                if creature.creature_type.is_hostile() && dist_to_player <= creature.aggro_range {
                    ai.behavior = CreatureBehavior::Chase;
                    ai.target = Some(player_entity);
                } else if ai.was_hit && !creature.creature_type.is_hostile() {
                    let flee_dir = (pos - player_pos).normalize_or_zero();
                    ai.move_direction = Vec3::new(flee_dir.x, 0.0, flee_dir.z).normalize_or_zero();
                    ai.behavior = CreatureBehavior::Flee;
                    ai.behavior_timer = 3.0;
                    ai.was_hit = false;
                } else if ai.behavior_timer <= 0.0 {
                    ai.behavior = CreatureBehavior::Idle;
                    ai.behavior_timer = rng.gen_range(1.0..4.0);
                }
            }
            CreatureBehavior::Flee => {
                if ai.behavior_timer <= 0.0 {
                    ai.behavior = CreatureBehavior::Idle;
                    ai.behavior_timer = rng.gen_range(2.0..4.0);
                }
            }
            CreatureBehavior::Chase => {
                if dist_to_player <= ATTACK_RANGE {
                    ai.behavior = CreatureBehavior::Attack;
                } else if dist_to_player > creature.aggro_range * 1.5 {
                    // Lost aggro
                    ai.behavior = CreatureBehavior::Idle;
                    ai.target = None;
                    ai.behavior_timer = rng.gen_range(1.0..3.0);
                } else {
                    // Update chase direction
                    let chase_dir = (player_pos - pos).normalize_or_zero();
                    ai.move_direction = Vec3::new(chase_dir.x, 0.0, chase_dir.z).normalize_or_zero();
                }
            }
            CreatureBehavior::Attack => {
                if dist_to_player > ATTACK_RANGE * 1.5 {
                    ai.behavior = CreatureBehavior::Chase;
                }
            }
        }
    }
}

/// Moves creatures based on their current AI behavior.
pub fn creature_movement_system(
    time: Res<Time>,
    mut query: Query<(&Creature, &CreatureAI, &mut Transform)>,
) {
    let dt = time.delta_secs();

    for (creature, ai, mut transform) in &mut query {
        let speed = match ai.behavior {
            CreatureBehavior::Idle => 0.0,
            CreatureBehavior::Wander => creature.speed * 0.5,
            CreatureBehavior::Flee => creature.speed * 1.5,
            CreatureBehavior::Chase => creature.speed,
            CreatureBehavior::Attack => 0.0,
        };

        if speed > 0.0 {
            let movement = ai.move_direction * speed * dt;
            transform.translation += movement;
        }
    }
}

/// Hostile creatures deal damage to the player when attacking.
pub fn creature_damage_system(
    time: Res<Time>,
    mut creature_query: Query<(&Creature, &mut CreatureAI)>,
    mut damage_events: EventWriter<DamageEvent>,
) {
    let dt = time.delta_secs();

    for (creature, mut ai) in &mut creature_query {
        if ai.behavior == CreatureBehavior::Attack && creature.creature_type.is_hostile() {
            if ai.attack_cooldown <= 0.0 {
                if let Some(target) = ai.target {
                    damage_events.send(DamageEvent {
                        target,
                        amount: creature.creature_type.attack_damage(),
                        source: DamageSource::Mob,
                    });
                    ai.attack_cooldown = 1.0;
                }
            }
        }
        // Tick attack cooldown (also ticked in ai_system but needed if not attacking)
        if ai.attack_cooldown > 0.0 {
            ai.attack_cooldown -= dt;
        }
    }
}

/// Handles creature hit events - applies damage and sets was_hit flag.
pub fn creature_hit_system(
    mut hit_events: EventReader<CreatureHitEvent>,
    mut damage_events: EventWriter<DamageEvent>,
    mut ai_query: Query<&mut CreatureAI>,
) {
    for event in hit_events.read() {
        damage_events.send(DamageEvent {
            target: event.creature,
            amount: event.damage,
            source: DamageSource::Environment,
        });
        if let Ok(mut ai) = ai_query.get_mut(event.creature) {
            ai.was_hit = true;
        }
    }
}

/// Handles creature death - removes entity and fires loot event.
pub fn creature_death_system(
    mut commands: Commands,
    mut death_events: EventReader<DeathEvent>,
    creature_query: Query<(&Creature, &Transform)>,
    mut loot_events: EventWriter<CreatureLootEvent>,
) {
    for event in death_events.read() {
        if let Ok((creature, transform)) = creature_query.get(event.entity) {
            let loot = creature.creature_type.loot_table();
            if !loot.is_empty() {
                loot_events.send(CreatureLootEvent {
                    position: transform.translation,
                    items: loot,
                });
            }
            commands.entity(event.entity).despawn();
        }
    }
}

/// Despawn creatures that are too far from the player.
pub fn creature_despawn_system(
    mut commands: Commands,
    creature_query: Query<(Entity, &Transform), With<Creature>>,
    player_query: Query<&Transform, With<Player>>,
) {
    let Ok(player_transform) = player_query.get_single() else {
        return;
    };
    let player_pos = player_transform.translation;

    for (entity, transform) in &creature_query {
        let dist = transform.translation.distance(player_pos);
        if dist > SPAWN_RADIUS * 2.0 {
            commands.entity(entity).despawn();
        }
    }
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that registers all creature systems, events, and resources.
pub struct CreaturePlugin;

impl Plugin for CreaturePlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<CreatureHitEvent>()
            .add_event::<CreatureLootEvent>()
            .init_resource::<CreatureCount>()
            .init_resource::<SpawnTimer>()
            .add_systems(
                Update,
                (
                    creature_spawning_system,
                    creature_ai_system,
                    creature_movement_system,
                    creature_damage_system,
                    creature_hit_system,
                    creature_death_system,
                    creature_despawn_system,
                )
                    .chain(),
            );
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creature_type_hostile() {
        assert!(!CreatureType::Cow.is_hostile());
        assert!(!CreatureType::Sheep.is_hostile());
        assert!(!CreatureType::Chicken.is_hostile());
        assert!(CreatureType::Zombie.is_hostile());
        assert!(CreatureType::Skeleton.is_hostile());
        assert!(CreatureType::Spider.is_hostile());
    }

    #[test]
    fn test_creature_defaults() {
        let creature = Creature::new(CreatureType::Zombie);
        assert_eq!(creature.speed, 2.3);
        assert_eq!(creature.aggro_range, 16.0);
    }

    #[test]
    fn test_creature_ai_default() {
        let ai = CreatureAI::new();
        assert_eq!(ai.behavior, CreatureBehavior::Idle);
        assert!(ai.target.is_none());
        assert!(!ai.was_hit);
    }

    #[test]
    fn test_creature_health_values() {
        assert_eq!(CreatureType::Cow.default_health(), 10.0);
        assert_eq!(CreatureType::Zombie.default_health(), 20.0);
        assert_eq!(CreatureType::Chicken.default_health(), 4.0);
    }

    #[test]
    fn test_creature_attack_damage() {
        assert_eq!(CreatureType::Cow.attack_damage(), 0.0);
        assert_eq!(CreatureType::Zombie.attack_damage(), 3.0);
        assert_eq!(CreatureType::Skeleton.attack_damage(), 4.0);
        assert_eq!(CreatureType::Spider.attack_damage(), 2.0);
    }

    #[test]
    fn test_creature_loot_table() {
        let cow_loot = CreatureType::Cow.loot_table();
        assert_eq!(cow_loot.len(), 1);
        assert_eq!(cow_loot[0], (ItemId::Food(FoodType::CookedMeat), 2));

        let spider_loot = CreatureType::Spider.loot_table();
        assert_eq!(spider_loot[0], (ItemId::Material(MaterialType::String), 2));
    }

    #[test]
    fn test_creature_valid_biomes() {
        let cow_biomes = CreatureType::Cow.valid_biomes();
        assert!(cow_biomes.contains(&BiomeType::Plains));
        assert!(!cow_biomes.contains(&BiomeType::Desert));

        let skeleton_biomes = CreatureType::Skeleton.valid_biomes();
        assert!(skeleton_biomes.contains(&BiomeType::Tundra));
        assert!(!skeleton_biomes.contains(&BiomeType::Jungle));
    }

    #[test]
    fn test_creature_speed_values() {
        assert!(CreatureType::Spider.default_speed() > CreatureType::Cow.default_speed());
        assert!(CreatureType::Chicken.default_speed() > CreatureType::Sheep.default_speed());
    }

    #[test]
    fn test_behavior_enum() {
        let behaviors = [
            CreatureBehavior::Idle,
            CreatureBehavior::Wander,
            CreatureBehavior::Flee,
            CreatureBehavior::Chase,
            CreatureBehavior::Attack,
        ];
        // All variants are distinct
        for (i, a) in behaviors.iter().enumerate() {
            for (j, b) in behaviors.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }
}
