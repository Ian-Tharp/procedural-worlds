//! Water flow simulation and swimming physics
//!
//! Implements a cellular automata water flow system where:
//! - Source blocks (level 7) spread horizontally, decreasing by 1 per step
//! - Water flows downward infinitely at full level (7)
//! - Flow simulation runs at 4 updates/sec with max 64 block updates per tick
//! - Swimming physics modify gravity, speed, and add buoyancy

use std::collections::{HashMap, HashSet, VecDeque};

use bevy::prelude::*;

use crate::world::{BlockType, Chunk, ChunkManager, CHUNK_SIZE};

/// Maximum water level (source block)
pub const MAX_WATER_LEVEL: u8 = 7;

/// Maximum horizontal flow distance from source
pub const MAX_FLOW_DISTANCE: u8 = 7;

/// Maximum block updates processed per simulation tick
pub const MAX_UPDATES_PER_TICK: usize = 64;

/// Water simulation tick rate (updates per second)
pub const WATER_TICK_RATE: f32 = 4.0;

/// Resource storing water levels for all water blocks in the world.
///
/// Maps world-space block coordinates to water level (1-7).
/// Level 0 means no water (air) and is not stored.
#[derive(Resource, Default, Debug)]
pub struct WaterLevelMap {
    /// World coordinate -> water level (1-7)
    pub levels: HashMap<IVec3, u8>,
}

impl WaterLevelMap {
    /// Get the water level at a world position. Returns 0 if no water.
    pub fn get_level(&self, pos: IVec3) -> u8 {
        self.levels.get(&pos).copied().unwrap_or(0)
    }

    /// Set the water level at a world position. Removes entry if level is 0.
    pub fn set_level(&mut self, pos: IVec3, level: u8) {
        if level == 0 {
            self.levels.remove(&pos);
        } else {
            self.levels.insert(pos, level.min(MAX_WATER_LEVEL));
        }
    }

    /// Remove a water block entry entirely.
    pub fn remove(&mut self, pos: &IVec3) {
        self.levels.remove(pos);
    }
}

/// Resource that queues positions needing flow recalculation.
#[derive(Resource, Default, Debug)]
pub struct WaterFlowQueue {
    /// Positions that need flow update
    pub pending: VecDeque<IVec3>,
    /// Set for O(1) duplicate check
    pub pending_set: HashSet<IVec3>,
}

impl WaterFlowQueue {
    /// Schedule a position for flow recalculation.
    pub fn schedule(&mut self, pos: IVec3) {
        if self.pending_set.insert(pos) {
            self.pending.push_back(pos);
        }
    }

    /// Schedule a position and all 6 face-adjacent neighbors.
    pub fn schedule_with_neighbors(&mut self, pos: IVec3) {
        self.schedule(pos);
        for offset in NEIGHBOR_OFFSETS {
            self.schedule(pos + offset);
        }
    }
}

/// Timer resource for water simulation tick rate.
#[derive(Resource)]
pub struct WaterSimTimer {
    /// Timer that fires at WATER_TICK_RATE
    pub timer: Timer,
}

impl Default for WaterSimTimer {
    fn default() -> Self {
        Self {
            timer: Timer::from_seconds(1.0 / WATER_TICK_RATE, TimerMode::Repeating),
        }
    }
}

/// Marker component added to the Player entity when submerged in water.
#[derive(Component, Debug)]
pub struct Swimming;

/// The 6 face-adjacent neighbor offsets.
const NEIGHBOR_OFFSETS: [IVec3; 6] = [
    IVec3::X, IVec3::NEG_X,
    IVec3::Y, IVec3::NEG_Y,
    IVec3::Z, IVec3::NEG_Z,
];

/// Horizontal neighbor offsets (4 directions).
const HORIZONTAL_OFFSETS: [IVec3; 4] = [
    IVec3::X, IVec3::NEG_X,
    IVec3::Z, IVec3::NEG_Z,
];

/// Plugin for water flow simulation and swimming.
pub struct WaterPlugin;

impl Plugin for WaterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WaterLevelMap>()
            .init_resource::<WaterFlowQueue>()
            .init_resource::<WaterSimTimer>()
            .add_systems(
                Update,
                (
                    initialize_water_levels_for_new_chunks,
                    water_flow_simulation,
                    detect_swimming,
                )
                    .chain(),
            );
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Convert world position to chunk position and local coordinates.
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

/// Set a block in the world and mark the chunk dirty.
fn set_block_at(
    pos: IVec3,
    block: BlockType,
    chunk_manager: &ChunkManager,
    chunks: &mut Query<&mut Chunk>,
) -> bool {
    let (chunk_pos, local) = world_to_chunk_and_local(pos);
    if let Some(&entity) = chunk_manager.chunks.get(&chunk_pos) {
        if let Ok(mut chunk) = chunks.get_mut(entity) {
            chunk.set_block(local.x as usize, local.y as usize, local.z as usize, block);
            chunk.dirty = true;
            return true;
        }
    }
    false
}

/// Mark the chunk containing a world position as dirty for remeshing.
fn mark_chunk_dirty(pos: IVec3, chunk_manager: &ChunkManager, chunks: &mut Query<&mut Chunk>) {
    let (chunk_pos, _) = world_to_chunk_and_local(pos);
    if let Some(&entity) = chunk_manager.chunks.get(&chunk_pos) {
        if let Ok(mut chunk) = chunks.get_mut(entity) {
            chunk.dirty = true;
        }
    }
}

/// Calculate the water height for rendering purposes.
///
/// Source blocks (level 7) render at 14/16 height (like Minecraft).
/// Lower levels render proportionally: `level / 7.0`.
pub fn water_render_height(level: u8) -> f32 {
    if level >= MAX_WATER_LEVEL {
        14.0 / 16.0 // 0.875 - slightly below full block
    } else if level > 0 {
        level as f32 / MAX_WATER_LEVEL as f32
    } else {
        0.0
    }
}

/// Check if a world position is submerged in water.
///
/// Returns true if the position is inside a water block and the Y coordinate
/// within the block is below the water surface level.
pub fn is_submerged(pos: Vec3, water_levels: &WaterLevelMap) -> bool {
    let block_pos = IVec3::new(
        pos.x.floor() as i32,
        pos.y.floor() as i32,
        pos.z.floor() as i32,
    );
    let level = water_levels.get_level(block_pos);
    if level == 0 {
        return false;
    }
    let height = water_render_height(level);
    let local_y = pos.y - block_pos.y as f32;
    local_y < height
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// Initialize water levels for newly loaded chunks.
///
/// Scans new chunks for Water blocks and assigns them level 7 (source)
/// in the WaterLevelMap if not already present.
fn initialize_water_levels_for_new_chunks(
    mut water_levels: ResMut<WaterLevelMap>,
    chunks: Query<&Chunk, Changed<Chunk>>,
) {
    for chunk in chunks.iter() {
        let world_pos = chunk.world_position();
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    if chunk.get_block(x, y, z) == BlockType::Water {
                        let wp = world_pos + IVec3::new(x as i32, y as i32, z as i32);
                        if water_levels.get_level(wp) == 0 {
                            water_levels.set_level(wp, MAX_WATER_LEVEL);
                        }
                    }
                }
            }
        }
    }
}

/// Run water flow simulation at fixed tick rate.
///
/// Processes up to MAX_UPDATES_PER_TICK pending flow updates per tick.
/// Water spreads horizontally with decreasing level and flows down at full level.
fn water_flow_simulation(
    time: Res<Time>,
    mut sim_timer: ResMut<WaterSimTimer>,
    mut water_levels: ResMut<WaterLevelMap>,
    mut flow_queue: ResMut<WaterFlowQueue>,
    chunk_manager: Res<ChunkManager>,
    mut chunks: Query<&mut Chunk>,
) {
    sim_timer.timer.tick(time.delta());
    if !sim_timer.timer.just_finished() {
        return;
    }

    // Collect positions to process this tick
    let mut to_process = Vec::with_capacity(MAX_UPDATES_PER_TICK);
    for _ in 0..MAX_UPDATES_PER_TICK {
        if let Some(pos) = flow_queue.pending.pop_front() {
            flow_queue.pending_set.remove(&pos);
            to_process.push(pos);
        } else {
            break;
        }
    }

    // Cache chunk data for read-only block queries
    let chunks_readonly: HashMap<IVec3, Vec<BlockType>> = {
        let mut map = HashMap::new();
        for (&chunk_pos, &entity) in chunk_manager.chunks.iter() {
            if let Ok(chunk) = chunks.get(entity) {
                map.insert(chunk_pos, chunk.blocks().to_vec());
            }
        }
        map
    };

    let is_solid_cached = |pos: IVec3| -> bool {
        let cs = CHUNK_SIZE as i32;
        let cp = IVec3::new(pos.x.div_euclid(cs), pos.y.div_euclid(cs), pos.z.div_euclid(cs));
        let lc = IVec3::new(pos.x.rem_euclid(cs), pos.y.rem_euclid(cs), pos.z.rem_euclid(cs));
        if let Some(data) = chunks_readonly.get(&cp) {
            let idx = lc.x as usize + lc.y as usize * CHUNK_SIZE + lc.z as usize * CHUNK_SIZE * CHUNK_SIZE;
            BlockType::from(data[idx] as u16).is_solid()
        } else {
            false
        }
    };

    let get_block_cached = |pos: IVec3| -> BlockType {
        let cs = CHUNK_SIZE as i32;
        let cp = IVec3::new(pos.x.div_euclid(cs), pos.y.div_euclid(cs), pos.z.div_euclid(cs));
        let lc = IVec3::new(pos.x.rem_euclid(cs), pos.y.rem_euclid(cs), pos.z.rem_euclid(cs));
        if let Some(data) = chunks_readonly.get(&cp) {
            let idx = lc.x as usize + lc.y as usize * CHUNK_SIZE + lc.z as usize * CHUNK_SIZE * CHUNK_SIZE;
            BlockType::from(data[idx] as u16)
        } else {
            BlockType::Air
        }
    };

    let mut new_levels: Vec<(IVec3, u8)> = Vec::new();
    let mut blocks_to_set: Vec<(IVec3, BlockType)> = Vec::new();

    for pos in &to_process {
        let pos = *pos;
        let current_level = water_levels.get_level(pos);
        let current_block = get_block_cached(pos);

        if current_block.is_solid() {
            continue;
        }

        if current_level > 0 {
            // Flow downward: block below gets level 7
            let below = pos + IVec3::NEG_Y;
            if !is_solid_cached(below) {
                let below_block = get_block_cached(below);
                if water_levels.get_level(below) < MAX_WATER_LEVEL
                    && (below_block == BlockType::Air || below_block == BlockType::Water)
                {
                    new_levels.push((below, MAX_WATER_LEVEL));
                    if below_block == BlockType::Air {
                        blocks_to_set.push((below, BlockType::Water));
                    }
                    flow_queue.schedule(below);
                }
            }

            // Spread horizontally with level - 1
            if current_level > 1 {
                for offset in HORIZONTAL_OFFSETS {
                    let neighbor = pos + offset;
                    if !is_solid_cached(neighbor) {
                        let neighbor_block = get_block_cached(neighbor);
                        let neighbor_level = water_levels.get_level(neighbor);
                        let new_level = current_level - 1;
                        if neighbor_level < new_level
                            && (neighbor_block == BlockType::Air || neighbor_block == BlockType::Water)
                        {
                            new_levels.push((neighbor, new_level));
                            if neighbor_block == BlockType::Air {
                                blocks_to_set.push((neighbor, BlockType::Water));
                            }
                            flow_queue.schedule(neighbor);
                        }
                    }
                }
            }

            // Check if non-source water should recede
            if current_level < MAX_WATER_LEVEL {
                let above_level = water_levels.get_level(pos + IVec3::Y);
                let mut max_neighbor_level = 0u8;
                for offset in HORIZONTAL_OFFSETS {
                    let nl = water_levels.get_level(pos + offset);
                    max_neighbor_level = max_neighbor_level.max(nl);
                }

                let should_have = if above_level > 0 {
                    MAX_WATER_LEVEL
                } else if max_neighbor_level > 1 {
                    max_neighbor_level - 1
                } else {
                    0
                };

                if should_have < current_level {
                    new_levels.push((pos, should_have));
                    if should_have == 0 {
                        blocks_to_set.push((pos, BlockType::Air));
                    }
                    for offset in HORIZONTAL_OFFSETS {
                        flow_queue.schedule(pos + offset);
                    }
                    flow_queue.schedule(pos + IVec3::NEG_Y);
                }
            }
        }
    }

    // Apply water level changes
    for (pos, level) in new_levels {
        water_levels.set_level(pos, level);
        mark_chunk_dirty(pos, &chunk_manager, &mut chunks);
    }

    // Apply block changes
    for (pos, block) in blocks_to_set {
        set_block_at(pos, block, &chunk_manager, &mut chunks);
    }
}

/// Detect if the player is swimming and add/remove the Swimming marker.
fn detect_swimming(
    mut commands: Commands,
    water_levels: Res<WaterLevelMap>,
    player_query: Query<(Entity, &Transform), With<crate::actors::Player>>,
    swimming_query: Query<Entity, (With<crate::actors::Player>, With<Swimming>)>,
) {
    for (entity, transform) in player_query.iter() {
        let feet_pos = transform.translation;
        let submerged = is_submerged(feet_pos, &water_levels);

        if submerged {
            if swimming_query.get(entity).is_err() {
                commands.entity(entity).insert(Swimming);
            }
        } else if swimming_query.get(entity).is_ok() {
            commands.entity(entity).remove::<Swimming>();
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_water_level_map_default() {
        let map = WaterLevelMap::default();
        assert_eq!(map.get_level(IVec3::ZERO), 0);
    }

    #[test]
    fn test_water_level_map_set_get() {
        let mut map = WaterLevelMap::default();
        map.set_level(IVec3::new(1, 2, 3), 5);
        assert_eq!(map.get_level(IVec3::new(1, 2, 3)), 5);
    }

    #[test]
    fn test_water_level_map_set_zero_removes() {
        let mut map = WaterLevelMap::default();
        map.set_level(IVec3::ZERO, 7);
        assert_eq!(map.get_level(IVec3::ZERO), 7);
        map.set_level(IVec3::ZERO, 0);
        assert_eq!(map.get_level(IVec3::ZERO), 0);
        assert!(!map.levels.contains_key(&IVec3::ZERO));
    }

    #[test]
    fn test_water_level_map_clamps_to_max() {
        let mut map = WaterLevelMap::default();
        map.set_level(IVec3::ZERO, 10);
        assert_eq!(map.get_level(IVec3::ZERO), MAX_WATER_LEVEL);
    }

    #[test]
    fn test_water_render_height_source() {
        let h = water_render_height(MAX_WATER_LEVEL);
        assert!((h - 0.875).abs() < 0.001, "Source blocks should render at 14/16 height");
    }

    #[test]
    fn test_water_render_height_partial() {
        let h = water_render_height(4);
        let expected = 4.0 / 7.0;
        assert!((h - expected).abs() < 0.001, "Level 4 height should be 4/7");
    }

    #[test]
    fn test_water_render_height_zero() {
        assert_eq!(water_render_height(0), 0.0);
    }

    #[test]
    fn test_water_render_height_min() {
        let h = water_render_height(1);
        let expected = 1.0 / 7.0;
        assert!((h - expected).abs() < 0.001);
    }

    #[test]
    fn test_is_submerged_in_full_water() {
        let mut map = WaterLevelMap::default();
        map.set_level(IVec3::new(5, 10, 5), MAX_WATER_LEVEL);
        assert!(is_submerged(Vec3::new(5.5, 10.3, 5.5), &map));
    }

    #[test]
    fn test_is_submerged_above_water() {
        let mut map = WaterLevelMap::default();
        map.set_level(IVec3::new(5, 10, 5), MAX_WATER_LEVEL);
        assert!(!is_submerged(Vec3::new(5.5, 10.9, 5.5), &map));
    }

    #[test]
    fn test_is_submerged_no_water() {
        let map = WaterLevelMap::default();
        assert!(!is_submerged(Vec3::new(5.5, 10.5, 5.5), &map));
    }

    #[test]
    fn test_is_submerged_partial_water() {
        let mut map = WaterLevelMap::default();
        map.set_level(IVec3::new(5, 10, 5), 3);
        let height = water_render_height(3);
        // At y=10.3, local_y = 0.3 < 0.4286 -> submerged
        assert!(is_submerged(Vec3::new(5.5, 10.3, 5.5), &map));
        // At y=10.5, local_y = 0.5 > 0.4286 -> not submerged
        assert!(!is_submerged(Vec3::new(5.5, 10.5, 5.5), &map));
    }

    #[test]
    fn test_flow_queue_deduplication() {
        let mut queue = WaterFlowQueue::default();
        queue.schedule(IVec3::ZERO);
        queue.schedule(IVec3::ZERO);
        queue.schedule(IVec3::ZERO);
        assert_eq!(queue.pending.len(), 1);
    }

    #[test]
    fn test_flow_queue_with_neighbors() {
        let mut queue = WaterFlowQueue::default();
        queue.schedule_with_neighbors(IVec3::ZERO);
        assert_eq!(queue.pending.len(), 7);
    }

    #[test]
    fn test_water_level_spread_logic() {
        let source_level: u8 = MAX_WATER_LEVEL;
        let spread_level = source_level - 1;
        assert_eq!(spread_level, 6);
        assert_eq!(2u8 - 1, 1);
        assert_eq!(1u8 - 1, 0);
    }

    #[test]
    fn test_world_to_chunk_and_local() {
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
}
