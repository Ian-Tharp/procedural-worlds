//! Block drops system - spawns item entities when blocks are broken
//!
//! When a player breaks a block, a `BlockDropEvent` is fired. The drop system
//! spawns a `DroppedItem` entity at the block position with a small random offset.
//! Nearby players can pick up dropped items, which are added to their inventory.
//! Items despawn after 300 seconds.

use bevy::prelude::*;
use crate::actors::Player;
use crate::inventory::{Inventory, ItemId};
use crate::world::BlockType;

// ============================================================================
// COMPONENTS & EVENTS
// ============================================================================

/// A dropped item entity in the world
#[derive(Component, Debug, Clone)]
pub struct DroppedItem {
    /// What item this represents
    pub item: ItemId,
    /// Remaining lifetime in seconds before despawn
    pub lifetime: f32,
}

impl DroppedItem {
    /// Default lifetime for dropped items (300 seconds = 5 minutes)
    pub const DEFAULT_LIFETIME: f32 = 300.0;

    pub fn new(item: ItemId) -> Self {
        Self {
            item,
            lifetime: Self::DEFAULT_LIFETIME,
        }
    }
}

/// Event fired when a block is broken and should drop an item
#[derive(Event, Debug, Clone)]
pub struct BlockDropEvent {
    /// The type of block that was broken
    pub block_type: BlockType,
    /// World position where the block was
    pub position: Vec3,
}

// ============================================================================
// DROP TABLE
// ============================================================================

/// Maps block types to the item they drop (if any)
pub fn block_drop_table(block_type: BlockType) -> Option<ItemId> {
    match block_type {
        BlockType::Air => None,
        BlockType::Water => None,
        BlockType::Leaves => None,
        BlockType::Stone => Some(ItemId::Block(BlockType::Stone)),
        BlockType::Dirt => Some(ItemId::Block(BlockType::Dirt)),
        BlockType::Grass => Some(ItemId::Block(BlockType::Grass)),
        BlockType::Sand => Some(ItemId::Block(BlockType::Sand)),
        BlockType::Wood => Some(ItemId::Block(BlockType::Wood)),
        BlockType::Sandstone => Some(ItemId::Block(BlockType::Sandstone)),
        BlockType::Snow => Some(ItemId::Block(BlockType::Snow)),
        BlockType::Ice => Some(ItemId::Block(BlockType::Ice)),
        BlockType::Obsidian => Some(ItemId::Block(BlockType::Obsidian)),
        BlockType::VolcanicRock => Some(ItemId::Block(BlockType::VolcanicRock)),
        BlockType::Cactus => Some(ItemId::Block(BlockType::Cactus)),
        BlockType::CopperOre => Some(ItemId::Block(BlockType::CopperOre)),
        BlockType::IronOre => Some(ItemId::Block(BlockType::IronOre)),
        BlockType::SilverOre => Some(ItemId::Block(BlockType::SilverOre)),
        BlockType::GoldOre => Some(ItemId::Block(BlockType::GoldOre)),
        BlockType::Mud => Some(ItemId::Block(BlockType::Mud)),
        BlockType::Clay => Some(ItemId::Block(BlockType::Clay)),
        _ => Some(ItemId::Block(block_type)),
    }
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// Spawns dropped item entities when BlockDropEvents are received
fn spawn_drops_system(
    mut commands: Commands,
    mut events: EventReader<BlockDropEvent>,
) {
    let mut rng = rand::thread_rng();

    for event in events.read() {
        let Some(item_id) = block_drop_table(event.block_type) else {
            continue;
        };

        // Small random offset so items don't stack perfectly
        use rand::Rng;
        let offset = Vec3::new(
            rng.gen_range(-0.25..0.25),
            rng.gen_range(0.1..0.4),
            rng.gen_range(-0.25..0.25),
        );

        commands.spawn((
            DroppedItem::new(item_id),
            Transform::from_translation(event.position + offset),
        ));
    }
}

/// Checks player proximity to dropped items and picks them up
fn pickup_system(
    mut commands: Commands,
    dropped_query: Query<(Entity, &DroppedItem, &Transform)>,
    mut player_query: Query<(&Transform, &mut Inventory), With<Player>>,
) {
    const PICKUP_RADIUS: f32 = 1.5;
    const PICKUP_RADIUS_SQ: f32 = PICKUP_RADIUS * PICKUP_RADIUS;
    // Quick rejection: skip items more than 32 blocks away (cheap check)
    const CULL_RADIUS_SQ: f32 = 32.0 * 32.0;

    for (player_transform, mut inventory) in player_query.iter_mut() {
        let player_pos = player_transform.translation;

        for (entity, dropped, item_transform) in dropped_query.iter() {
            let diff = player_pos - item_transform.translation;
            // Cheap axis-aligned check first
            if diff.x.abs() > 32.0 || diff.z.abs() > 32.0 { continue; }
            let distance_sq = diff.length_squared();
            if distance_sq > CULL_RADIUS_SQ { continue; }
            if distance_sq <= PICKUP_RADIUS_SQ {
                let leftover = inventory.add_item(dropped.item, 1);
                if leftover == 0 {
                    commands.entity(entity).despawn();
                }
            }
        }
    }
}

/// Ticks down dropped item lifetimes and despawns expired ones
fn despawn_system(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut DroppedItem)>,
) {
    let dt = time.delta_secs();
    for (entity, mut item) in query.iter_mut() {
        item.lifetime -= dt;
        if item.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that adds the block drops system
pub struct BlockDropPlugin;

impl Plugin for BlockDropPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<BlockDropEvent>()
            .add_systems(FixedUpdate, (
                spawn_drops_system,
                pickup_system,
                despawn_system,
            ));
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drop_table_stone() {
        assert_eq!(block_drop_table(BlockType::Stone), Some(ItemId::Block(BlockType::Stone)));
    }

    #[test]
    fn test_drop_table_air_returns_none() {
        assert_eq!(block_drop_table(BlockType::Air), None);
    }

    #[test]
    fn test_drop_table_water_returns_none() {
        assert_eq!(block_drop_table(BlockType::Water), None);
    }

    #[test]
    fn test_drop_table_wood() {
        assert_eq!(block_drop_table(BlockType::Wood), Some(ItemId::Block(BlockType::Wood)));
    }

    #[test]
    fn test_drop_table_ores() {
        assert_eq!(block_drop_table(BlockType::IronOre), Some(ItemId::Block(BlockType::IronOre)));
        assert_eq!(block_drop_table(BlockType::GoldOre), Some(ItemId::Block(BlockType::GoldOre)));
        assert_eq!(block_drop_table(BlockType::CopperOre), Some(ItemId::Block(BlockType::CopperOre)));
    }

    #[test]
    fn test_dropped_item_default_lifetime() {
        let item = DroppedItem::new(ItemId::Block(BlockType::Stone));
        assert_eq!(item.lifetime, 300.0);
        assert_eq!(item.item, ItemId::Block(BlockType::Stone));
    }

    #[test]
    fn test_block_drop_event_creation() {
        let event = BlockDropEvent {
            block_type: BlockType::Dirt,
            position: Vec3::new(1.0, 2.0, 3.0),
        };
        assert_eq!(event.block_type, BlockType::Dirt);
        assert_eq!(event.position, Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn test_drop_table_leaves_returns_none() {
        assert_eq!(block_drop_table(BlockType::Leaves), None);
    }
}
