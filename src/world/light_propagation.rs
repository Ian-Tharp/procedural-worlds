//! Block light propagation using breadth-first search.
//!
//! When a light-emitting block (torch, glow block) is placed, light levels
//! propagate outward to adjacent blocks with linear falloff: each step reduces
//! the light level by 1. Light levels are stored per-block on a 0–15 scale
//! (matching Minecraft's convention).
//!
//! **Falloff model:** Linear — light decreases by 1 per block traversed.
//! This was chosen over exponential falloff for simplicity, predictability,
//! and compatibility with the integer 0–15 scale. A max-15 torch illuminates
//! up to 15 blocks away (configurable via `LightConfig::max_light_distance`).
//!
//! **Design:** Light maps are per-chunk. Cross-chunk propagation is supported
//! by collecting neighbor chunk light sources during propagation. The system
//! uses an event-driven approach: block changes emit `LightUpdateEvent`s,
//! which trigger incremental re-propagation only for affected chunks.

use std::collections::{HashMap, VecDeque};

use bevy::prelude::*;

use super::{BlockType, Chunk, CHUNK_SIZE, CHUNK_VOLUME};

// ============================================================================
// CONFIGURATION
// ============================================================================

/// Configuration for the light propagation system.
#[derive(Resource, Debug, Clone)]
pub struct LightConfig {
    /// Maximum distance light can travel from a source (default: 15).
    /// Clamped to 1–15 since light levels are stored as u8 on 0–15 scale.
    pub max_light_distance: u8,
}

impl Default for LightConfig {
    fn default() -> Self {
        Self {
            max_light_distance: 15,
        }
    }
}

// ============================================================================
// LIGHT MAP
// ============================================================================

/// Per-chunk light level storage.
///
/// Each block position stores a light level from 0 (dark) to 15 (max brightness).
/// The array is indexed identically to `Chunk::blocks`: `x + y * CHUNK_SIZE + z * CHUNK_SIZE * CHUNK_SIZE`.
#[derive(Debug, Clone)]
pub struct LightMap {
    /// Light levels for each block in the chunk (0–15).
    levels: [u8; CHUNK_VOLUME],
}

impl Default for LightMap {
    fn default() -> Self {
        Self {
            levels: [0; CHUNK_VOLUME],
        }
    }
}

impl LightMap {
    /// Create a new light map with all levels at 0.
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert local (x, y, z) coordinates to flat array index.
    #[inline]
    fn index(x: usize, y: usize, z: usize) -> usize {
        x + y * CHUNK_SIZE + z * CHUNK_SIZE * CHUNK_SIZE
    }

    /// Get the light level at local coordinates.
    /// Returns 0 for out-of-bounds coordinates.
    #[inline]
    pub fn get(&self, x: usize, y: usize, z: usize) -> u8 {
        if x < CHUNK_SIZE && y < CHUNK_SIZE && z < CHUNK_SIZE {
            self.levels[Self::index(x, y, z)]
        } else {
            0
        }
    }

    /// Set the light level at local coordinates.
    /// No-op for out-of-bounds coordinates.
    #[inline]
    pub fn set(&mut self, x: usize, y: usize, z: usize, level: u8) {
        if x < CHUNK_SIZE && y < CHUNK_SIZE && z < CHUNK_SIZE {
            self.levels[Self::index(x, y, z)] = level.min(15);
        }
    }

    /// Get a reference to the raw light level array.
    pub fn levels(&self) -> &[u8; CHUNK_VOLUME] {
        &self.levels
    }

    /// Clear all light levels to 0.
    pub fn clear(&mut self) {
        self.levels.fill(0);
    }
}

// ============================================================================
// LIGHT EMISSION LOOKUP
// ============================================================================

/// Returns the light emission level for a given block type (0–15).
///
/// Most blocks emit no light. Light-emitting blocks:
/// - Lava/Obsidian-glow blocks could be added here in the future.
///
/// Currently no built-in BlockType variants emit light since there are no
/// torch or glow block types in the enum. This function is ready to be
/// extended when those block types are added.
pub fn block_light_emission(_block: BlockType) -> u8 {
    // Future light sources would go here, e.g.:
    // match block {
    //     BlockType::Torch => 15,
    //     BlockType::GlowBlock => 12,
    //     BlockType::Lava => 15,
    //     _ => 0,
    // }
    0
}

/// Returns whether a block allows light to pass through it.
/// Transparent blocks (air, water) allow light; solid blocks do not.
#[inline]
pub fn block_transmits_light(block: BlockType) -> bool {
    block.is_transparent()
}

// ============================================================================
// BFS LIGHT PROPAGATION
// ============================================================================

/// A position in the BFS queue with its light level.
#[derive(Debug, Clone, Copy)]
struct LightNode {
    x: i32,
    y: i32,
    z: i32,
    level: u8,
}

/// Propagate light levels for a single chunk using BFS.
///
/// This clears the existing light map and recomputes from scratch by:
/// 1. Finding all light-emitting blocks in the chunk
/// 2. Seeding the BFS queue with those positions at their emission levels
/// 3. Spreading light to adjacent transparent blocks with -1 falloff per step
///
/// Light does not cross chunk boundaries in this function. For cross-chunk
/// propagation, see `propagate_light_with_neighbors`.
pub fn propagate_light(chunk: &Chunk, light_map: &mut LightMap, config: &LightConfig) {
    light_map.clear();

    let mut queue: VecDeque<LightNode> = VecDeque::new();
    let max_dist = config.max_light_distance;

    // Seed: find all light-emitting blocks
    for z in 0..CHUNK_SIZE {
        for y in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let block = chunk.get_block(x, y, z);
                let emission = block_light_emission(block);
                if emission > 0 {
                    let level = emission.min(max_dist);
                    light_map.set(x, y, z, level);
                    queue.push_back(LightNode {
                        x: x as i32,
                        y: y as i32,
                        z: z as i32,
                        level,
                    });
                }
            }
        }
    }

    // BFS propagation
    propagate_from_queue(chunk, light_map, &mut queue);
}

/// Propagate light from a BFS queue, staying within chunk bounds.
fn propagate_from_queue(chunk: &Chunk, light_map: &mut LightMap, queue: &mut VecDeque<LightNode>) {
    const NEIGHBORS: [(i32, i32, i32); 6] = [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ];

    while let Some(node) = queue.pop_front() {
        if node.level <= 1 {
            continue; // Can't propagate further
        }
        let new_level = node.level - 1;

        for (dx, dy, dz) in NEIGHBORS {
            let nx = node.x + dx;
            let ny = node.y + dy;
            let nz = node.z + dz;

            // Stay within chunk bounds
            if nx < 0
                || nx >= CHUNK_SIZE as i32
                || ny < 0
                || ny >= CHUNK_SIZE as i32
                || nz < 0
                || nz >= CHUNK_SIZE as i32
            {
                continue;
            }

            let (ux, uy, uz) = (nx as usize, ny as usize, nz as usize);

            // Only propagate through light-transmitting blocks
            let neighbor_block = chunk.get_block(ux, uy, uz);
            if !block_transmits_light(neighbor_block) {
                continue;
            }

            // Only update if we'd increase the light level
            if light_map.get(ux, uy, uz) >= new_level {
                continue;
            }

            light_map.set(ux, uy, uz, new_level);
            queue.push_back(LightNode {
                x: nx,
                y: ny,
                z: nz,
                level: new_level,
            });
        }
    }
}

/// Propagate light for a chunk, also considering light sources that bleed
/// in from the edges of neighbor chunks.
///
/// `neighbor_light` maps chunk-local border positions to the light level
/// arriving from adjacent chunks. These are seeded into the BFS alongside
/// the chunk's own emitters.
pub fn propagate_light_with_neighbors(
    chunk: &Chunk,
    light_map: &mut LightMap,
    config: &LightConfig,
    neighbor_light: &[(usize, usize, usize, u8)],
) {
    light_map.clear();

    let mut queue: VecDeque<LightNode> = VecDeque::new();
    let max_dist = config.max_light_distance;

    // Seed from this chunk's own emitters
    for z in 0..CHUNK_SIZE {
        for y in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let block = chunk.get_block(x, y, z);
                let emission = block_light_emission(block);
                if emission > 0 {
                    let level = emission.min(max_dist);
                    light_map.set(x, y, z, level);
                    queue.push_back(LightNode {
                        x: x as i32,
                        y: y as i32,
                        z: z as i32,
                        level,
                    });
                }
            }
        }
    }

    // Seed from neighbor chunk border light
    for &(x, y, z, level) in neighbor_light {
        if level > light_map.get(x, y, z) && block_transmits_light(chunk.get_block(x, y, z)) {
            light_map.set(x, y, z, level);
            queue.push_back(LightNode {
                x: x as i32,
                y: y as i32,
                z: z as i32,
                level,
            });
        }
    }

    propagate_from_queue(chunk, light_map, &mut queue);
}

// ============================================================================
// WORLD LIGHT MAP RESOURCE
// ============================================================================

/// Resource storing light maps for all loaded chunks.
#[derive(Resource, Default)]
pub struct WorldLightMap {
    /// Per-chunk light maps, keyed by chunk position.
    maps: HashMap<IVec3, LightMap>,
}

impl WorldLightMap {
    /// Get the light map for a chunk, if it exists.
    pub fn get(&self, chunk_pos: &IVec3) -> Option<&LightMap> {
        self.maps.get(chunk_pos)
    }

    /// Get or create a light map for a chunk.
    pub fn get_or_insert(&mut self, chunk_pos: IVec3) -> &mut LightMap {
        self.maps.entry(chunk_pos).or_default()
    }

    /// Remove the light map for a chunk (when unloaded).
    pub fn remove(&mut self, chunk_pos: &IVec3) -> Option<LightMap> {
        self.maps.remove(chunk_pos)
    }

    /// Get the number of stored light maps.
    pub fn len(&self) -> usize {
        self.maps.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.maps.is_empty()
    }
}

// ============================================================================
// LIGHT UPDATE EVENT
// ============================================================================

/// Event emitted when a block change requires light recalculation.
#[derive(Event, Debug, Clone)]
pub struct LightUpdateEvent {
    /// The chunk position that needs light recalculation.
    pub chunk_pos: IVec3,
}

// ============================================================================
// BEVY SYSTEMS
// ============================================================================

/// System that propagates light for chunks that have been marked dirty
/// or received a `LightUpdateEvent`.
pub fn update_light_maps(
    mut light_map_res: ResMut<WorldLightMap>,
    config: Res<LightConfig>,
    chunk_query: Query<&Chunk>,
    mut events: EventReader<LightUpdateEvent>,
    chunk_manager: Res<super::ChunkManager>,
) {
    // Collect unique chunk positions that need updates
    let mut to_update: Vec<IVec3> = events.read().map(|e| e.chunk_pos).collect();
    // IVec3 doesn't impl Ord, so use a HashSet for dedup
    let unique: std::collections::HashSet<IVec3> = to_update.drain(..).collect();
    let to_update: Vec<IVec3> = unique.into_iter().collect();

    for chunk_pos in to_update {
        if let Some(&entity) = chunk_manager.chunks.get(&chunk_pos)
            && let Ok(chunk) = chunk_query.get(entity)
        {
            let light_map = light_map_res.get_or_insert(chunk_pos);
            propagate_light(chunk, light_map, &config);
        }
    }
}

/// System that cleans up light maps for unloaded chunks.
pub fn cleanup_light_maps(
    mut light_map_res: ResMut<WorldLightMap>,
    chunk_manager: Res<super::ChunkManager>,
) {
    let loaded: std::collections::HashSet<IVec3> =
        chunk_manager.chunks.keys().copied().collect();
    light_map_res
        .maps
        .retain(|pos, _| loaded.contains(pos));
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin for the block light propagation system.
pub struct LightPropagationPlugin;

impl Plugin for LightPropagationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LightConfig>()
            .init_resource::<WorldLightMap>()
            .add_event::<LightUpdateEvent>()
            .add_systems(
                Update,
                (update_light_maps, cleanup_light_maps)
                    .chain()
                    .in_set(super::WorldSystems::Meshing),
            );
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::IVec3;

    /// Helper: create a chunk and manually set a block as a light source,
    /// then propagate. Since no BlockType currently emits light, we test
    /// the BFS by directly seeding the queue.
    fn propagate_with_manual_source(
        chunk: &Chunk,
        sources: &[(usize, usize, usize, u8)],
    ) -> LightMap {
        let mut light_map = LightMap::new();
        let mut queue: VecDeque<LightNode> = VecDeque::new();

        for &(x, y, z, level) in sources {
            light_map.set(x, y, z, level);
            queue.push_back(LightNode {
                x: x as i32,
                y: y as i32,
                z: z as i32,
                level,
            });
        }

        propagate_from_queue(chunk, &mut light_map, &mut queue);
        light_map
    }

    // ------------------------------------------------------------------
    // Test: BFS propagation from a single source
    // ------------------------------------------------------------------

    #[test]
    fn test_single_source_propagation() {
        // Air chunk with a light source at center
        let chunk = Chunk::new(IVec3::ZERO);
        let light_map = propagate_with_manual_source(&chunk, &[(8, 8, 8, 15)]);

        // Source should be 15
        assert_eq!(light_map.get(8, 8, 8), 15);

        // Adjacent blocks should be 14
        assert_eq!(light_map.get(9, 8, 8), 14);
        assert_eq!(light_map.get(7, 8, 8), 14);
        assert_eq!(light_map.get(8, 9, 8), 14);
        assert_eq!(light_map.get(8, 7, 8), 14);
        assert_eq!(light_map.get(8, 8, 9), 14);
        assert_eq!(light_map.get(8, 8, 7), 14);

        // 2 blocks away should be 13
        assert_eq!(light_map.get(10, 8, 8), 13);

        // Diagonal (Manhattan distance 2) should be 13
        assert_eq!(light_map.get(9, 9, 8), 13);
    }

    // ------------------------------------------------------------------
    // Test: Falloff calculation
    // ------------------------------------------------------------------

    #[test]
    fn test_linear_falloff() {
        let chunk = Chunk::new(IVec3::ZERO);
        let light_map = propagate_with_manual_source(&chunk, &[(8, 8, 8, 15)]);

        // Check falloff along +X axis
        for dist in 0..=7 {
            let expected = 15u8.saturating_sub(dist as u8);
            let actual = light_map.get(8 + dist, 8, 8);
            assert_eq!(
                actual, expected,
                "At distance {dist}, expected {expected}, got {actual}"
            );
        }
    }

    #[test]
    fn test_light_stops_at_zero() {
        // Source at level 3 — should only reach 3 blocks away
        let chunk = Chunk::new(IVec3::ZERO);
        let light_map = propagate_with_manual_source(&chunk, &[(8, 8, 8, 3)]);

        assert_eq!(light_map.get(8, 8, 8), 3);
        assert_eq!(light_map.get(9, 8, 8), 2);
        assert_eq!(light_map.get(10, 8, 8), 1);
        assert_eq!(light_map.get(11, 8, 8), 0); // Out of reach
    }

    // ------------------------------------------------------------------
    // Test: Multiple light sources interference
    // ------------------------------------------------------------------

    #[test]
    fn test_multiple_sources_max_wins() {
        let chunk = Chunk::new(IVec3::ZERO);
        // Two sources: level 15 at (4,8,8) and level 15 at (12,8,8)
        let light_map =
            propagate_with_manual_source(&chunk, &[(4, 8, 8, 15), (12, 8, 8, 15)]);

        // At midpoint (8,8,8), distance 4 from each source
        // From source at 4: 15 - 4 = 11
        // From source at 12: 15 - 4 = 11
        // Should be max = 11
        assert_eq!(light_map.get(8, 8, 8), 11);

        // Near source 1
        assert_eq!(light_map.get(4, 8, 8), 15);
        assert_eq!(light_map.get(5, 8, 8), 14);

        // Near source 2
        assert_eq!(light_map.get(12, 8, 8), 15);
        assert_eq!(light_map.get(11, 8, 8), 14);
    }

    #[test]
    fn test_multiple_sources_different_levels() {
        let chunk = Chunk::new(IVec3::ZERO);
        // Bright source at (4,8,8), dim source at (12,8,8)
        let light_map =
            propagate_with_manual_source(&chunk, &[(4, 8, 8, 15), (12, 8, 8, 5)]);

        // Source levels — dim source gets overwritten by bright source's reach (15-8=7 > 5)
        assert_eq!(light_map.get(4, 8, 8), 15);
        assert_eq!(light_map.get(12, 8, 8), 7); // 15 - 8 = 7 > 5, bright wins

        // Midpoint: 15 - 4 = 11 (bright source dominates)
        assert_eq!(light_map.get(8, 8, 8), 11);
    }

    // ------------------------------------------------------------------
    // Test: Light blocked by solid blocks
    // ------------------------------------------------------------------

    #[test]
    fn test_light_blocked_by_solid() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        // Place a wall of stone at x=9
        for y in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                chunk.set_block(9, y, z, BlockType::Stone);
            }
        }

        let light_map = propagate_with_manual_source(&chunk, &[(8, 8, 8, 15)]);

        // Source side
        assert_eq!(light_map.get(8, 8, 8), 15);
        assert_eq!(light_map.get(7, 8, 8), 14);

        // Wall blocks light (stone doesn't transmit)
        assert_eq!(light_map.get(9, 8, 8), 0);
        // Other side of wall should be dark
        assert_eq!(light_map.get(10, 8, 8), 0);
    }

    #[test]
    fn test_light_passes_through_water() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        // Place water at x=9
        chunk.set_block(9, 8, 8, BlockType::Water);

        let light_map = propagate_with_manual_source(&chunk, &[(8, 8, 8, 15)]);

        // Light should pass through water
        assert_eq!(light_map.get(9, 8, 8), 14);
        assert_eq!(light_map.get(10, 8, 8), 13);
    }

    // ------------------------------------------------------------------
    // Test: Light updates on block changes (via propagate_light)
    // ------------------------------------------------------------------

    #[test]
    fn test_repropagate_after_block_change() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        let config = LightConfig::default();

        // Initially no light (no emitters)
        let mut light_map = LightMap::new();
        propagate_light(&chunk, &mut light_map, &config);
        assert_eq!(light_map.get(8, 8, 8), 0);

        // Place a full wall at x=9 (all y,z) to completely block light
        for y in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                chunk.set_block(9, y, z, BlockType::Stone);
            }
        }
        let light_map = propagate_with_manual_source(&chunk, &[(8, 8, 8, 10)]);

        // Wall should completely block
        assert_eq!(light_map.get(8, 8, 8), 10);
        assert_eq!(light_map.get(9, 8, 8), 0);
        assert_eq!(light_map.get(10, 8, 8), 0);

        // Remove one block from the wall
        chunk.set_block(9, 8, 8, BlockType::Air);
        let light_map = propagate_with_manual_source(&chunk, &[(8, 8, 8, 10)]);

        // Light should now pass through the gap
        assert_eq!(light_map.get(9, 8, 8), 9);
        assert_eq!(light_map.get(10, 8, 8), 8);
    }

    // ------------------------------------------------------------------
    // Test: LightMap basics
    // ------------------------------------------------------------------

    #[test]
    fn test_light_map_default_zero() {
        let lm = LightMap::new();
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(lm.get(x, y, z), 0);
                }
            }
        }
    }

    #[test]
    fn test_light_map_set_get() {
        let mut lm = LightMap::new();
        lm.set(5, 10, 3, 12);
        assert_eq!(lm.get(5, 10, 3), 12);
    }

    #[test]
    fn test_light_map_clamps_to_15() {
        let mut lm = LightMap::new();
        lm.set(0, 0, 0, 255);
        assert_eq!(lm.get(0, 0, 0), 15);
    }

    #[test]
    fn test_light_map_out_of_bounds() {
        let lm = LightMap::new();
        assert_eq!(lm.get(16, 0, 0), 0);
        assert_eq!(lm.get(0, 16, 0), 0);
        assert_eq!(lm.get(0, 0, 16), 0);
    }

    #[test]
    fn test_light_map_clear() {
        let mut lm = LightMap::new();
        lm.set(5, 5, 5, 10);
        assert_eq!(lm.get(5, 5, 5), 10);
        lm.clear();
        assert_eq!(lm.get(5, 5, 5), 0);
    }

    // ------------------------------------------------------------------
    // Test: WorldLightMap resource
    // ------------------------------------------------------------------

    #[test]
    fn test_world_light_map_insert_and_get() {
        let mut wlm = WorldLightMap::default();
        let pos = IVec3::new(1, 2, 3);
        let lm = wlm.get_or_insert(pos);
        lm.set(0, 0, 0, 7);

        assert_eq!(wlm.get(&pos).unwrap().get(0, 0, 0), 7);
        assert_eq!(wlm.len(), 1);
    }

    #[test]
    fn test_world_light_map_remove() {
        let mut wlm = WorldLightMap::default();
        let pos = IVec3::new(1, 2, 3);
        wlm.get_or_insert(pos);
        assert_eq!(wlm.len(), 1);
        wlm.remove(&pos);
        assert_eq!(wlm.len(), 0);
    }

    // ------------------------------------------------------------------
    // Test: Neighbor light seeding
    // ------------------------------------------------------------------

    #[test]
    fn test_propagate_with_neighbor_light() {
        let chunk = Chunk::new(IVec3::ZERO);
        let config = LightConfig::default();

        // Simulate light bleeding in from +X neighbor at x=0 border
        let neighbor_light = vec![(0, 8, 8, 10u8)];
        let mut light_map = LightMap::new();
        propagate_light_with_neighbors(&chunk, &mut light_map, &config, &neighbor_light);

        assert_eq!(light_map.get(0, 8, 8), 10);
        assert_eq!(light_map.get(1, 8, 8), 9);
        assert_eq!(light_map.get(2, 8, 8), 8);
    }

    // ------------------------------------------------------------------
    // Test: Edge cases
    // ------------------------------------------------------------------

    #[test]
    fn test_source_at_chunk_corner() {
        let chunk = Chunk::new(IVec3::ZERO);
        let light_map = propagate_with_manual_source(&chunk, &[(0, 0, 0, 15)]);

        assert_eq!(light_map.get(0, 0, 0), 15);
        assert_eq!(light_map.get(1, 0, 0), 14);
        assert_eq!(light_map.get(0, 1, 0), 14);
        assert_eq!(light_map.get(0, 0, 1), 14);
        // No negative coordinates to check (they're out of bounds)
    }

    #[test]
    fn test_empty_chunk_no_light() {
        let chunk = Chunk::new(IVec3::ZERO);
        let config = LightConfig::default();
        let mut light_map = LightMap::new();
        propagate_light(&chunk, &mut light_map, &config);

        // All should be 0
        for level in light_map.levels().iter() {
            assert_eq!(*level, 0);
        }
    }
}
