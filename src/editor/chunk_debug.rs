//! Chunk Debug Overlay
//!
//! Renders wireframe gizmo borders around chunks with color coding based on
//! their loading/mesh state. Essential for debugging the cross-chunk face
//! culling system and visualizing chunk streaming behaviour.
//!
//! ## Chunk States (Color Coded)
//!
//! | Color  | State     | Meaning                                          |
//! |--------|-----------|--------------------------------------------------|
//! | Orange | Loading   | Terrain being generated on background thread      |
//! | Yellow | Meshing   | Chunk data ready, mesh being built                |
//! | Green  | Loaded    | Fully loaded with mesh                            |
//! | Cyan   | Optimized | All 6 face-neighbors present (cross-chunk culling)|
//! | Red    | Player    | Chunk the player is currently standing in         |
//!
//! Toggle with **F4** key.

use bevy::prelude::*;

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
    /// Highlight the player's current chunk with a red border.
    pub highlight_player_chunk: bool,
}

impl Default for ChunkDebugState {
    fn default() -> Self {
        Self {
            visible: false,
            show_loaded: true,
            show_pending: true,
            show_optimized: true,
            highlight_player_chunk: true,
        }
    }
}

// ============================================================================
// Chunk Visual State
// ============================================================================

/// The visual state of a chunk for debug rendering purposes.
///
/// Determined from ECS component presence on chunk entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChunkVisualState {
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
}

impl ChunkVisualState {
    /// Get the gizmo color for this visual state.
    pub fn color(&self) -> Color {
        match self {
            Self::Loading => Color::srgba(1.0, 0.6, 0.2, 0.6),   // Orange
            Self::Meshing => Color::srgba(1.0, 1.0, 0.2, 0.6),   // Yellow
            Self::Loaded => Color::srgba(0.2, 1.0, 0.2, 0.4),    // Green (subtle)
            Self::Optimized => Color::srgba(0.2, 0.8, 1.0, 0.5), // Cyan
        }
    }

    /// Human-readable label for this state.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Loading => "Loading",
            Self::Meshing => "Meshing",
            Self::Loaded => "Loaded",
            Self::Optimized => "Optimized",
        }
    }
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
        assert!(state.highlight_player_chunk);
    }

    // ── ChunkVisualState colors ──

    #[test]
    fn test_visual_state_colors_are_distinct() {
        let states = [
            ChunkVisualState::Loading,
            ChunkVisualState::Meshing,
            ChunkVisualState::Loaded,
            ChunkVisualState::Optimized,
        ];

        // Each state should produce a distinct color
        for (i, a) in states.iter().enumerate() {
            for (j, b) in states.iter().enumerate() {
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
        assert_eq!(ChunkVisualState::Loading.label(), "Loading");
        assert_eq!(ChunkVisualState::Meshing.label(), "Meshing");
        assert_eq!(ChunkVisualState::Loaded.label(), "Loaded");
        assert_eq!(ChunkVisualState::Optimized.label(), "Optimized");
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
}
