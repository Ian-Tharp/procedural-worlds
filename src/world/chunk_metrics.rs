//! Per-chunk memory profiling
//!
//! Tracks memory consumption broken down by chunk state (active, cached,
//! unloading). Provides sortable per-entity memory data for the performance
//! overlay's memory panel.

use bevy::prelude::*;
use std::collections::HashMap;

use super::{Chunk, ChunkMesh, PendingMesh, CHUNK_VOLUME};
use super::unloading::PendingSave;

// ============================================================================
// Chunk State
// ============================================================================

/// Logical state of a chunk for memory categorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChunkState {
    /// Fully loaded with mesh, in render range.
    Active,
    /// Loaded but pending mesh generation.
    Pending,
    /// Being saved/unloaded (has `PendingSave` component).
    Unloading,
}

impl std::fmt::Display for ChunkState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChunkState::Active => write!(f, "Active"),
            ChunkState::Pending => write!(f, "Pending"),
            ChunkState::Unloading => write!(f, "Unloading"),
        }
    }
}

// ============================================================================
// Memory Stats
// ============================================================================

/// Per-chunk memory entry for the largest-consumers list.
#[derive(Debug, Clone)]
pub struct ChunkMemoryEntry {
    /// Entity owning this chunk.
    pub entity: Entity,
    /// Chunk position.
    pub position: IVec3,
    /// Estimated memory in bytes.
    pub bytes: u64,
    /// Current state.
    pub state: ChunkState,
}

/// Aggregated per-chunk memory statistics.
///
/// Updated each frame by [`update_chunk_memory_stats`] when the memory
/// panel is visible.
#[derive(Resource, Default)]
pub struct ChunkMemoryStats {
    /// Total estimated memory across all loaded chunks.
    pub total_allocated: u64,
    /// Memory broken down by chunk state.
    pub per_state_breakdown: HashMap<ChunkState, u64>,
    /// Count of chunks per state.
    pub per_state_count: HashMap<ChunkState, usize>,
    /// Largest memory consumers, sorted descending by bytes.
    pub largest_consumers: Vec<ChunkMemoryEntry>,
    /// Whether the memory panel is visible (toggled by M key).
    pub panel_visible: bool,
    /// Optional state filter for the panel display.
    pub state_filter: Option<ChunkState>,
}

impl ChunkMemoryStats {
    /// Estimated bytes for a single chunk's block data.
    pub const BLOCK_DATA_BYTES: u64 =
        (CHUNK_VOLUME * std::mem::size_of::<super::BlockType>()) as u64;

    /// Rough estimate for mesh overhead per chunk (vertices, indices, normals, UVs).
    /// A typical chunk mesh is ~50-200 KB; we use 128 KB as a reasonable middle.
    pub const MESH_ESTIMATE_BYTES: u64 = 128 * 1024;

    /// Estimate memory for a single chunk based on whether it has a mesh.
    pub fn estimate_chunk_memory(has_mesh: bool) -> u64 {
        let mut total = Self::BLOCK_DATA_BYTES;
        if has_mesh {
            total += Self::MESH_ESTIMATE_BYTES;
        }
        total
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Toggle memory panel visibility with M key (only when F8 dashboard is open).
pub fn toggle_memory_panel(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut stats: ResMut<ChunkMemoryStats>,
    dashboard: Res<super::super::editor::performance::PerformanceDashboard>,
) {
    if dashboard.visible && keyboard.just_pressed(KeyCode::KeyM) {
        stats.panel_visible = !stats.panel_visible;
    }
}

/// Cycle state filter with N key when memory panel is open.
pub fn cycle_memory_filter(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut stats: ResMut<ChunkMemoryStats>,
) {
    if !stats.panel_visible {
        return;
    }
    if keyboard.just_pressed(KeyCode::KeyN) {
        stats.state_filter = match stats.state_filter {
            None => Some(ChunkState::Active),
            Some(ChunkState::Active) => Some(ChunkState::Pending),
            Some(ChunkState::Pending) => Some(ChunkState::Unloading),
            Some(ChunkState::Unloading) => None,
        };
    }
}

/// Query type alias for chunk memory profiling to reduce type complexity.
type ChunkMemoryQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static Chunk, Option<&'static ChunkMesh>, Option<&'static PendingMesh>, Option<&'static PendingSave>),
>;

/// Recompute chunk memory statistics each frame.
///
/// Only performs work when the memory panel is visible to avoid
/// unnecessary iteration over all chunks.
pub fn update_chunk_memory_stats(
    mut stats: ResMut<ChunkMemoryStats>,
    chunk_query: ChunkMemoryQuery,
) {
    if !stats.panel_visible {
        return;
    }

    stats.total_allocated = 0;
    stats.per_state_breakdown.clear();
    stats.per_state_count.clear();
    stats.largest_consumers.clear();

    for (entity, chunk, has_mesh, _has_pending_mesh, has_pending_save) in &chunk_query {
        let state = if has_pending_save.is_some() {
            ChunkState::Unloading
        } else if has_mesh.is_some() {
            ChunkState::Active
        } else {
            ChunkState::Pending
        };

        let has_rendered_mesh = has_mesh.is_some();
        let bytes = ChunkMemoryStats::estimate_chunk_memory(has_rendered_mesh);

        stats.total_allocated += bytes;
        *stats.per_state_breakdown.entry(state).or_insert(0) += bytes;
        *stats.per_state_count.entry(state).or_insert(0) += 1;

        stats.largest_consumers.push(ChunkMemoryEntry {
            entity,
            position: chunk.position,
            bytes,
            state,
        });
    }

    // Sort descending by memory
    stats
        .largest_consumers
        .sort_by(|a, b| b.bytes.cmp(&a.bytes));

    // Keep top 20
    stats.largest_consumers.truncate(20);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_state_display() {
        assert_eq!(format!("{}", ChunkState::Active), "Active");
        assert_eq!(format!("{}", ChunkState::Pending), "Pending");
        assert_eq!(format!("{}", ChunkState::Unloading), "Unloading");
    }

    #[test]
    fn test_estimate_chunk_memory_without_mesh() {
        let bytes = ChunkMemoryStats::estimate_chunk_memory(false);
        assert_eq!(bytes, ChunkMemoryStats::BLOCK_DATA_BYTES);
    }

    #[test]
    fn test_estimate_chunk_memory_with_mesh() {
        let bytes = ChunkMemoryStats::estimate_chunk_memory(true);
        assert_eq!(
            bytes,
            ChunkMemoryStats::BLOCK_DATA_BYTES + ChunkMemoryStats::MESH_ESTIMATE_BYTES
        );
    }

    #[test]
    fn test_block_data_bytes_correct() {
        // 4096 blocks * 2 bytes per BlockType (u16 repr)
        assert_eq!(
            ChunkMemoryStats::BLOCK_DATA_BYTES,
            (4096 * std::mem::size_of::<super::super::BlockType>()) as u64
        );
    }

    #[test]
    fn test_chunk_memory_stats_default() {
        let stats = ChunkMemoryStats::default();
        assert_eq!(stats.total_allocated, 0);
        assert!(stats.per_state_breakdown.is_empty());
        assert!(stats.largest_consumers.is_empty());
        assert!(!stats.panel_visible);
        assert!(stats.state_filter.is_none());
    }

    #[test]
    fn test_state_filter_cycle() {
        // Test the cycle logic manually
        let cycle = |current: Option<ChunkState>| -> Option<ChunkState> {
            match current {
                None => Some(ChunkState::Active),
                Some(ChunkState::Active) => Some(ChunkState::Pending),
                Some(ChunkState::Pending) => Some(ChunkState::Unloading),
                Some(ChunkState::Unloading) => None,
            }
        };

        assert_eq!(cycle(None), Some(ChunkState::Active));
        assert_eq!(cycle(Some(ChunkState::Active)), Some(ChunkState::Pending));
        assert_eq!(cycle(Some(ChunkState::Pending)), Some(ChunkState::Unloading));
        assert_eq!(cycle(Some(ChunkState::Unloading)), None);
    }
}
