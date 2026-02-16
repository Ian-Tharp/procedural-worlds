//! InventoryΓÇôHealth Bridge
//!
//! Connects the inventory system to the health system:
//! - Food consumption: press F to eat food from inventory, restoring health/hunger
//! - Death drops: when a player dies, drop all inventory items as loot entities
//! - HUD indicators: show available food items and their restoration amounts

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiSet, egui};

use crate::actors::Player;
use crate::health::{DeathEvent, Health, Hunger};
use crate::inventory::{FoodType, Inventory, ItemId, ItemStack};

// ============================================================================
// FOOD PROPERTIES
// ============================================================================

/// Properties for food items ΓÇö how much health and hunger they restore.
#[derive(Clone, Copy, Debug)]
pub struct FoodProperty {
    pub food: FoodType,
    pub health_restore: f32,
    pub hunger_restore: f32,
}

/// Returns the food properties for a given food type.
pub fn food_properties(food: FoodType) -> FoodProperty {
    match food {
        FoodType::Apple => FoodProperty {
            food,
            health_restore: 2.0,
            hunger_restore: 4.0,
        },
        FoodType::CookedMeat => FoodProperty {
            food,
            health_restore: 6.0,
            hunger_restore: 8.0,
        },
        FoodType::Bread => FoodProperty {
            food,
            health_restore: 3.0,
            hunger_restore: 5.0,
        },
        FoodType::Berries => FoodProperty {
            food,
            health_restore: 1.0,
            hunger_restore: 2.0,
        },
    }
}

/// All known food types for iteration.
const ALL_FOODS: [FoodType; 4] = [
    FoodType::Apple,
    FoodType::CookedMeat,
    FoodType::Bread,
    FoodType::Berries,
];

// ============================================================================
// EVENTS
// ============================================================================

/// Fired when a player consumes food from their inventory.
#[derive(Event, Debug, Clone)]
pub struct ConsumeFoodEvent {
    pub player_entity: Entity,
    pub food_type: FoodType,
    pub health_restored: f32,
    pub hunger_restored: f32,
}

/// Fired when a player's inventory is dropped as loot on death.
#[derive(Event, Debug, Clone)]
pub struct InventoryDropEvent {
    pub player_entity: Entity,
    pub items: Vec<ItemStack>,
    pub position: Vec3,
}

// ============================================================================
// COMPONENTS
// ============================================================================

/// Marker for dropped loot entities in the world.
#[derive(Component, Debug)]
pub struct DroppedLoot {
    pub item: ItemId,
    pub count: u32,
    pub despawn_timer: f32,
}

/// Tracks which food the player can eat (best available).
#[derive(Component, Debug, Default)]
pub struct FoodPrompt {
    /// The best food available in inventory, if any.
    pub available_food: Option<FoodType>,
    /// Whether the prompt should be visible (health < max or hunger < max).
    pub visible: bool,
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// Attach FoodPrompt to players that have an inventory but no prompt yet.
fn attach_food_prompt(
    mut commands: Commands,
    #[allow(clippy::type_complexity)] query: Query<
        Entity,
        (With<Player>, With<Inventory>, Without<FoodPrompt>),
    >,
) {
    for entity in &query {
        commands.entity(entity).insert(FoodPrompt::default());
    }
}

/// Scan player inventory for food items and update the prompt.
fn update_food_prompt(
    mut query: Query<(&Inventory, &Health, &Hunger, &mut FoodPrompt), With<Player>>,
) {
    for (inventory, health, hunger, mut prompt) in &mut query {
        let needs_food = health.current < health.max || hunger.current < hunger.max;
        prompt.visible = needs_food;

        if !needs_food {
            prompt.available_food = None;
            continue;
        }

        // Find the best food in inventory (highest health restore)
        let mut best: Option<(FoodType, f32)> = None;
        for food_type in &ALL_FOODS {
            let item_id = ItemId::Food(*food_type);
            if inventory.has_item(item_id, 1) {
                let props = food_properties(*food_type);
                match best {
                    Some((_, best_restore)) if props.health_restore <= best_restore => {}
                    _ => best = Some((*food_type, props.health_restore)),
                }
            }
        }

        prompt.available_food = best.map(|(f, _)| f);
    }
}

/// Handle F key press to consume food.
fn food_consumption_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut query: Query<(Entity, &FoodPrompt, &mut Inventory), With<Player>>,
    mut consume_events: EventWriter<ConsumeFoodEvent>,
    mut egui_contexts: EguiContexts,
) {
    let ctx = egui_contexts.ctx_mut();
    if ctx.wants_keyboard_input() {
        return;
    }

    if !keys.just_pressed(KeyCode::KeyF) {
        return;
    }

    for (entity, prompt, mut inventory) in &mut query {
        if let Some(food_type) = prompt.available_food {
            let item_id = ItemId::Food(food_type);
            if inventory.remove_item(item_id, 1) {
                let props = food_properties(food_type);
                consume_events.send(ConsumeFoodEvent {
                    player_entity: entity,
                    food_type,
                    health_restored: props.health_restore,
                    hunger_restored: props.hunger_restore,
                });
            }
        }
    }
}

/// Apply health and hunger restoration from consumed food.
fn apply_food_consumption(
    mut events: EventReader<ConsumeFoodEvent>,
    mut query: Query<(&mut Health, &mut Hunger)>,
) {
    for event in events.read() {
        if let Ok((mut health, mut hunger)) = query.get_mut(event.player_entity) {
            health.heal(event.health_restored);
            hunger.consume(event.hunger_restored);
            info!(
                "Player ate {:?}: +{} health, +{} hunger",
                event.food_type, event.health_restored, event.hunger_restored
            );
        }
    }
}

/// On death, drop all inventory items as loot entities and clear inventory.
fn handle_death_drop_inventory(
    mut death_events: EventReader<DeathEvent>,
    mut query: Query<(&mut Inventory, &Transform), With<Player>>,
    mut drop_events: EventWriter<InventoryDropEvent>,
) {
    for event in death_events.read() {
        if let Ok((mut inventory, transform)) = query.get_mut(event.entity) {
            let mut dropped_items = Vec::new();
            for slot in inventory.slots.iter_mut() {
                if let Some(stack) = slot.take() {
                    dropped_items.push(stack);
                }
            }

            if !dropped_items.is_empty() {
                info!(
                    "Player died ΓÇö dropping {} item stacks as loot",
                    dropped_items.len()
                );
                drop_events.send(InventoryDropEvent {
                    player_entity: event.entity,
                    items: dropped_items,
                    position: transform.translation,
                });
            }
        }
    }
}

/// Spawn loot entities in the world when inventory is dropped.
fn spawn_dropped_loot(
    mut commands: Commands,
    mut drop_events: EventReader<InventoryDropEvent>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for event in drop_events.read() {
        for (i, stack) in event.items.iter().enumerate() {
            // Scatter items around the death position
            let angle = (i as f32 / event.items.len() as f32) * std::f32::consts::TAU;
            let offset = Vec3::new(angle.cos() * 1.5, 0.5, angle.sin() * 1.5);
            let pos = event.position + offset;

            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.3, 0.3, 0.3))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(0.8, 0.6, 0.2),
                    ..default()
                })),
                Transform::from_translation(pos),
                DroppedLoot {
                    item: stack.item,
                    count: stack.count,
                    despawn_timer: 300.0, // 5 minutes
                },
            ));
        }
    }
}

/// Despawn loot entities after their timer expires.
fn despawn_loot_timer(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut DroppedLoot)>,
) {
    let dt = time.delta_secs();
    for (entity, mut loot) in &mut query {
        loot.despawn_timer -= dt;
        if loot.despawn_timer <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

// ============================================================================
// HUD
// ============================================================================

/// Show food consumption prompt and available food items in the HUD.
fn food_hud_system(
    mut contexts: EguiContexts,
    query: Query<(&Health, &Hunger, &FoodPrompt, &Inventory), With<Player>>,
) {
    let Ok((_health, _hunger, prompt, inventory)) = query.get_single() else {
        return;
    };

    if !prompt.visible {
        return;
    }

    let ctx = contexts.ctx_mut();

    egui::Area::new(egui::Id::new("food_hud"))
        .fixed_pos(egui::pos2(16.0, 140.0))
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 140))
                .rounding(egui::Rounding::same(6.0))
                .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                .show(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);

                    // Title
                    ui.label(
                        egui::RichText::new("≡ƒìÄ Food")
                            .color(egui::Color32::from_rgb(150, 220, 150))
                            .size(13.0)
                            .strong(),
                    );

                    ui.add_space(2.0);

                    // List available food items with restoration amounts
                    let mut any_food = false;
                    for food_type in &ALL_FOODS {
                        let item_id = ItemId::Food(*food_type);
                        if inventory.has_item(item_id, 1) {
                            any_food = true;
                            let props = food_properties(*food_type);
                            let count: u32 = inventory
                                .slots
                                .iter()
                                .filter_map(|s| s.as_ref())
                                .filter(|s| s.item == item_id)
                                .map(|s| s.count)
                                .sum();
                            let label = format!(
                                "  {} x{} (+{:.0}Γ¥ñ +{:.0}≡ƒìû)",
                                item_id.display_name(),
                                count,
                                props.health_restore,
                                props.hunger_restore,
                            );
                            let is_best = prompt.available_food == Some(*food_type);
                            let color = if is_best {
                                egui::Color32::from_rgb(255, 255, 200)
                            } else {
                                egui::Color32::from_rgb(180, 180, 180)
                            };
                            ui.label(egui::RichText::new(label).color(color).size(11.0));
                        }
                    }

                    if !any_food {
                        ui.label(
                            egui::RichText::new("  No food items")
                                .color(egui::Color32::from_rgb(150, 100, 100))
                                .size(11.0),
                        );
                    }

                    // Show eat prompt if food is available
                    if prompt.available_food.is_some() {
                        ui.add_space(4.0);
                        let food_name = prompt
                            .available_food
                            .map(|f| ItemId::Food(f).display_name())
                            .unwrap_or("food");
                        let props = prompt.available_food.map(food_properties);
                        let restore_text = props
                            .map(|p| format!("+{:.0}Γ¥ñ", p.health_restore))
                            .unwrap_or_default();
                        ui.label(
                            egui::RichText::new(format!(
                                "  [F] Eat {} ({})",
                                food_name, restore_text
                            ))
                            .color(egui::Color32::from_rgb(100, 255, 100))
                            .size(12.0)
                            .strong(),
                        );
                    }
                });
        });
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that bridges the inventory and health systems.
pub struct InventoryHealthPlugin;

impl Plugin for InventoryHealthPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<ConsumeFoodEvent>()
            .add_event::<InventoryDropEvent>()
            .add_systems(
                Update,
                (
                    attach_food_prompt,
                    update_food_prompt,
                    apply_food_consumption,
                    handle_death_drop_inventory,
                    spawn_dropped_loot,
                    despawn_loot_timer,
                )
                    .chain(),
            )
            // Systems using EguiContexts must run after egui init
            .add_systems(
                Update,
                (food_consumption_input, food_hud_system)
                    .after(EguiSet::InitContexts),
            );
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::Health;
    use crate::inventory::{FoodType, Inventory, ItemId};

    #[test]
    fn test_food_properties_defined() {
        for food in &ALL_FOODS {
            let props = food_properties(*food);
            assert!(props.health_restore > 0.0);
            assert!(props.hunger_restore > 0.0);
        }
    }

    #[test]
    fn test_food_properties_values() {
        let apple = food_properties(FoodType::Apple);
        assert!((apple.health_restore - 2.0).abs() < f32::EPSILON);
        assert!((apple.hunger_restore - 4.0).abs() < f32::EPSILON);

        let meat = food_properties(FoodType::CookedMeat);
        assert!((meat.health_restore - 6.0).abs() < f32::EPSILON);
        assert!((meat.hunger_restore - 8.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_consume_food_removes_from_inventory() {
        let mut inv = Inventory::new(36);
        inv.add_item(ItemId::Food(FoodType::Apple), 3);
        assert!(inv.has_item(ItemId::Food(FoodType::Apple), 3));

        assert!(inv.remove_item(ItemId::Food(FoodType::Apple), 1));
        assert!(inv.has_item(ItemId::Food(FoodType::Apple), 2));
        assert!(!inv.has_item(ItemId::Food(FoodType::Apple), 3));
    }

    #[test]
    fn test_consume_food_empty_inventory() {
        let mut inv = Inventory::new(36);
        assert!(!inv.remove_item(ItemId::Food(FoodType::Apple), 1));
    }

    #[test]
    fn test_health_restore_capped_at_max() {
        let mut health = Health::new(20.0);
        health.damage(5.0); // 15 hp
        health.invulnerable_timer = 0.0;
        health.heal(100.0); // should cap at 20
        assert!((health.current - 20.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_death_drops_all_items() {
        let mut inv = Inventory::new(36);
        inv.add_item(ItemId::Food(FoodType::Apple), 5);
        inv.add_item(ItemId::Food(FoodType::Bread), 3);

        // Simulate death drop: collect all items and clear
        let mut dropped = Vec::new();
        for slot in inv.slots.iter_mut() {
            if let Some(stack) = slot.take() {
                dropped.push(stack);
            }
        }

        assert_eq!(dropped.len(), 2);
        assert!(!inv.has_item(ItemId::Food(FoodType::Apple), 1));
        assert!(!inv.has_item(ItemId::Food(FoodType::Bread), 1));
    }

    #[test]
    fn test_multiple_food_consumption() {
        let mut inv = Inventory::new(36);
        inv.add_item(ItemId::Food(FoodType::Apple), 2);
        inv.add_item(ItemId::Food(FoodType::CookedMeat), 1);

        // Eat one apple
        assert!(inv.remove_item(ItemId::Food(FoodType::Apple), 1));
        // Eat cooked meat
        assert!(inv.remove_item(ItemId::Food(FoodType::CookedMeat), 1));
        // Still have one apple
        assert!(inv.has_item(ItemId::Food(FoodType::Apple), 1));
        // No more meat
        assert!(!inv.has_item(ItemId::Food(FoodType::CookedMeat), 1));
    }
}
