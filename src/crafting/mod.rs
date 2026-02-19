//! Crafting system ΓÇö recipe-based item transformation with an egui UI.
//!
//! Players press C to open the crafting window, browse available recipes,
//! and craft items when they have the required materials in their inventory.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiSet};

use crate::actors::Player;
use crate::inventory::{Inventory, ItemId, MaterialType, ToolType};
use crate::world::BlockType;

// ============================================================================
// RECIPE
// ============================================================================

/// A single crafting recipe: consume inputs, produce output.
#[derive(Clone, Debug)]
pub struct Recipe {
    /// Human-readable recipe name.
    pub name: String,
    /// Required input items and their quantities.
    pub inputs: Vec<(ItemId, u32)>,
    /// Produced output item and quantity.
    pub output: (ItemId, u32),
}

impl Recipe {
    /// Create a new recipe.
    pub fn new(name: impl Into<String>, inputs: Vec<(ItemId, u32)>, output: (ItemId, u32)) -> Self {
        Self {
            name: name.into(),
            inputs,
            output,
        }
    }

    /// Check whether the given inventory has all required inputs.
    pub fn can_craft(&self, inventory: &Inventory) -> bool {
        self.inputs
            .iter()
            .all(|&(item, count)| inventory.has_item(item, count))
    }
}

// ============================================================================
// RECIPE REGISTRY
// ============================================================================

/// Resource holding all known crafting recipes.
#[derive(Resource, Clone, Debug, Default)]
pub struct RecipeRegistry {
    pub recipes: Vec<Recipe>,
}

impl RecipeRegistry {
    /// Register a new recipe.
    pub fn register(&mut self, recipe: Recipe) {
        self.recipes.push(recipe);
    }

    /// Find all recipes whose output matches the given item.
    pub fn find_matching(&self, output_item: ItemId) -> Vec<&Recipe> {
        self.recipes
            .iter()
            .filter(|r| r.output.0 == output_item)
            .collect()
    }
}

/// Returns default starter recipes.
pub fn default_recipes() -> Vec<Recipe> {
    vec![
        // Sticks from wood
        Recipe::new(
            "Sticks",
            vec![(ItemId::Block(BlockType::Wood), 1)],
            (ItemId::Material(MaterialType::Stick), 4),
        ),
        // Stone Pickaxe
        Recipe::new(
            "Stone Pickaxe",
            vec![
                (ItemId::Block(BlockType::Stone), 3),
                (ItemId::Material(MaterialType::Stick), 2),
            ],
            (ItemId::Tool(ToolType::StonePickaxe), 1),
        ),
        // Stone Axe
        Recipe::new(
            "Stone Axe",
            vec![
                (ItemId::Block(BlockType::Stone), 3),
                (ItemId::Material(MaterialType::Stick), 2),
            ],
            (ItemId::Tool(ToolType::StoneAxe), 1),
        ),
        // Stone Shovel
        Recipe::new(
            "Stone Shovel",
            vec![
                (ItemId::Block(BlockType::Stone), 1),
                (ItemId::Material(MaterialType::Stick), 2),
            ],
            (ItemId::Tool(ToolType::StoneShovel), 1),
        ),
        // Sandstone from sand
        Recipe::new(
            "Sandstone",
            vec![(ItemId::Block(BlockType::Sand), 4)],
            (ItemId::Block(BlockType::Sandstone), 1),
        ),
        // Wood Pickaxe
        Recipe::new(
            "Wood Pickaxe",
            vec![
                (ItemId::Block(BlockType::Wood), 3),
                (ItemId::Material(MaterialType::Stick), 2),
            ],
            (ItemId::Tool(ToolType::WoodPickaxe), 1),
        ),
    ]
}

// ============================================================================
// CRAFTING STATE
// ============================================================================

/// Tracks whether the crafting UI is open and which recipe is selected.
#[derive(Resource, Debug)]
pub struct CraftingState {
    pub open: bool,
    pub selected_recipe: Option<usize>,
}

impl Default for CraftingState {
    fn default() -> Self {
        Self {
            open: false,
            selected_recipe: None,
        }
    }
}

// ============================================================================
// EVENTS
// ============================================================================

/// Fired when the player requests crafting a recipe by index.
#[derive(Event)]
pub struct CraftEvent {
    pub recipe_index: usize,
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// Toggle the crafting UI with the C key.
fn toggle_crafting_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<CraftingState>,
    mut egui_contexts: EguiContexts,
) {
    let ctx = egui_contexts.ctx_mut();
    if ctx.wants_keyboard_input() {
        return;
    }

    if keys.just_pressed(KeyCode::KeyC) {
        state.open = !state.open;
        if !state.open {
            state.selected_recipe = None;
        }
    }
}

/// Render the crafting UI window.
fn crafting_ui_system(
    mut contexts: EguiContexts,
    mut state: ResMut<CraftingState>,
    registry: Res<RecipeRegistry>,
    query: Query<&Inventory, With<Player>>,
    mut craft_events: EventWriter<CraftEvent>,
) {
    if !state.open {
        return;
    }

    let Ok(inventory) = query.get_single() else {
        return;
    };

    let ctx = contexts.ctx_mut();

    egui::Window::new("Crafting")
        .collapsible(false)
        .resizable(true)
        .default_width(320.0)
        .show(ctx, |ui| {
            ui.label("Available Recipes:");
            ui.separator();

            egui::ScrollArea::vertical()
                .max_height(400.0)
                .show(ui, |ui| {
                    for (i, recipe) in registry.recipes.iter().enumerate() {
                        let can_craft = recipe.can_craft(inventory);
                        let is_selected = state.selected_recipe == Some(i);

                        let text_color = if can_craft {
                            egui::Color32::WHITE
                        } else {
                            egui::Color32::GRAY
                        };

                        ui.horizontal(|ui| {
                            let label = egui::RichText::new(&recipe.name).color(text_color);
                            if ui.selectable_label(is_selected, label).clicked() {
                                state.selected_recipe = Some(i);
                            }
                        });
                    }
                });

            ui.separator();

            // Detail panel for selected recipe
            if let Some(idx) = state.selected_recipe {
                if let Some(recipe) = registry.recipes.get(idx) {
                    ui.label(
                        egui::RichText::new(&recipe.name)
                            .strong()
                            .size(16.0),
                    );

                    ui.label("Requires:");
                    for &(item, count) in &recipe.inputs {
                        let has = inventory.has_item(item, count);
                        let color = if has {
                            egui::Color32::from_rgb(100, 255, 100)
                        } else {
                            egui::Color32::from_rgb(255, 100, 100)
                        };
                        ui.label(
                            egui::RichText::new(format!(
                                "  {} x{}",
                                item.display_name(),
                                count
                            ))
                            .color(color),
                        );
                    }

                    ui.label(format!(
                        "Produces: {} x{}",
                        recipe.output.0.display_name(),
                        recipe.output.1,
                    ));

                    ui.add_space(8.0);

                    let can_craft = recipe.can_craft(inventory);
                    if ui
                        .add_enabled(can_craft, egui::Button::new("Craft"))
                        .clicked()
                    {
                        craft_events.send(CraftEvent { recipe_index: idx });
                    }
                }
            }
        });
}

/// Process craft events: remove inputs, add output.
fn craft_item_system(
    mut craft_events: EventReader<CraftEvent>,
    registry: Res<RecipeRegistry>,
    mut query: Query<&mut Inventory, With<Player>>,
) {
    let Ok(mut inventory) = query.get_single_mut() else {
        return;
    };

    for event in craft_events.read() {
        let Some(recipe) = registry.recipes.get(event.recipe_index) else {
            continue;
        };

        if !recipe.can_craft(&inventory) {
            continue;
        }

        // Remove inputs
        for &(item, count) in &recipe.inputs {
            inventory.remove_item(item, count);
        }

        // Add output
        let (output_item, output_count) = recipe.output;
        inventory.add_item(output_item, output_count);
    }
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that adds the crafting system.
pub struct CraftingPlugin;

impl Plugin for CraftingPlugin {
    fn build(&self, app: &mut App) {
        let mut registry = RecipeRegistry::default();
        for recipe in default_recipes() {
            registry.register(recipe);
        }

        app.insert_resource(registry)
            .init_resource::<CraftingState>()
            .add_event::<CraftEvent>()
            .add_systems(
                Update,
                (toggle_crafting_system, crafting_ui_system, craft_item_system)
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

    fn make_test_inventory() -> Inventory {
        let mut inv = Inventory::new(36);
        inv.add_item(ItemId::Block(BlockType::Wood), 10);
        inv.add_item(ItemId::Block(BlockType::Stone), 10);
        inv.add_item(ItemId::Material(MaterialType::Stick), 10);
        inv
    }

    #[test]
    fn test_recipe_can_craft_true() {
        let inv = make_test_inventory();
        let recipe = Recipe::new(
            "Sticks",
            vec![(ItemId::Block(BlockType::Wood), 1)],
            (ItemId::Material(MaterialType::Stick), 4),
        );
        assert!(recipe.can_craft(&inv));
    }

    #[test]
    fn test_recipe_can_craft_false() {
        let inv = Inventory::new(36);
        let recipe = Recipe::new(
            "Sticks",
            vec![(ItemId::Block(BlockType::Wood), 1)],
            (ItemId::Material(MaterialType::Stick), 4),
        );
        assert!(!recipe.can_craft(&inv));
    }

    #[test]
    fn test_registry_register_and_find() {
        let mut registry = RecipeRegistry::default();
        registry.register(Recipe::new(
            "Sticks",
            vec![(ItemId::Block(BlockType::Wood), 1)],
            (ItemId::Material(MaterialType::Stick), 4),
        ));
        registry.register(Recipe::new(
            "Stone Pickaxe",
            vec![
                (ItemId::Block(BlockType::Stone), 3),
                (ItemId::Material(MaterialType::Stick), 2),
            ],
            (ItemId::Tool(ToolType::StonePickaxe), 1),
        ));

        let matches = registry.find_matching(ItemId::Material(MaterialType::Stick));
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "Sticks");

        let no_matches = registry.find_matching(ItemId::Block(BlockType::Dirt));
        assert!(no_matches.is_empty());
    }

    #[test]
    fn test_craft_removes_inputs_adds_output() {
        let mut inv = make_test_inventory();
        let recipe = Recipe::new(
            "Sticks",
            vec![(ItemId::Block(BlockType::Wood), 1)],
            (ItemId::Material(MaterialType::Stick), 4),
        );

        assert!(recipe.can_craft(&inv));

        // Simulate crafting
        for &(item, count) in &recipe.inputs {
            inv.remove_item(item, count);
        }
        inv.add_item(recipe.output.0, recipe.output.1);

        assert!(inv.has_item(ItemId::Block(BlockType::Wood), 9));
        assert!(inv.has_item(ItemId::Material(MaterialType::Stick), 14));
    }

    #[test]
    fn test_default_recipes_count() {
        let recipes = default_recipes();
        assert!(recipes.len() >= 6);
    }

    #[test]
    fn test_crafting_state_default() {
        let state = CraftingState::default();
        assert!(!state.open);
        assert!(state.selected_recipe.is_none());
    }

    #[test]
    fn test_recipe_insufficient_partial() {
        let mut inv = Inventory::new(36);
        inv.add_item(ItemId::Block(BlockType::Stone), 2); // Need 3
        inv.add_item(ItemId::Material(MaterialType::Stick), 2);

        let recipe = Recipe::new(
            "Stone Pickaxe",
            vec![
                (ItemId::Block(BlockType::Stone), 3),
                (ItemId::Material(MaterialType::Stick), 2),
            ],
            (ItemId::Tool(ToolType::StonePickaxe), 1),
        );
        assert!(!recipe.can_craft(&inv));
    }
}
