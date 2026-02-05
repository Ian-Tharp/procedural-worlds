//! Chunk Debug Overlay
//!
//! Renders wireframe gizmo borders around chunks with color coding based on
//! their loading/mesh state. Essential for debugging the cross-chunk face
//! culling system and visualizing chunk streaming behaviour.
//!
//! ## Chunk States (Color Coded)
//!
//! | Color   | State     | Meaning                                          |
//! |---------|-----------|--------------------------------------------------|
//! | Red     | Unloaded  | Within load distance but not yet spawned          |
//! | Orange  | Loading   | Terrain being generated on background thread      |
//! | Yellow  | Meshing   | Chunk data ready, mesh being built                |
//! | Green   | Loaded    | Fully loaded with mesh                            |
//! | Cyan    | Optimized | All 6 face-neighbors present (cross-chunk culling)|
//! | Magenta | Error     | Chunk generation or loading failed                |
//! | Red(b)  | Player    | Chunk the player is currently standing in         |
//!
//! Toggle with **F4** key.

use bevy::prelude::*;
use bevy_egui::egui;

use crate::world::{
    chunk_to_world_pos, Chunk, ChunkManager, ChunkMesh, PendingChunk, PendingMesh, CHUNK_SIZE,
};

// ============================================================================
// Resources
// ============================================================================

/// Configuration and runtime state for the chunk debug overlay.
#[derive(Resource)]
pub struct ChunkDebugState {
    /// Master toggle — whether any chunk borders are drawn.
    pub visible: bool,
    /// Draw borders for fully loaded chunks (green).
    pub show_loaded: bool,
    /// Draw borders for chunks in loading/meshing states (orange/yellow).
    pub show_pending: bool,
    /// Draw borders for optimized chunks with all neighbors (cyan).
    pub show_optimized: bool,
    /// Draw borders for unloaded chunk positions within load distance (red).
    pub show_unloaded: bool,
    /// Draw borders for chunks that failed to load (magenta).
    pub show_errors: bool,
    /// Highlight the player's current chunk with a red border.
    pub highlight_player_chunk: bool,
    /// Show the loading state legend panel in the debug overlay.
    pub show_legend: bool,
}

impl Default for ChunkDebugState {
    fn default() -> Self {
        Self {
            visible: false,
            show_loaded: true,
            show_pending: true,
            show_optimized: true,
            show_unloaded: false,
            show_errors: true,
            highlight_player_chunk: true,
            show_legend: true,
        }
    }
}

// ============================================================================
// Chunk Visual State
// ============================================================================

/// The visual state of a chunk for debug rendering purposes.
///
/// Determined from ECS component presence on chunk entities and
/// bookkeeping in the [`ChunkManager`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChunkVisualState {
    /// Position is within load distance but no entity or task exists yet.
    Unloaded,
    /// Terrain is being generated on a background thread (`PendingChunk`).
    Loading,
    /// Chunk data loaded, mesh being built (`Chunk` + `PendingMesh`, or
    /// `Chunk` without `ChunkMesh`).
    Meshing,
    /// Fully loaded with mesh (`Chunk` + `ChunkMesh`).
    Loaded,
    /// Loaded and all 6 face-neighbors are also loaded — cross-chunk face
    /// culling is fully active for this chunk.
    Optimized,
    /// Chunk generation or loading encountered an error.
    Error,
}

impl ChunkVisualState {
    /// Get the gizmo color for this visual state.
    pub fn color(&self) -> Color {
        match self {
            Self::Unloaded => Color::srgba(1.0, 0.2, 0.2, 0.3),  // Red (dim)
            Self::Loading => Color::srgba(1.0, 0.6, 0.2, 0.6),   // Orange
            Self::Meshing => Color::srgba(1.0, 1.0, 0.2, 0.6),   // Yellow
            Self::Loaded => Color::srgba(0.2, 1.0, 0.2, 0.4),    // Green (subtle)
            Self::Optimized => Color::srgba(0.2, 0.8, 1.0, 0.5), // Cyan
            Self::Error => Color::srgba(1.0, 0.2, 1.0, 0.8),     // Magenta
        }
    }

    /// Human-readable label for this state.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Unloaded => "Unloaded",
            Self::Loading => "Loading",
            Self::Meshing => "Meshing",
            Self::Loaded => "Loaded",
            Self::Optimized => "Optimized",
            Self::Error => "Error",
        }
    }

    /// Emoji indicator for use in the legend UI.
    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Unloaded => "🔴",
            Self::Loading => "🟠",
            Self::Meshing => "🟡",
            Self::Loaded => "🟢",
            Self::Optimized => "🔵",
            Self::Error => "🟣",
        }
    }

    /// The egui color for the legend labels.
    pub fn egui_color(&self) -> egui::Color32 {
        match self {
            Self::Unloaded => egui::Color32::from_rgb(255, 80, 80),
            Self::Loading => egui::Color32::from_rgb(255, 160, 60),
            Self::Meshing => egui::Color32::from_rgb(255, 255, 80),
            Self::Loaded => egui::Color32::from_rgb(80, 255, 80),
            Self::Optimized => egui::Color32::from_rgb(80, 210, 255),
            Self::Error => egui::Color32::from_rgb(255, 80, 255),
        }
    }

    /// All variants in lifecycle order (for legend display).
    pub const ALL: [ChunkVisualState; 6] = [
        Self::Unloaded,
        Self::Loading,
        Self::Meshing,
        Self::Loaded,
        Self::Optimized,
        Self::Error,
    ];
}

/// Color used for the player chunk highlight border.
const PLAYER_CHUNK_COLOR: Color = Color::srgba(1.0, 0.2, 0.2, 0.8);

// ============================================================================
// Systems
// ============================================================================

/// Toggle the chunk debug overlay with F4.
pub fn chunk_debug_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut debug_state: ResMut<ChunkDebugState>,
) {
    if keyboard.just_pressed(KeyCode::F4) {
        debug_state.visible = !debug_state.visible;
        info!(
            "Chunk debug overlay: {}",
            if debug_state.visible { "ON" } else { "OFF" }
        );
    }
}

/// Returns `true` if all 6 face-neighbors of the chunk at `pos` are loaded.
///
/// This is a pure function over the `ChunkManager` data — no ECS queries needed.
pub fn has_all_neighbors(pos: IVec3, chunk_manager: &ChunkManager) -> bool {
    const OFFSETS: [IVec3; 6] = [
        IVec3::X,
        IVec3::NEG_X,
        IVec3::Y,
        IVec3::NEG_Y,
        IVec3::Z,
        IVec3::NEG_Z,
    ];
    OFFSETS
        .iter()
        .all(|&offset| chunk_manager.chunks.contains_key(&(pos + offset)))
}

/// Classify a chunk entity into a [`ChunkVisualState`] based on its ECS
/// components and neighbor data.
pub fn classify_chunk(
    has_mesh: bool,
    has_pending_mesh: bool,
    pos: IVec3,
    chunk_manager: &ChunkManager,
) -> ChunkVisualState {
    if has_pending_mesh {
        ChunkVisualState::Meshing
    } else if has_mesh {
        if has_all_neighbors(pos, chunk_manager) {
            ChunkVisualState::Optimized
        } else {
            ChunkVisualState::Loaded
        }
    } else {
        // Chunk data exists but no mesh yet and not currently meshing
        ChunkVisualState::Meshing
    }
}

/// Compute all chunk positions that should be loaded based on the current
/// player position and load distances.
///
/// Returns a `HashSet` of chunk-coordinate positions within the horizontal
/// load distance and vertical load range.
pub fn expected_chunk_positions(chunk_manager: &ChunkManager) -> std::collections::HashSet<IVec3> {
    let center = chunk_manager.player_chunk;
    let ld = chunk_manager.effective_load_distance();
    let vert_down = chunk_manager.vertical_load_down;
    let vert_up = chunk_manager.vertical_load_up;

    let mut positions = std::collections::HashSet::new();
    for x in (center.x - ld)..=(center.x + ld) {
        for z in (center.z - ld)..=(center.z + ld) {
            for y in (center.y - vert_down)..=(center.y + vert_up) {
                positions.insert(IVec3::new(x, y, z));
            }
        }
    }
    positions
}

/// Draw wireframe gizmo borders around all chunks, color-coded by state.
///
/// Runs every frame but early-returns when `ChunkDebugState::visible` is false.
pub fn draw_chunk_borders(
    debug_state: Res<ChunkDebugState>,
    chunk_manager: Res<ChunkManager>,
    mut gizmos: Gizmos,
    loaded_chunks: Query<(&Chunk, Option<&ChunkMesh>, Option<&PendingMesh>)>,
    pending_chunks: Query<&PendingChunk>,
) {
    if !debug_state.visible {
        return;
    }

    let half_size = CHUNK_SIZE as f32 / 2.0;
    let chunk_size_vec = Vec3::splat(CHUNK_SIZE as f32);

    // ── Unloaded chunks (within load distance but not spawned or pending) ──
    if debug_state.show_unloaded {
        let expected = expected_chunk_positions(&chunk_manager);
        for pos in &expected {
            // Skip positions that are loaded or currently pending
            if chunk_manager.chunks.contains_key(pos)
                || chunk_manager.pending.contains(pos)
                || chunk_manager.failed_chunks.contains_key(pos)
            {
                continue;
            }

            let world_pos = chunk_to_world_pos(*pos);
            let center = world_pos + Vec3::splat(half_size);
            let transform = Transform::from_translation(center).with_scale(chunk_size_vec);
            gizmos.cuboid(transform, ChunkVisualState::Unloaded.color());
        }
    }

    // ── Error chunks (failed to generate/load) ──
    if debug_state.show_errors {
        for &pos in chunk_manager.failed_chunks.keys() {
            let world_pos = chunk_to_world_pos(pos);
            let center = world_pos + Vec3::splat(half_size);
            let transform = Transform::from_translation(center).with_scale(chunk_size_vec);
            gizmos.cuboid(transform, ChunkVisualState::Error.color());
        }
    }

    // ── Pending chunks (terrain generation in progress) ──
    if debug_state.show_pending {
        for pending in &pending_chunks {
            let world_pos = chunk_to_world_pos(pending.position);
            let center = world_pos + Vec3::splat(half_size);
            let transform = Transform::from_translation(center).with_scale(chunk_size_vec);
            gizmos.cuboid(transform, ChunkVisualState::Loading.color());
        }
    }

    // ── Loaded chunks (various mesh states) ──
    for (chunk, mesh_marker, pending_mesh) in &loaded_chunks {
        let pos = chunk.position;

        let state = classify_chunk(
            mesh_marker.is_some(),
            pending_mesh.is_some(),
            pos,
            &chunk_manager,
        );

        // Apply filter settings
        match state {
            ChunkVisualState::Loaded if !debug_state.show_loaded => continue,
            ChunkVisualState::Optimized if !debug_state.show_optimized => continue,
            ChunkVisualState::Meshing if !debug_state.show_pending => continue,
            _ => {}
        }

        let world_pos = chunk_to_world_pos(pos);
        let center = world_pos + Vec3::splat(half_size);
        let transform = Transform::from_translation(center).with_scale(chunk_size_vec);
        gizmos.cuboid(transform, state.color());
    }

    // ── Player's current chunk (red highlight, slightly larger to stand out) ──
    if debug_state.highlight_player_chunk {
        let player_world_pos = chunk_to_world_pos(chunk_manager.player_chunk);
        let center = player_world_pos + Vec3::splat(half_size);
        // Scale up 1% to ensure it renders on top of the state border
        let transform =
            Transform::from_translation(center).with_scale(chunk_size_vec * 1.01);
        gizmos.cuboid(transform, PLAYER_CHUNK_COLOR);
    }
}

/// Draw the chunk loading state legend UI into an egui collapsing header.
///
/// Called from the inspector panel when the chunk debug overlay is visible.
pub fn draw_chunk_state_legend(ui: &mut egui::Ui, debug_state: &mut ChunkDebugState) {
    if !debug_state.show_legend {
        return;
    }

    ui.collapsing("🗺 Chunk Border Legend", |ui| {
        for state in &ChunkVisualState::ALL {
            ui.horizontal(|ui| {
                ui.label(state.emoji());
                ui.colored_label(state.egui_color(), state.label());
            });
        }

        ui.separator();
        ui.label(egui::RichText::new("Filters:").strong().size(12.0));
        ui.checkbox(&mut debug_state.show_unloaded, "Unloaded (red)");
        ui.checkbox(&mut debug_state.show_pending, "Loading/Meshing");
        ui.checkbox(&mut debug_state.show_loaded, "Loaded (green)");
        ui.checkbox(&mut debug_state.show_optimized, "Optimized (cyan)");
        ui.checkbox(&mut debug_state.show_errors, "Errors (magenta)");
        ui.checkbox(&mut debug_state.highlight_player_chunk, "Player chunk");
    });
}

// ============================================================================
// Plugin
// ============================================================================

/// Plugin for chunk border debug visualization.
///
/// Adds `ChunkDebugState` resource and gizmo drawing systems.
/// Toggle with F4 at runtime.
pub struct ChunkDebugPlugin;

impl Plugin for ChunkDebugPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChunkDebugState>().add_systems(
            Update,
            (chunk_debug_input, draw_chunk_borders).chain(),
        );
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a `ChunkManager` with chunks loaded at the given positions.
    fn manager_with_chunks(positions: &[IVec3]) -> ChunkManager {
        let mut cm = ChunkManager::default();
        for &pos in positions {
            cm.chunks.insert(pos, Entity::PLACEHOLDER);
        }
        cm
    }

    // ── ChunkDebugState defaults ──

    #[test]
    fn test_default_state_is_hidden() {
        let state = ChunkDebugState::default();
        assert!(!state.visible, "Chunk debug overlay should start hidden");
    }

    #[test]
    fn test_default_state_filters_enabled() {
        let state = ChunkDebugState::default();
        assert!(state.show_loaded);
        assert!(state.show_pending);
        assert!(state.show_optimized);
        assert!(state.show_errors);
        assert!(state.highlight_player_chunk);
        assert!(state.show_legend);
    }

    #[test]
    fn test_default_state_unloaded_off() {
        // Unloaded borders default to off since they can be noisy
        let state = ChunkDebugState::default();
        assert!(!state.show_unloaded);
    }

    // ── ChunkVisualState colors ──

    #[test]
    fn test_visual_state_colors_are_distinct() {
        // Each state should produce a distinct color
        for (i, a) in ChunkVisualState::ALL.iter().enumerate() {
            for (j, b) in ChunkVisualState::ALL.iter().enumerate() {
                if i != j {
                    assert_ne!(
                        a.color(),
                        b.color(),
                        "{:?} and {:?} should have different colors",
                        a,
                        b
                    );
                }
            }
        }
    }

    #[test]
    fn test_visual_state_labels() {
        assert_eq!(ChunkVisualState::Unloaded.label(), "Unloaded");
        assert_eq!(ChunkVisualState::Loading.label(), "Loading");
        assert_eq!(ChunkVisualState::Meshing.label(), "Meshing");
        assert_eq!(ChunkVisualState::Loaded.label(), "Loaded");
        assert_eq!(ChunkVisualState::Optimized.label(), "Optimized");
        assert_eq!(ChunkVisualState::Error.label(), "Error");
    }

    #[test]
    fn test_visual_state_emojis_all_unique() {
        let emojis: Vec<&str> = ChunkVisualState::ALL.iter().map(|s| s.emoji()).collect();
        let unique: std::collections::HashSet<&str> = emojis.iter().copied().collect();
        assert_eq!(emojis.len(), unique.len(), "All state emojis should be unique");
    }

    #[test]
    fn test_visual_state_all_count() {
        assert_eq!(ChunkVisualState::ALL.len(), 6);
    }

    #[test]
    fn test_player_chunk_color_is_red() {
        // Player chunk highlight should be red-ish (R > 0.5, G < 0.5)
        let Color::Srgba(srgba) = PLAYER_CHUNK_COLOR else {
            panic!("Expected Srgba color variant");
        };
        assert!(srgba.red > 0.5, "Red component should be dominant");
        assert!(srgba.green < 0.5, "Green should be low for red color");
    }

    #[test]
    fn test_error_state_is_magenta() {
        let Color::Srgba(srgba) = ChunkVisualState::Error.color() else {
            panic!("Expected Srgba color variant");
        };
        assert!(srgba.red > 0.5, "Magenta should have high red");
        assert!(srgba.blue > 0.5, "Magenta should have high blue");
        assert!(srgba.green < 0.5, "Magenta should have low green");
    }

    #[test]
    fn test_unloaded_state_is_reddish() {
        let Color::Srgba(srgba) = ChunkVisualState::Unloaded.color() else {
            panic!("Expected Srgba color variant");
        };
        assert!(srgba.red > 0.5, "Unloaded should have high red");
        assert!(srgba.green < 0.5, "Unloaded should have low green");
    }

    // ── has_all_neighbors ──

    #[test]
    fn test_has_all_neighbors_true() {
        let center = IVec3::ZERO;
        let positions = vec![
            center,
            IVec3::X,
            IVec3::NEG_X,
            IVec3::Y,
            IVec3::NEG_Y,
            IVec3::Z,
            IVec3::NEG_Z,
        ];
        let cm = manager_with_chunks(&positions);
        assert!(has_all_neighbors(center, &cm));
    }

    #[test]
    fn test_has_all_neighbors_missing_one() {
        let center = IVec3::ZERO;
        // Missing NEG_Z neighbor
        let positions = vec![
            center,
            IVec3::X,
            IVec3::NEG_X,
            IVec3::Y,
            IVec3::NEG_Y,
            IVec3::Z,
        ];
        let cm = manager_with_chunks(&positions);
        assert!(!has_all_neighbors(center, &cm));
    }

    #[test]
    fn test_has_all_neighbors_isolated() {
        let center = IVec3::new(5, 5, 5);
        let cm = manager_with_chunks(&[center]);
        assert!(!has_all_neighbors(center, &cm));
    }

    #[test]
    fn test_has_all_neighbors_empty_manager() {
        let cm = ChunkManager::default();
        assert!(!has_all_neighbors(IVec3::ZERO, &cm));
    }

    #[test]
    fn test_has_all_neighbors_negative_coords() {
        let center = IVec3::new(-3, -1, -5);
        let positions = vec![
            center,
            center + IVec3::X,
            center + IVec3::NEG_X,
            center + IVec3::Y,
            center + IVec3::NEG_Y,
            center + IVec3::Z,
            center + IVec3::NEG_Z,
        ];
        let cm = manager_with_chunks(&positions);
        assert!(has_all_neighbors(center, &cm));
    }

    // ── classify_chunk ──

    #[test]
    fn test_classify_pending_mesh() {
        let cm = ChunkManager::default();
        let state = classify_chunk(false, true, IVec3::ZERO, &cm);
        assert_eq!(state, ChunkVisualState::Meshing);
    }

    #[test]
    fn test_classify_loaded_no_neighbors() {
        let cm = manager_with_chunks(&[IVec3::ZERO]);
        let state = classify_chunk(true, false, IVec3::ZERO, &cm);
        assert_eq!(state, ChunkVisualState::Loaded);
    }

    #[test]
    fn test_classify_optimized_all_neighbors() {
        let center = IVec3::ZERO;
        let positions = vec![
            center,
            IVec3::X,
            IVec3::NEG_X,
            IVec3::Y,
            IVec3::NEG_Y,
            IVec3::Z,
            IVec3::NEG_Z,
        ];
        let cm = manager_with_chunks(&positions);
        let state = classify_chunk(true, false, center, &cm);
        assert_eq!(state, ChunkVisualState::Optimized);
    }

    #[test]
    fn test_classify_no_mesh_no_pending() {
        let cm = ChunkManager::default();
        // Chunk exists in ECS but has neither mesh nor pending mesh
        let state = classify_chunk(false, false, IVec3::ZERO, &cm);
        assert_eq!(state, ChunkVisualState::Meshing);
    }

    #[test]
    fn test_classify_pending_mesh_overrides_mesh() {
        // Even if mesh marker is present, pending mesh means we're rebuilding
        let cm = ChunkManager::default();
        let state = classify_chunk(true, true, IVec3::ZERO, &cm);
        assert_eq!(state, ChunkVisualState::Meshing);
    }

    // ── expected_chunk_positions ──

    #[test]
    fn test_expected_positions_count() {
        let mut cm = ChunkManager::default();
        cm.render_distance = 2;
        cm.load_distance = None;
        cm.vertical_load_up = 1;
        cm.vertical_load_down = 1;
        cm.player_chunk = IVec3::ZERO;

        let positions = expected_chunk_positions(&cm);
        // (2*2+1)^2 horizontal * (1+1+1) vertical = 25 * 3 = 75
        assert_eq!(positions.len(), 75);
    }

    #[test]
    fn test_expected_positions_contains_player_chunk() {
        let cm = ChunkManager::default();
        let positions = expected_chunk_positions(&cm);
        assert!(
            positions.contains(&cm.player_chunk),
            "Expected positions should include the player's chunk"
        );
    }

    #[test]
    fn test_expected_positions_respects_load_distance() {
        let mut cm = ChunkManager::default();
        cm.render_distance = 4;
        cm.load_distance = Some(2);
        cm.vertical_load_up = 0;
        cm.vertical_load_down = 0;
        cm.player_chunk = IVec3::ZERO;

        let positions = expected_chunk_positions(&cm);
        // load_distance=2 is used, not render_distance=4
        // (2*2+1)^2 * 1 = 25
        assert_eq!(positions.len(), 25);

        // Position at distance 3 should NOT be included
        assert!(!positions.contains(&IVec3::new(3, 0, 0)));
        // Position at distance 2 should be included
        assert!(positions.contains(&IVec3::new(2, 0, 0)));
    }

    #[test]
    fn test_expected_positions_with_offset_player() {
        let mut cm = ChunkManager::default();
        cm.render_distance = 1;
        cm.load_distance = None;
        cm.vertical_load_up = 0;
        cm.vertical_load_down = 0;
        cm.player_chunk = IVec3::new(10, 5, -3);

        let positions = expected_chunk_positions(&cm);
        // (2*1+1)^2 * 1 = 9
        assert_eq!(positions.len(), 9);
        assert!(positions.contains(&IVec3::new(10, 5, -3)));
        assert!(positions.contains(&IVec3::new(11, 5, -3)));
        assert!(positions.contains(&IVec3::new(9, 5, -4)));
    }

    // ── failed_chunks tracking ──

    #[test]
    fn test_failed_chunks_default_empty() {
        let cm = ChunkManager::default();
        assert!(cm.failed_chunks.is_empty());
    }

    #[test]
    fn test_failed_chunks_tracks_errors() {
        let mut cm = ChunkManager::default();
        let pos = IVec3::new(1, 0, 1);
        cm.failed_chunks.insert(pos, "generation panic".to_string());

        assert!(cm.failed_chunks.contains_key(&pos));
        assert_eq!(cm.failed_chunks[&pos], "generation panic");
    }

    #[test]
    fn test_failed_chunks_excluded_from_unloaded() {
        // Verifies that expected_chunk_positions minus loaded/pending/failed
        // correctly excludes failed chunks when determining unloaded set.
        let mut cm = ChunkManager::default();
        cm.render_distance = 1;
        cm.load_distance = None;
        cm.vertical_load_up = 0;
        cm.vertical_load_down = 0;
        cm.player_chunk = IVec3::ZERO;

        let failed_pos = IVec3::new(1, 0, 0);
        cm.failed_chunks.insert(failed_pos, "test error".to_string());

        let expected = expected_chunk_positions(&cm);
        assert!(expected.contains(&failed_pos));

        // Simulate the draw_chunk_borders filtering logic
        let unloaded: Vec<IVec3> = expected
            .iter()
            .filter(|pos| {
                !cm.chunks.contains_key(pos)
                    && !cm.pending.contains(pos)
                    && !cm.failed_chunks.contains_key(pos)
            })
            .copied()
            .collect();

        assert!(
            !unloaded.contains(&failed_pos),
            "Failed chunk should not appear in unloaded set"
        );
    }
}
