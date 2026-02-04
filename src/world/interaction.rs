//! Block Interaction System — Place & Break Blocks
//!
//! Provides player-driven block placement and destruction using the DDA
//! raycast system. The player can:
//! - **Break** blocks with left-click (replaces with Air)
//! - **Place** blocks with right-click (uses the currently selected block)
//! - **Cycle** the selected block via scroll wheel or number keys (1–9)
//!
//! Cross-chunk boundary updates are handled automatically: when a block at
//! the edge of a chunk is modified, the neighboring chunk is also marked
//! dirty so its mesh is rebuilt with correct face culling.

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;

use crate::actors::Player;
use crate::audio::{BlockSoundEvent, BlockSoundKind};
use crate::engine::controller::CursorState;
use crate::engine::raycast::CurrentTarget;

use super::{BlockType, Chunk, ChunkManager, ChunkMesh, CHUNK_SIZE};

// ============================================================================
// CONSTANTS
// ============================================================================

/// Ordered list of blocks the player can cycle through for placement.
const PLACEABLE_BLOCKS: &[BlockType] = &[
    BlockType::Stone,
    BlockType::Dirt,
    BlockType::Grass,
    BlockType::Sand,
    BlockType::Wood,
    BlockType::Leaves,
    BlockType::Sandstone,
    BlockType::Snow,
    BlockType::Ice,
    BlockType::Obsidian,
    BlockType::VolcanicRock,
    BlockType::Cactus,
    BlockType::SandDunes,
];

// ============================================================================
// RESOURCES
// ============================================================================

/// Tracks which block type the player will place on right-click.
#[derive(Resource)]
pub struct SelectedBlock {
    pub block_type: BlockType,
    /// Index into [`PLACEABLE_BLOCKS`] for scroll-wheel cycling.
    index: usize,
}

impl Default for SelectedBlock {
    fn default() -> Self {
        Self {
            block_type: BlockType::Stone,
            index: 0,
        }
    }
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that adds block interaction systems (break, place, cycle).
///
/// Systems run in [`Update`] and depend on:
/// - [`CurrentTarget`] from [`crate::engine::raycast::RaycastPlugin`]
/// - [`ChunkManager`] and [`Chunk`] from [`super::WorldPlugin`]
/// - [`Player`] marker for self-entombment prevention
pub struct BlockInteractionPlugin;

impl Plugin for BlockInteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SelectedBlock>().add_systems(
            Update,
            (break_block, place_block, cycle_selected_block)
                .after(crate::engine::raycast::update_raycast_target),
        );
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Convert a world-space block position to (chunk_pos, local_pos) using
/// Euclidean division so that negative coordinates are handled correctly.
fn world_to_chunk_and_local(world_pos: IVec3) -> (IVec3, IVec3) {
    let cs = CHUNK_SIZE as i32;
    let chunk_pos = IVec3::new(
        world_pos.x.div_euclid(cs),
        world_pos.y.div_euclid(cs),
        world_pos.z.div_euclid(cs),
    );
    let local = IVec3::new(
        world_pos.x.rem_euclid(cs),
        world_pos.y.rem_euclid(cs),
        world_pos.z.rem_euclid(cs),
    );
    (chunk_pos, local)
}

/// Mark neighboring chunks as dirty when a block at the edge of a chunk is
/// modified. This ensures cross-chunk face culling is recalculated.
fn dirty_neighbors_if_boundary(
    local: IVec3,
    chunk_pos: IVec3,
    chunk_manager: &ChunkManager,
    chunks: &mut Query<&mut Chunk, With<ChunkMesh>>,
) {
    let last = CHUNK_SIZE as i32 - 1;

    // For each axis, check if the local coordinate is at the boundary.
    let offsets: [(i32, IVec3); 6] = [
        (local.x, IVec3::new(-1, 0, 0)),     // x == 0
        (last - local.x, IVec3::new(1, 0, 0)), // x == 15
        (local.y, IVec3::new(0, -1, 0)),     // y == 0
        (last - local.y, IVec3::new(0, 1, 0)), // y == 15
        (local.z, IVec3::new(0, 0, -1)),     // z == 0
        (last - local.z, IVec3::new(0, 0, 1)), // z == 15
    ];

    for (coord_val, offset) in offsets {
        if coord_val == 0 {
            let neighbor_pos = chunk_pos + offset;
            if let Some(&entity) = chunk_manager.chunks.get(&neighbor_pos) {
                if let Ok(mut neighbor_chunk) = chunks.get_mut(entity) {
                    neighbor_chunk.dirty = true;
                }
            }
        }
    }
}

/// Returns `true` if the given world-space block position overlaps the
/// player's capsule (roughly 0.6 × 1.8 × 0.6 centered on feet).
fn overlaps_player(block_pos: IVec3, player_pos: Vec3) -> bool {
    // The block occupies [block_pos, block_pos + 1) on each axis.
    // The player capsule AABB: x ± 0.3, z ± 0.3, y in [feet, feet + 1.8].
    let bmin = block_pos.as_vec3();
    let bmax = bmin + Vec3::ONE;

    let pmin = Vec3::new(player_pos.x - 0.3, player_pos.y, player_pos.z - 0.3);
    let pmax = Vec3::new(player_pos.x + 0.3, player_pos.y + 1.8, player_pos.z + 0.3);

    // AABB vs AABB overlap test
    bmin.x < pmax.x
        && bmax.x > pmin.x
        && bmin.y < pmax.y
        && bmax.y > pmin.y
        && bmin.z < pmax.z
        && bmax.z > pmin.z
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// Break the block the player is looking at (left-click).
///
/// Replaces the targeted block with Air and marks the chunk (and any
/// boundary-adjacent neighbor) as dirty for re-meshing. Water blocks
/// cannot be broken. Emits a [`BlockSoundEvent`] on success.
fn break_block(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    current_target: Res<CurrentTarget>,
    chunk_manager: Res<ChunkManager>,
    cursor_state: Res<CursorState>,
    mut chunks: Query<&mut Chunk, With<ChunkMesh>>,
    mut sound_events: EventWriter<BlockSoundEvent>,
) {
    // Only interact when cursor is grabbed (FPS mode)
    if !cursor_state.grabbed {
        return;
    }

    if !mouse_buttons.just_pressed(MouseButton::Left) {
        return;
    }

    let Some(ref result) = current_target.0 else {
        return;
    };

    // Don't break water
    if result.block_type == BlockType::Water {
        return;
    }

    let broken_block_type = result.block_type;
    let block_world_pos = result.block_pos.as_vec3() + Vec3::splat(0.5);
    let (chunk_pos, local) = world_to_chunk_and_local(result.block_pos);

    let Some(&entity) = chunk_manager.chunks.get(&chunk_pos) else {
        return;
    };

    if let Ok(mut chunk) = chunks.get_mut(entity) {
        chunk.set_block(
            local.x as usize,
            local.y as usize,
            local.z as usize,
            BlockType::Air,
        );
        chunk.dirty = true;
        chunk.modified = true;

        // Emit sound event for the broken block
        sound_events.send(BlockSoundEvent {
            kind: BlockSoundKind::Break,
            position: block_world_pos,
            block_type: broken_block_type,
        });
    }

    dirty_neighbors_if_boundary(local, chunk_pos, &chunk_manager, &mut chunks);
}

/// Place a block at the adjacent position of the targeted face (right-click).
///
/// Uses the currently selected block type from [`SelectedBlock`]. Prevents
/// placement if the target position overlaps the player's capsule.
/// Emits a [`BlockSoundEvent`] on success.
#[allow(clippy::too_many_arguments)]
fn place_block(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    current_target: Res<CurrentTarget>,
    selected_block: Res<SelectedBlock>,
    chunk_manager: Res<ChunkManager>,
    cursor_state: Res<CursorState>,
    mut chunks: Query<&mut Chunk, With<ChunkMesh>>,
    player_query: Query<&GlobalTransform, With<Player>>,
    mut sound_events: EventWriter<BlockSoundEvent>,
) {
    if !cursor_state.grabbed {
        return;
    }

    if !mouse_buttons.just_pressed(MouseButton::Right) {
        return;
    }

    let Some(ref result) = current_target.0 else {
        return;
    };

    let place_pos = result.adjacent_pos;

    // Prevent self-entombment: check if placement overlaps the player
    if let Ok(player_global) = player_query.get_single() {
        if overlaps_player(place_pos, player_global.translation()) {
            return;
        }
    }

    let (chunk_pos, local) = world_to_chunk_and_local(place_pos);

    let Some(&entity) = chunk_manager.chunks.get(&chunk_pos) else {
        return;
    };

    if let Ok(mut chunk) = chunks.get_mut(entity) {
        chunk.set_block(
            local.x as usize,
            local.y as usize,
            local.z as usize,
            selected_block.block_type,
        );
        chunk.dirty = true;
        chunk.modified = true;

        // Emit sound event for the placed block
        sound_events.send(BlockSoundEvent {
            kind: BlockSoundKind::Place,
            position: place_pos.as_vec3() + Vec3::splat(0.5),
            block_type: selected_block.block_type,
        });
    }

    dirty_neighbors_if_boundary(local, chunk_pos, &chunk_manager, &mut chunks);
}

/// Cycle the selected block via scroll wheel or number keys 1–9.
fn cycle_selected_block(
    mut scroll_events: EventReader<MouseWheel>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut selected: ResMut<SelectedBlock>,
) {
    let len = PLACEABLE_BLOCKS.len();

    // Number keys 1–9 for direct selection
    let number_keys = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];

    for (i, &key) in number_keys.iter().enumerate() {
        if keyboard.just_pressed(key) && i < len {
            selected.index = i;
            selected.block_type = PLACEABLE_BLOCKS[i];
            return;
        }
    }

    // Scroll wheel cycling
    let mut scroll_delta: f32 = 0.0;
    for event in scroll_events.read() {
        scroll_delta += event.y;
    }

    if scroll_delta > 0.0 {
        // Scroll up → next block
        selected.index = (selected.index + 1) % len;
        selected.block_type = PLACEABLE_BLOCKS[selected.index];
    } else if scroll_delta < 0.0 {
        // Scroll down → previous block
        selected.index = (selected.index + len - 1) % len;
        selected.block_type = PLACEABLE_BLOCKS[selected.index];
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selected_block_default() {
        let sb = SelectedBlock::default();
        assert_eq!(sb.block_type, BlockType::Stone);
        assert_eq!(sb.index, 0);
    }

    #[test]
    fn test_placeable_blocks_non_empty() {
        assert!(!PLACEABLE_BLOCKS.is_empty());
        // No Air or Water in the list
        for &b in PLACEABLE_BLOCKS {
            assert_ne!(b, BlockType::Air);
            assert_ne!(b, BlockType::Water);
        }
    }

    #[test]
    fn test_world_to_chunk_and_local_positive() {
        let (chunk, local) = world_to_chunk_and_local(IVec3::new(35, 22, 50));
        assert_eq!(chunk, IVec3::new(2, 1, 3));
        assert_eq!(local, IVec3::new(3, 6, 2));
    }

    #[test]
    fn test_world_to_chunk_and_local_negative() {
        let (chunk, local) = world_to_chunk_and_local(IVec3::new(-1, -17, -16));
        assert_eq!(chunk, IVec3::new(-1, -2, -1));
        assert_eq!(local, IVec3::new(15, 15, 0));
    }

    #[test]
    fn test_world_to_chunk_and_local_origin() {
        let (chunk, local) = world_to_chunk_and_local(IVec3::ZERO);
        assert_eq!(chunk, IVec3::ZERO);
        assert_eq!(local, IVec3::ZERO);
    }

    #[test]
    fn test_world_to_chunk_and_local_boundary() {
        // Block at (16, 0, 0) should be chunk (1,0,0), local (0,0,0)
        let (chunk, local) = world_to_chunk_and_local(IVec3::new(16, 0, 0));
        assert_eq!(chunk, IVec3::new(1, 0, 0));
        assert_eq!(local, IVec3::new(0, 0, 0));
    }

    #[test]
    fn test_overlaps_player_directly_on() {
        // Player feet at (5.5, 10.0, 5.5), block at (5, 10, 5) should overlap
        assert!(overlaps_player(
            IVec3::new(5, 10, 5),
            Vec3::new(5.5, 10.0, 5.5)
        ));
    }

    #[test]
    fn test_overlaps_player_head_level() {
        // Player feet at (5.5, 10.0, 5.5), head goes to 11.8
        // Block at (5, 11, 5) should overlap (within head range)
        assert!(overlaps_player(
            IVec3::new(5, 11, 5),
            Vec3::new(5.5, 10.0, 5.5)
        ));
    }

    #[test]
    fn test_overlaps_player_above_head() {
        // Block at (5, 12, 5) is above player head (11.8), should NOT overlap
        assert!(!overlaps_player(
            IVec3::new(5, 12, 5),
            Vec3::new(5.5, 10.0, 5.5)
        ));
    }

    #[test]
    fn test_overlaps_player_far_away() {
        assert!(!overlaps_player(
            IVec3::new(100, 10, 100),
            Vec3::new(5.5, 10.0, 5.5)
        ));
    }

    #[test]
    fn test_overlaps_player_below_feet() {
        // Block at (5, 9, 5) is below feet at y=10.0, should NOT overlap
        assert!(!overlaps_player(
            IVec3::new(5, 9, 5),
            Vec3::new(5.5, 10.0, 5.5)
        ));
    }
}
