//! Inventory system - item stacks, player inventory, and hotbar
//!
//! Provides a Minecraft-style inventory with 36 slots and a 9-slot hotbar.
//! Items can be blocks, tools, materials, or food.

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use crate::actors::Player;
use crate::world::BlockType;

// ============================================================================
// ITEM TYPES
// ============================================================================

/// Tool variants
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolType {
    WoodPickaxe,
    StonePickaxe,
    IronPickaxe,
    WoodAxe,
    StoneAxe,
    IronAxe,
    WoodShovel,
    StoneShovel,
    IronShovel,
}

/// Material variants
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MaterialType {
    Stick,
    CopperIngot,
    IronIngot,
    SilverIngot,
    GoldIngot,
    String,
    Fiber,
}

/// Food variants
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FoodType {
    Apple,
    CookedMeat,
    Bread,
    Berries,
}

/// Unique item identifier
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemId {
    Block(BlockType),
    Tool(ToolType),
    Material(MaterialType),
    Food(FoodType),
}

impl ItemId {
    /// Display name for the item
    pub fn display_name(&self) -> &'static str {
        match self {
            ItemId::Block(b) => b.display_name(),
            ItemId::Tool(t) => match t {
                ToolType::WoodPickaxe => "Wood Pickaxe",
                ToolType::StonePickaxe => "Stone Pickaxe",
                ToolType::IronPickaxe => "Iron Pickaxe",
                ToolType::WoodAxe => "Wood Axe",
                ToolType::StoneAxe => "Stone Axe",
                ToolType::IronAxe => "Iron Axe",
                ToolType::WoodShovel => "Wood Shovel",
                ToolType::StoneShovel => "Stone Shovel",
                ToolType::IronShovel => "Iron Shovel",
            },
            ItemId::Material(m) => match m {
                MaterialType::Stick => "Stick",
                MaterialType::CopperIngot => "Copper Ingot",
                MaterialType::IronIngot => "Iron Ingot",
                MaterialType::SilverIngot => "Silver Ingot",
                MaterialType::GoldIngot => "Gold Ingot",
                MaterialType::String => "String",
                MaterialType::Fiber => "Fiber",
            },
            ItemId::Food(f) => match f {
                FoodType::Apple => "Apple",
                FoodType::CookedMeat => "Cooked Meat",
                FoodType::Bread => "Bread",
                FoodType::Berries => "Berries",
            },
        }
    }

    /// Default max stack size for this item type
    pub fn default_max_stack(&self) -> u32 {
        match self {
            ItemId::Block(_) => 64,
            ItemId::Tool(_) => 1,
            ItemId::Material(_) => 64,
            ItemId::Food(_) => 16,
        }
    }
}

// ============================================================================
// ITEM STACK
// ============================================================================

/// A stack of identical items in a single inventory slot
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemStack {
    pub item: ItemId,
    pub count: u32,
    pub max_stack: u32,
}

impl ItemStack {
    /// Create a new item stack
    pub fn new(item: ItemId, count: u32) -> Self {
        let max_stack = item.default_max_stack();
        Self {
            item,
            count: count.min(max_stack),
            max_stack,
        }
    }

    /// Try to add items to this stack. Returns the number that couldn't fit.
    pub fn add(&mut self, amount: u32) -> u32 {
        let space = self.max_stack - self.count;
        let added = amount.min(space);
        self.count += added;
        amount - added
    }

    /// Try to remove items from this stack. Returns the number actually removed.
    pub fn remove(&mut self, amount: u32) -> u32 {
        let removed = amount.min(self.count);
        self.count -= removed;
        removed
    }

    /// Whether the stack is at max capacity
    pub fn is_full(&self) -> bool {
        self.count >= self.max_stack
    }

    /// Whether the stack is empty
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

// ============================================================================
// INVENTORY COMPONENT
// ============================================================================

/// Player inventory with configurable slot count
#[derive(Component, Clone, Debug)]
pub struct Inventory {
    pub slots: Vec<Option<ItemStack>>,
    pub size: usize,
}

impl Default for Inventory {
    fn default() -> Self {
        Self::new(36)
    }
}

impl Inventory {
    /// Create a new inventory with the given number of slots
    pub fn new(size: usize) -> Self {
        Self {
            slots: vec![None; size],
            size,
        }
    }

    /// Add an item to the inventory. Returns the leftover count that couldn't fit.
    pub fn add_item(&mut self, item: ItemId, mut count: u32) -> u32 {
        // First try to stack with existing matching items
        for slot in self.slots.iter_mut() {
            if count == 0 {
                break;
            }
            if let Some(stack) = slot {
                if stack.item == item && !stack.is_full() {
                    count = stack.add(count);
                }
            }
        }

        // Then fill empty slots
        for slot in self.slots.iter_mut() {
            if count == 0 {
                break;
            }
            if slot.is_none() {
                let mut stack = ItemStack::new(item, 0);
                count = stack.add(count);
                *slot = Some(stack);
            }
        }

        count
    }

    /// Remove items from the inventory. Returns true if the full amount was removed.
    pub fn remove_item(&mut self, item: ItemId, mut count: u32) -> bool {
        let total = count;

        for slot in self.slots.iter_mut() {
            if count == 0 {
                break;
            }
            if let Some(stack) = slot {
                if stack.item == item {
                    let removed = stack.remove(count);
                    count -= removed;
                    if stack.is_empty() {
                        *slot = None;
                    }
                }
            }
        }

        count == 0 && total > 0 || total == 0
    }

    /// Get a reference to a slot
    pub fn get_slot(&self, index: usize) -> Option<&ItemStack> {
        self.slots.get(index).and_then(|s| s.as_ref())
    }

    /// Set a slot directly
    pub fn set_slot(&mut self, index: usize, stack: Option<ItemStack>) {
        if index < self.size {
            self.slots[index] = stack;
        }
    }

    /// Find the first slot containing the given item
    pub fn find_item(&self, item: ItemId) -> Option<usize> {
        self.slots
            .iter()
            .position(|s| s.as_ref().is_some_and(|stack| stack.item == item))
    }

    /// Check if the inventory contains at least `count` of the given item
    pub fn has_item(&self, item: ItemId, count: u32) -> bool {
        let total: u32 = self
            .slots
            .iter()
            .filter_map(|s| s.as_ref())
            .filter(|stack| stack.item == item)
            .map(|stack| stack.count)
            .sum();
        total >= count
    }

    /// Swap two slots
    pub fn swap_slots(&mut self, a: usize, b: usize) {
        if a < self.size && b < self.size {
            self.slots.swap(a, b);
        }
    }
}

// ============================================================================
// HOTBAR COMPONENT
// ============================================================================

/// Hotbar selection ΓÇö indexes into the first N slots of the Inventory
#[derive(Component, Clone, Debug)]
pub struct Hotbar {
    pub selected_slot: usize,
    pub size: usize,
}

impl Default for Hotbar {
    fn default() -> Self {
        Self {
            selected_slot: 0,
            size: 9,
        }
    }
}

impl Hotbar {
    /// Select the next slot (wraps around)
    pub fn select_next(&mut self) {
        self.selected_slot = (self.selected_slot + 1) % self.size;
    }

    /// Select the previous slot (wraps around)
    pub fn select_prev(&mut self) {
        if self.selected_slot == 0 {
            self.selected_slot = self.size - 1;
        } else {
            self.selected_slot -= 1;
        }
    }

    /// Select a specific slot (clamped to size)
    pub fn select_slot(&mut self, idx: usize) {
        if idx < self.size {
            self.selected_slot = idx;
        }
    }
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// Attach Inventory and Hotbar to the player entity on startup
fn attach_inventory_to_player(
    mut commands: Commands,
    query: Query<Entity, (With<Player>, Without<Inventory>)>,
) {
    for entity in query.iter() {
        commands
            .entity(entity)
            .insert((Inventory::default(), Hotbar::default()));
    }
}

/// Handle hotbar input (scroll wheel + number keys)
fn hotbar_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut scroll: EventReader<MouseWheel>,
    mut query: Query<&mut Hotbar>,
    mut egui_contexts: EguiContexts,
) {
    // Don't process input when egui wants keyboard/pointer
    let ctx = egui_contexts.ctx_mut();
    if ctx.wants_keyboard_input() || ctx.wants_pointer_input() {
        return;
    }

    for mut hotbar in query.iter_mut() {
        // Scroll wheel
        for ev in scroll.read() {
            if ev.y > 0.0 {
                hotbar.select_prev();
            } else if ev.y < 0.0 {
                hotbar.select_next();
            }
        }

        // Number keys 1-9
        let keys_map = [
            (KeyCode::Digit1, 0),
            (KeyCode::Digit2, 1),
            (KeyCode::Digit3, 2),
            (KeyCode::Digit4, 3),
            (KeyCode::Digit5, 4),
            (KeyCode::Digit6, 5),
            (KeyCode::Digit7, 6),
            (KeyCode::Digit8, 7),
            (KeyCode::Digit9, 8),
        ];
        for (key, slot) in keys_map {
            if keys.just_pressed(key) {
                hotbar.select_slot(slot);
            }
        }
    }
}

/// Render the hotbar HUD at the bottom of the screen
fn hotbar_hud(mut contexts: EguiContexts, query: Query<(&Inventory, &Hotbar), With<Player>>) {
    let Ok((inventory, hotbar)) = query.get_single() else {
        return;
    };

    let ctx = contexts.ctx_mut();
    let screen_rect = ctx.screen_rect();

    let slot_size = 48.0;
    let slot_padding = 4.0;
    let total_width = hotbar.size as f32 * (slot_size + slot_padding) - slot_padding;
    let bar_x = (screen_rect.width() - total_width) / 2.0;
    let bar_y = screen_rect.height() - slot_size - 16.0;

    egui::Area::new(egui::Id::new("hotbar_hud"))
        .fixed_pos(egui::pos2(bar_x, bar_y))
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                for i in 0..hotbar.size {
                    let is_selected = i == hotbar.selected_slot;
                    let bg_color = if is_selected {
                        egui::Color32::from_rgba_unmultiplied(80, 80, 120, 220)
                    } else {
                        egui::Color32::from_rgba_unmultiplied(30, 30, 30, 180)
                    };
                    let border_color = if is_selected {
                        egui::Color32::from_rgb(200, 200, 255)
                    } else {
                        egui::Color32::from_rgba_unmultiplied(100, 100, 100, 120)
                    };

                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(slot_size, slot_size),
                        egui::Sense::hover(),
                    );

                    // Background
                    ui.painter().rect_filled(rect, 4.0, bg_color);
                    ui.painter().rect_stroke(
                        rect,
                        4.0,
                        egui::Stroke::new(if is_selected { 2.0 } else { 1.0 }, border_color),
                    );

                    // Slot number
                    ui.painter().text(
                        egui::pos2(rect.min.x + 4.0, rect.min.y + 2.0),
                        egui::Align2::LEFT_TOP,
                        format!("{}", i + 1),
                        egui::FontId::proportional(10.0),
                        egui::Color32::from_rgba_unmultiplied(180, 180, 180, 150),
                    );

                    // Item content
                    if let Some(stack) = inventory.get_slot(i) {
                        // Item name (abbreviated)
                        let name = stack.item.display_name();
                        let short_name = if name.len() > 6 { &name[..6] } else { name };
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            short_name,
                            egui::FontId::proportional(11.0),
                            egui::Color32::WHITE,
                        );

                        // Count (bottom-right)
                        if stack.count > 1 {
                            ui.painter().text(
                                egui::pos2(rect.max.x - 4.0, rect.max.y - 2.0),
                                egui::Align2::RIGHT_BOTTOM,
                                format!("{}", stack.count),
                                egui::FontId::proportional(12.0),
                                egui::Color32::from_rgb(255, 255, 200),
                            );
                        }
                    }
                }
            });
        });
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that adds the inventory system
pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (attach_inventory_to_player, hotbar_input, hotbar_hud),
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
    fn test_item_stack_add_remove() {
        let mut stack = ItemStack::new(ItemId::Block(BlockType::Stone), 10);
        assert_eq!(stack.count, 10);

        let leftover = stack.add(5);
        assert_eq!(leftover, 0);
        assert_eq!(stack.count, 15);

        let removed = stack.remove(8);
        assert_eq!(removed, 8);
        assert_eq!(stack.count, 7);
    }

    #[test]
    fn test_item_stack_overflow() {
        let mut stack = ItemStack::new(ItemId::Block(BlockType::Dirt), 60);
        assert_eq!(stack.count, 60);

        let leftover = stack.add(10);
        assert_eq!(leftover, 6);
        assert_eq!(stack.count, 64);
        assert!(stack.is_full());
    }

    #[test]
    fn test_inventory_add_item() {
        let mut inv = Inventory::new(36);
        let leftover = inv.add_item(ItemId::Block(BlockType::Stone), 10);
        assert_eq!(leftover, 0);
        assert!(inv.get_slot(0).is_some());
        assert_eq!(inv.get_slot(0).unwrap().count, 10);
        assert!(inv.get_slot(1).is_none());
    }

    #[test]
    fn test_inventory_add_stacking() {
        let mut inv = Inventory::new(36);
        inv.add_item(ItemId::Block(BlockType::Stone), 30);
        inv.add_item(ItemId::Block(BlockType::Stone), 20);

        // Should stack into the first slot
        assert_eq!(inv.get_slot(0).unwrap().count, 50);
        assert!(inv.get_slot(1).is_none());
    }

    #[test]
    fn test_inventory_remove_item() {
        let mut inv = Inventory::new(36);
        inv.add_item(ItemId::Block(BlockType::Stone), 10);

        assert!(inv.remove_item(ItemId::Block(BlockType::Stone), 5));
        assert_eq!(inv.get_slot(0).unwrap().count, 5);

        assert!(!inv.remove_item(ItemId::Block(BlockType::Dirt), 1));
    }

    #[test]
    fn test_inventory_swap_slots() {
        let mut inv = Inventory::new(36);
        inv.set_slot(0, Some(ItemStack::new(ItemId::Block(BlockType::Stone), 10)));
        inv.set_slot(1, Some(ItemStack::new(ItemId::Block(BlockType::Dirt), 5)));

        inv.swap_slots(0, 1);

        assert_eq!(
            inv.get_slot(0).unwrap().item,
            ItemId::Block(BlockType::Dirt)
        );
        assert_eq!(
            inv.get_slot(1).unwrap().item,
            ItemId::Block(BlockType::Stone)
        );
    }

    #[test]
    fn test_hotbar_cycle() {
        let mut hotbar = Hotbar::default();
        assert_eq!(hotbar.selected_slot, 0);

        hotbar.select_next();
        assert_eq!(hotbar.selected_slot, 1);

        hotbar.select_prev();
        assert_eq!(hotbar.selected_slot, 0);

        // Wrap around backwards
        hotbar.select_prev();
        assert_eq!(hotbar.selected_slot, 8);

        // Wrap around forwards
        hotbar.select_next();
        assert_eq!(hotbar.selected_slot, 0);
    }
}
