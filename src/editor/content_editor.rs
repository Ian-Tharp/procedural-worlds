//! Content Editor - Draggable window for creating/editing game content
//!
//! Features:
//! - Draggable floating window
//! - Full-screen mode (pauses game)
//! - Browse/create/edit ores, blocks, biomes
//! - Save changes to disk

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::content::{OreDefinition, OreRegistry, BiomeFilter};

// ============================================================================
// STATE
// ============================================================================

/// Which content type is being viewed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContentTab {
    #[default]
    Ores,
    // Future: Blocks, Biomes, Structures
}

/// Current editing state
#[derive(Debug, Clone, Default)]
pub struct EditingState {
    /// Currently selected ore ID (if any)
    pub selected_ore: Option<String>,
    /// Working copy of the ore being edited
    pub editing_ore: Option<OreDefinition>,
    /// Has unsaved changes
    pub dirty: bool,
    /// Error message to display
    pub error_message: Option<String>,
    /// Success message to display
    pub success_message: Option<String>,
    /// Message display timer
    pub message_timer: f32,
}

/// Content editor window state
#[derive(Resource)]
pub struct ContentEditorState {
    /// Is the editor window visible
    pub visible: bool,
    /// Is in full-screen mode (pauses game)
    pub fullscreen: bool,
    /// Window position (for draggable mode)
    pub window_pos: egui::Pos2,
    /// Window size
    pub window_size: egui::Vec2,
    /// Current content tab
    pub current_tab: ContentTab,
    /// Editing state
    pub editing: EditingState,
    /// Show delete confirmation dialog
    pub show_delete_confirm: bool,
}

impl Default for ContentEditorState {
    fn default() -> Self {
        Self {
            visible: false,
            fullscreen: false,
            window_pos: egui::pos2(100.0, 100.0),
            window_size: egui::vec2(700.0, 500.0),
            current_tab: ContentTab::Ores,
            editing: EditingState::default(),
            show_delete_confirm: false,
        }
    }
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin for the content editor
pub struct ContentEditorPlugin;

impl Plugin for ContentEditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ContentEditorState>()
            .add_systems(Update, (
                content_editor_toggle_system,
                content_editor_ui_system,
                update_message_timer,
            ).chain());
    }
}

/// Handle keyboard toggle for content editor (F10)
fn content_editor_toggle_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<ContentEditorState>,
) {
    if keyboard.just_pressed(KeyCode::F10) {
        state.visible = !state.visible;
        if !state.visible {
            state.fullscreen = false;
        }
    }
    
    // ESC exits fullscreen or closes editor
    if keyboard.just_pressed(KeyCode::Escape) {
        if state.fullscreen {
            state.fullscreen = false;
        } else if state.visible {
            state.visible = false;
        }
    }
}

/// Update message display timer
fn update_message_timer(
    mut state: ResMut<ContentEditorState>,
    time: Res<Time>,
) {
    if state.editing.message_timer > 0.0 {
        state.editing.message_timer -= time.delta_secs();
        if state.editing.message_timer <= 0.0 {
            state.editing.error_message = None;
            state.editing.success_message = None;
        }
    }
}

// ============================================================================
// UI SYSTEM
// ============================================================================

/// Main content editor UI system
fn content_editor_ui_system(
    mut contexts: EguiContexts,
    mut state: ResMut<ContentEditorState>,
    mut ore_registry: Option<ResMut<OreRegistry>>,
) {
    if !state.visible {
        return;
    }

    let ctx = contexts.ctx_mut();

    // Full-screen overlay if in fullscreen mode
    if state.fullscreen {
        draw_fullscreen_overlay(ctx);
    }

    // Draw the main editor window
    if state.fullscreen {
        draw_fullscreen_editor(ctx, &mut state, ore_registry.as_deref_mut());
    } else {
        draw_floating_editor(ctx, &mut state, ore_registry.as_deref_mut());
    }
}

/// Draw semi-transparent overlay for fullscreen mode
fn draw_fullscreen_overlay(ctx: &egui::Context) {
    egui::Area::new(egui::Id::new("content_editor_overlay"))
        .fixed_pos(egui::pos2(0.0, 0.0))
        .order(egui::Order::Background)
        .interactable(false)
        .show(ctx, |ui| {
            let screen = ui.ctx().screen_rect();
            ui.painter().rect_filled(
                screen,
                0.0,
                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 180),
            );
        });
}

/// Draw the floating (draggable) editor window
fn draw_floating_editor(
    ctx: &egui::Context,
    state: &mut ContentEditorState,
    ore_registry: Option<&mut OreRegistry>,
) {
    egui::Window::new("📦 Content Editor")
        .id(egui::Id::new("content_editor_window"))
        .default_pos(state.window_pos)
        .default_size(state.window_size)
        .resizable(true)
        .collapsible(true)
        .show(ctx, |ui| {
            draw_editor_content(ui, state, ore_registry);
        });
}

/// Draw the fullscreen editor
fn draw_fullscreen_editor(
    ctx: &egui::Context,
    state: &mut ContentEditorState,
    ore_registry: Option<&mut OreRegistry>,
) {
    let screen = ctx.screen_rect();
    let margin = 40.0;

    egui::Window::new("📦 Content Editor")
        .id(egui::Id::new("content_editor_fullscreen"))
        .fixed_pos(egui::pos2(margin, margin))
        .fixed_size(egui::vec2(
            screen.width() - margin * 2.0,
            screen.height() - margin * 2.0,
        ))
        .title_bar(true)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            draw_editor_content(ui, state, ore_registry);
        });
}

/// Draw the editor content (shared between floating and fullscreen)
fn draw_editor_content(
    ui: &mut egui::Ui,
    state: &mut ContentEditorState,
    ore_registry: Option<&mut OreRegistry>,
) {
    // Toolbar
    ui.horizontal(|ui| {
        // Tab buttons
        ui.selectable_value(&mut state.current_tab, ContentTab::Ores, "📦 Ores");
        // Future tabs:
        // ui.selectable_value(&mut state.current_tab, ContentTab::Blocks, "🧱 Blocks");
        // ui.selectable_value(&mut state.current_tab, ContentTab::Biomes, "🌲 Biomes");

        ui.separator();

        // Fullscreen toggle
        let fullscreen_text = if state.fullscreen { "⊟ Exit Fullscreen" } else { "⊞ Fullscreen" };
        if ui.button(fullscreen_text).on_hover_text("Toggle fullscreen mode (pauses game)").clicked() {
            state.fullscreen = !state.fullscreen;
        }

        // Close button
        if ui.button("✕ Close").on_hover_text("Close editor (F10)").clicked() {
            state.visible = false;
            state.fullscreen = false;
        }
    });

    ui.separator();

    // Status messages
    if let Some(ref error) = state.editing.error_message {
        ui.horizontal(|ui| {
            ui.colored_label(egui::Color32::from_rgb(255, 100, 100), format!("❌ {}", error));
        });
    }
    if let Some(ref success) = state.editing.success_message {
        ui.horizontal(|ui| {
            ui.colored_label(egui::Color32::from_rgb(100, 255, 100), format!("✓ {}", success));
        });
    }

    // Main content area
    ui.separator();

    match state.current_tab {
        ContentTab::Ores => draw_ores_tab(ui, state, ore_registry),
    }
}

/// Draw the ores tab
fn draw_ores_tab(
    ui: &mut egui::Ui,
    state: &mut ContentEditorState,
    ore_registry: Option<&mut OreRegistry>,
) {
    let Some(registry) = ore_registry else {
        ui.colored_label(egui::Color32::from_rgb(255, 200, 100), "Ore registry not loaded");
        return;
    };

    // Split into left (list) and right (editor) panels
    ui.columns(2, |columns| {
        // LEFT: Ore list
        let left = &mut columns[0];
        left.heading("Registered Ores");
        left.separator();

        // New ore button
        if left.button("➕ New Ore").on_hover_text("Create a new ore definition").clicked() {
            let new_ore = registry.create_new();
            state.editing.selected_ore = Some(new_ore.id.clone());
            state.editing.editing_ore = Some(new_ore);
            state.editing.dirty = true;
        }

        left.separator();

        // Ore list
        egui::ScrollArea::vertical()
            .id_salt("ore_list")
            .show(left, |ui| {
                let ore_ids = registry.ids_sorted();
                for id in ore_ids {
                    if let Some(ore) = registry.get(&id) {
                        let is_selected = state.editing.selected_ore.as_ref() == Some(&id);
                        let label = if ore.user_content {
                            format!("📦 {}*", ore.display_name)
                        } else {
                            format!("📦 {}", ore.display_name)
                        };

                        let response = ui.selectable_label(is_selected, &label);
                        
                        if response.clicked() {
                            state.editing.selected_ore = Some(id.clone());
                            state.editing.editing_ore = Some(ore.clone());
                            state.editing.dirty = false;
                        }
                        
                        response.on_hover_text(format!(
                            "ID: {}\nY: {}-{}\nFrequency: {:.3}",
                            ore.id,
                            ore.generation.min_y,
                            ore.generation.max_y,
                            ore.generation.frequency
                        ));
                    }
                }
            });

        left.separator();
        left.small("* = user content");

        // RIGHT: Ore editor
        let right = &mut columns[1];
        
        // Check if we have an ore to edit
        let has_ore = state.editing.editing_ore.is_some();
        let is_dirty = state.editing.dirty;
        let is_user_content = state.editing.editing_ore.as_ref().map(|o| o.user_content).unwrap_or(false);
        let ore_name = state.editing.editing_ore.as_ref().map(|o| o.display_name.clone()).unwrap_or_default();
        let ore_id = state.editing.editing_ore.as_ref().map(|o| o.id.clone());

        if has_ore {
            // Draw the editor form
            if let Some(ref mut editing_ore) = state.editing.editing_ore {
                draw_ore_editor(right, editing_ore, &mut state.editing.dirty);
            }

            right.separator();

            // Action buttons
            let save_text = if is_dirty {
                egui::RichText::new("💾 Save").color(egui::Color32::from_rgb(255, 200, 100)).strong()
            } else {
                egui::RichText::new("💾 Save")
            };

            let (save_clicked, revert_clicked, delete_clicked) = right.horizontal(|ui| {
                let save = ui.button(save_text).on_hover_text("Save changes to disk").clicked();
                let revert = ui.button("↩ Revert").on_hover_text("Discard changes").clicked();
                ui.separator();

                let delete = if is_user_content {
                    ui.button("🗑 Delete")
                        .on_hover_text("Delete this ore definition")
                        .clicked()
                } else {
                    ui.add_enabled(false, egui::Button::new("🗑 Delete"))
                        .on_hover_text("Cannot delete built-in ores");
                    false
                };
                (save, revert, delete)
            }).inner;

            // Handle button actions
            if save_clicked {
                if let Some(ore) = state.editing.editing_ore.clone() {
                    match registry.save(ore.clone()) {
                        Ok(()) => {
                            state.editing.success_message = Some(format!("Saved {}", ore.id));
                            state.editing.message_timer = 3.0;
                            state.editing.dirty = false;
                        }
                        Err(e) => {
                            state.editing.error_message = Some(e);
                            state.editing.message_timer = 5.0;
                        }
                    }
                }
            }

            if revert_clicked {
                if let Some(ref id) = state.editing.selected_ore {
                    if let Some(original) = registry.get(id) {
                        state.editing.editing_ore = Some(original.clone());
                        state.editing.dirty = false;
                    }
                }
            }

            if delete_clicked {
                state.show_delete_confirm = true;
            }

            // Delete confirmation
            if state.show_delete_confirm {
                right.separator();
                right.colored_label(
                    egui::Color32::from_rgb(255, 200, 100),
                    format!("Delete '{}'? This cannot be undone.", ore_name),
                );
                
                let (confirm_delete, cancel_delete) = right.horizontal(|ui| {
                    (ui.button("Yes, Delete").clicked(), ui.button("Cancel").clicked())
                }).inner;

                if confirm_delete {
                    if let Some(id) = ore_id {
                        match registry.delete(&id) {
                            Ok(()) => {
                                state.editing.success_message = Some(format!("Deleted {}", id));
                                state.editing.message_timer = 3.0;
                                state.editing.selected_ore = None;
                                state.editing.editing_ore = None;
                                state.editing.dirty = false;
                            }
                            Err(e) => {
                                state.editing.error_message = Some(e);
                                state.editing.message_timer = 5.0;
                            }
                        }
                    }
                    state.show_delete_confirm = false;
                }
                if cancel_delete {
                    state.show_delete_confirm = false;
                }
            }

            // Dirty indicator
            if is_dirty {
                right.separator();
                right.colored_label(
                    egui::Color32::from_rgb(255, 200, 100),
                    "⚠ Unsaved changes",
                );
            }
        } else {
            right.centered_and_justified(|ui| {
                ui.label("Select an ore to edit, or create a new one");
            });
        }
    });
}

/// Draw the ore editor form
fn draw_ore_editor(ui: &mut egui::Ui, ore: &mut OreDefinition, dirty: &mut bool) {
    ui.heading(&ore.display_name);
    ui.separator();

    egui::Grid::new("ore_editor_grid")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            // Basic Info
            ui.label("ID:");
            if ui.add(egui::TextEdit::singleline(&mut ore.id).hint_text("unique_id")).changed() {
                *dirty = true;
            }
            ui.end_row();

            ui.label("Display Name:");
            if ui.add(egui::TextEdit::singleline(&mut ore.display_name).hint_text("Display Name")).changed() {
                *dirty = true;
            }
            ui.end_row();

            ui.label("Description:");
            if ui.add(
                egui::TextEdit::multiline(&mut ore.description)
                    .desired_rows(2)
                    .hint_text("Optional description...")
            ).changed() {
                *dirty = true;
            }
            ui.end_row();

            ui.label("Texture Index:");
            if ui.add(egui::DragValue::new(&mut ore.texture_index).range(0..=255)).changed() {
                *dirty = true;
            }
            ui.end_row();

            ui.label("Hardness:");
            if ui.add(
                egui::Slider::new(&mut ore.hardness, 0.5..=10.0)
            ).on_hover_text("Mining difficulty (1=dirt, 5=obsidian)").changed() {
                *dirty = true;
            }
            ui.end_row();

            ui.label("Tool Required:");
            if ui.add(egui::TextEdit::singleline(&mut ore.tool_required).hint_text("pickaxe")).changed() {
                *dirty = true;
            }
            ui.end_row();
        });

    ui.separator();
    ui.label(egui::RichText::new("Generation").strong());

    egui::Grid::new("ore_generation_grid")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Y Range:");
            ui.horizontal(|ui| {
                if ui.add(egui::DragValue::new(&mut ore.generation.min_y).range(-64..=320)).changed() {
                    *dirty = true;
                }
                ui.label("to");
                if ui.add(egui::DragValue::new(&mut ore.generation.max_y).range(-64..=320)).changed() {
                    *dirty = true;
                }
            });
            ui.end_row();

            ui.label("Vein Size:");
            if ui.add(
                egui::Slider::new(&mut ore.generation.vein_size, 1..=32)
            ).on_hover_text("Average blocks per vein").changed() {
                *dirty = true;
            }
            ui.end_row();

            ui.label("Frequency:");
            if ui.add(
                egui::Slider::new(&mut ore.generation.frequency, 0.0001..=0.1)
                    .logarithmic(true)
            ).on_hover_text("Spawn rate (0.001=rare, 0.05=common)").changed() {
                *dirty = true;
            }
            ui.end_row();

            ui.label("Biomes:");
            draw_biome_filter_editor(ui, &mut ore.generation.biomes, dirty);
            ui.end_row();
        });

    ui.separator();
    ui.label(egui::RichText::new("Drops").strong());

    egui::Grid::new("ore_drops_grid")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Item:");
            if ui.add(egui::TextEdit::singleline(&mut ore.drop.item).hint_text("item_id")).changed() {
                *dirty = true;
            }
            ui.end_row();

            ui.label("Count:");
            ui.horizontal(|ui| {
                if ui.add(egui::DragValue::new(&mut ore.drop.min_count).range(1..=64)).changed() {
                    *dirty = true;
                }
                ui.label("to");
                if ui.add(egui::DragValue::new(&mut ore.drop.max_count).range(1..=64)).changed() {
                    *dirty = true;
                }
            });
            ui.end_row();
        });
}

/// Draw biome filter editor
fn draw_biome_filter_editor(ui: &mut egui::Ui, filter: &mut BiomeFilter, dirty: &mut bool) {
    let mut filter_type = match filter {
        BiomeFilter::All => 0,
        BiomeFilter::Only(_) => 1,
        BiomeFilter::Except(_) => 2,
    };

    egui::ComboBox::from_id_salt("biome_filter")
        .selected_text(match filter_type {
            0 => "All Biomes",
            1 => "Only Specific",
            2 => "Except Specific",
            _ => "Unknown",
        })
        .show_ui(ui, |ui| {
            if ui.selectable_value(&mut filter_type, 0, "All Biomes").changed() {
                *filter = BiomeFilter::All;
                *dirty = true;
            }
            if ui.selectable_value(&mut filter_type, 1, "Only Specific").changed() {
                *filter = BiomeFilter::Only(vec!["mountains".to_string()]);
                *dirty = true;
            }
            if ui.selectable_value(&mut filter_type, 2, "Except Specific").changed() {
                *filter = BiomeFilter::Except(vec!["desert".to_string()]);
                *dirty = true;
            }
        });

    // Show biome list for Only/Except
    match filter {
        BiomeFilter::Only(biomes) | BiomeFilter::Except(biomes) => {
            let mut biomes_text = biomes.join(", ");
            if ui.add(
                egui::TextEdit::singleline(&mut biomes_text)
                    .hint_text("biome1, biome2, ...")
                    .desired_width(150.0)
            ).changed() {
                *biomes = biomes_text
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                *dirty = true;
            }
        }
        BiomeFilter::All => {}
    }
}
