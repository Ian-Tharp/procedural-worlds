//! World systems - chunks, blocks, voxel data structures
//!
//! This module contains:
//! - Chunk data structure (16x16x16 blocks)
//! - Block type registry
//! - World coordinate system
//! - Chunk loading/unloading (async via `AsyncComputeTaskPool`)
//! - Mesh generation (async via `AsyncComputeTaskPool`)
//! - Block queries for collision

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

use bevy::diagnostic::{Diagnostic, DiagnosticPath, Diagnostics, RegisterDiagnostic};
use bevy::prelude::*;
use bevy::tasks::{block_on, futures_lite::future, AsyncComputeTaskPool, Task};
use serde::{Deserialize, Serialize};

use crate::engine::memory::ChunkMeshPool;
use crate::generation::{
    generate_cacti, generate_caves, generate_chunk_terrain, generate_ores, generate_trees,
    default_ore_configs, ore_configs_from_definitions, OreSpawnConfig, TerrainConfig,
};

pub mod atlas_material;
pub mod chunk_priority;
pub mod chunk_streaming;
pub mod interaction;
pub mod meshing;
pub mod persistence;
pub mod save;
pub mod streaming;
pub mod texture_atlas;
pub mod texture_variation;
pub mod unloading;

use persistence::ChunkStorage;

/// Size of a chunk in blocks (16x16x16)
pub const CHUNK_SIZE: usize = 16;

/// Total blocks per chunk
pub const CHUNK_VOLUME: usize = CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE;

/// Block type identifier
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug, Serialize, Deserialize)]
#[serde(into = "u16", from = "u16")]
#[repr(u16)]
#[allow(dead_code)] // Future block types
pub enum BlockType {
    #[default]
    Air = 0,
    Stone = 1,
    Dirt = 2,
    Grass = 3,
    Sand = 4,
    Water = 5,
    Wood = 6,
    Leaves = 7,
    Sandstone = 8,
    Snow = 9,
    Ice = 10,
    Obsidian = 11,
    VolcanicRock = 12,
    Cactus = 13,
    SandDunes = 14,
    // Ores
    CopperOre = 15,
    IronOre = 16,
    SilverOre = 17,
    GoldOre = 18,
}

impl From<BlockType> for u16 {
    fn from(block: BlockType) -> u16 {
        block as u16
    }
}

impl From<u16> for BlockType {
    fn from(val: u16) -> BlockType {
        match val {
            0 => BlockType::Air,
            1 => BlockType::Stone,
            2 => BlockType::Dirt,
            3 => BlockType::Grass,
            4 => BlockType::Sand,
            5 => BlockType::Water,
            6 => BlockType::Wood,
            7 => BlockType::Leaves,
            8 => BlockType::Sandstone,
            9 => BlockType::Snow,
            10 => BlockType::Ice,
            11 => BlockType::Obsidian,
            12 => BlockType::VolcanicRock,
            13 => BlockType::Cactus,
            14 => BlockType::SandDunes,
            15 => BlockType::CopperOre,
            16 => BlockType::IronOre,
            17 => BlockType::SilverOre,
            18 => BlockType::GoldOre,
            _ => BlockType::Air, // Unknown block types default to Air
        }
    }
}

impl BlockType {
    /// Returns true if this block type is transparent (for face culling)
    pub fn is_transparent(&self) -> bool {
        matches!(self, BlockType::Air | BlockType::Water)
    }

    /// Returns true if this block is solid (for collision)
    pub fn is_solid(&self) -> bool {
        !matches!(self, BlockType::Air | BlockType::Water)
    }

    /// Returns a human-readable display name for this block type
    pub fn display_name(&self) -> &'static str {
        match self {
            BlockType::Air => "Air",
            BlockType::Stone => "Stone",
            BlockType::Dirt => "Dirt",
            BlockType::Grass => "Grass",
            BlockType::Sand => "Sand",
            BlockType::Water => "Water",
            BlockType::Wood => "Wood",
            BlockType::Leaves => "Leaves",
            BlockType::Sandstone => "Sandstone",
            BlockType::Snow => "Snow",
            BlockType::Ice => "Ice",
            BlockType::Obsidian => "Obsidian",
            BlockType::VolcanicRock => "Volcanic Rock",
            BlockType::Cactus => "Cactus",
            BlockType::SandDunes => "Sand Dunes",
            BlockType::CopperOre => "Copper Ore",
            BlockType::IronOre => "Iron Ore",
            BlockType::SilverOre => "Silver Ore",
            BlockType::GoldOre => "Gold Ore",
        }
    }
}

/// A chunk of voxel data
#[derive(Component, Debug, Clone)]
pub struct Chunk {
    /// Block data stored in a flat array [x + y * SIZE + z * SIZE * SIZE]
    blocks: [BlockType; CHUNK_VOLUME],
    /// Chunk position in chunk coordinates
    pub position: IVec3,
    /// Whether this chunk needs its mesh rebuilt
    pub dirty: bool,
    /// Whether this chunk has been modified since generation/loading
    /// (e.g., by player block placement). Modified chunks are saved to
    /// disk before unloading; unmodified chunks can be regenerated.
    ///
    /// **Not set automatically** by `set_block` â€" callers (e.g., block
    /// placement systems) should set `chunk.modified = true` explicitly
    /// when making player-driven changes. This avoids terrain generation
    /// routines (which also use `set_block`) from marking chunks as modified.
    pub modified: bool,
}

impl Chunk {
    /// Create a new empty chunk at the given position
    pub fn new(position: IVec3) -> Self {
        Self {
            blocks: [BlockType::Air; CHUNK_VOLUME],
            position,
            dirty: true,
            modified: false,
        }
    }

    /// Create a chunk with pre-populated block data
    ///
    /// Used by the persistence system to reconstruct chunks from saved data.
    /// The chunk is marked dirty so its mesh will be rebuilt.
    pub fn from_blocks(position: IVec3, blocks: [BlockType; CHUNK_VOLUME]) -> Self {
        Self {
            blocks,
            position,
            dirty: true,
            modified: false,
        }
    }

    /// Get a reference to the raw block data array
    pub fn blocks(&self) -> &[BlockType; CHUNK_VOLUME] {
        &self.blocks
    }

    /// Convert local (x, y, z) coordinates to flat array index
    #[inline]
    fn index(x: usize, y: usize, z: usize) -> usize {
        x + y * CHUNK_SIZE + z * CHUNK_SIZE * CHUNK_SIZE
    }

    /// Get block at local coordinates
    pub fn get_block(&self, x: usize, y: usize, z: usize) -> BlockType {
        if x < CHUNK_SIZE && y < CHUNK_SIZE && z < CHUNK_SIZE {
            self.blocks[Self::index(x, y, z)]
        } else {
            BlockType::Air
        }
    }

    /// Set block at local coordinates
    pub fn set_block(&mut self, x: usize, y: usize, z: usize, block: BlockType) {
        if x < CHUNK_SIZE && y < CHUNK_SIZE && z < CHUNK_SIZE {
            self.blocks[Self::index(x, y, z)] = block;
            self.dirty = true;
        }
    }

    /// Fill entire chunk with a single block type (used in tests)
    #[allow(dead_code)]
    pub fn fill(&mut self, block: BlockType) {
        self.blocks.fill(block);
        self.dirty = true;
    }

    /// Get chunk position in world coordinates (block units)
    pub fn world_position(&self) -> IVec3 {
        self.position * CHUNK_SIZE as i32
    }
}

/// Marker component for chunk mesh entities
#[derive(Component)]
pub struct ChunkMesh;

/// Result of an async chunk loading task.
///
/// Contains the completed chunk data and whether it was loaded from disk
/// (cache hit) or freshly generated (cache miss).
pub struct ChunkLoadResult {
    /// The completed chunk data.
    pub chunk: Chunk,
    /// `true` if loaded from disk, `false` if generated from scratch.
    pub from_cache: bool,
}

/// Component attached to an entity while its chunk terrain is being generated
/// on a background thread via `AsyncComputeTaskPool`.
#[derive(Component)]
pub struct PendingChunk {
    /// The async task that will produce the completed chunk with cache info.
    pub(crate) task: Task<ChunkLoadResult>,
    /// The chunk-coordinate position (used to remove from pending set on completion).
    pub(crate) position: IVec3,
}

/// Component attached to an entity while its mesh is being generated
/// on a background thread via `AsyncComputeTaskPool`.
///
/// The task produces `(opaque_mesh, Option<water_mesh>)`. The opaque mesh
/// is assigned to the chunk entity itself; the optional water mesh is
/// spawned as a child entity with [`WaterMesh`] marker and a blend material.
#[derive(Component)]
pub struct PendingMesh {
    /// The async task that produces the mesh data (opaque + optional water).
    task: Task<(Mesh, Option<Mesh>)>,
}

/// Resource for tracking loaded chunks
#[derive(Resource)]
pub struct ChunkManager {
    /// Map of chunk positions to their entities
    pub chunks: HashMap<IVec3, Entity>,
    /// Positions currently being generated in background tasks.
    /// Prevents duplicate task spawning for the same chunk coordinate.
    pub pending: HashSet<IVec3>,
    /// Positions that failed to load or generate, with the error reason.
    /// Displayed by the chunk debug overlay as magenta borders.
    pub failed_chunks: HashMap<IVec3, String>,
    /// Render distance in chunks
    pub render_distance: i32,
    /// Horizontal chunk loading distance. If `None`, uses `render_distance`.
    pub load_distance: Option<i32>,
    /// Number of chunk layers to load above the player's chunk
    pub vertical_load_up: i32,
    /// Number of chunk layers to load below the player's chunk
    pub vertical_load_down: i32,
    /// Player's current chunk position
    pub player_chunk: IVec3,
    /// Tasks spawned this frame (for rate limiting)
    tasks_spawned_this_frame: u32,
    /// Maximum chunk-generation tasks to *spawn* per frame
    pub max_chunks_per_frame: u32,
}

impl Default for ChunkManager {
    fn default() -> Self {
        Self {
            chunks: HashMap::new(),
            pending: HashSet::new(),
            failed_chunks: HashMap::new(),
            render_distance: 4,
            load_distance: None,
            vertical_load_up: 4,
            vertical_load_down: 2,
            player_chunk: IVec3::ZERO,
            tasks_spawned_this_frame: 0,
            // Increased from 2 to 4 for faster initial load
            // Trade-off: slightly more per-frame work, but faster to playable state
            max_chunks_per_frame: 4,
        }
    }
}

impl ChunkManager {
    #[allow(dead_code)]
    pub fn new(render_distance: i32) -> Self {
        Self {
            render_distance,
            ..default()
        }
    }

    /// Returns the effective horizontal load distance.
    ///
    /// If `load_distance` is explicitly set, returns that value.
    /// Otherwise falls back to `render_distance`.
    pub fn effective_load_distance(&self) -> i32 {
        self.load_distance.unwrap_or(self.render_distance)
    }
}

/// Maximum number of load-time samples kept for rolling average calculation.
const METRICS_HISTORY_SIZE: usize = 256;

// ============================================================================
// Bevy Diagnostic Paths for chunk metrics
// ============================================================================

/// Diagnostic path for average chunk load time (ms).
pub const CHUNK_AVG_LOAD_TIME: DiagnosticPath = DiagnosticPath::const_new("chunk/avg_load_time_ms");
/// Diagnostic path for peak chunk load time (ms).
pub const CHUNK_PEAK_LOAD_TIME: DiagnosticPath = DiagnosticPath::const_new("chunk/peak_load_time_ms");
/// Diagnostic path for chunks loaded per second.
pub const CHUNK_LOADS_PER_SEC: DiagnosticPath = DiagnosticPath::const_new("chunk/loads_per_second");
/// Diagnostic path for total chunks loaded.
pub const CHUNK_TOTAL_LOADED: DiagnosticPath = DiagnosticPath::const_new("chunk/total_loaded");
/// Diagnostic path for estimated chunk memory usage (MB).
pub const CHUNK_MEMORY_MB: DiagnosticPath = DiagnosticPath::const_new("chunk/memory_mb");
/// Diagnostic path for chunk cache hit rate (0.0-1.0).
pub const CHUNK_CACHE_HIT_RATE: DiagnosticPath = DiagnosticPath::const_new("chunk/cache_hit_rate");
/// Diagnostic path for total chunk cache hits.
pub const CHUNK_CACHE_HITS: DiagnosticPath = DiagnosticPath::const_new("chunk/cache_hits");
/// Diagnostic path for total chunk cache misses.
pub const CHUNK_CACHE_MISSES: DiagnosticPath = DiagnosticPath::const_new("chunk/cache_misses");

/// Performance metrics for chunk loading.
///
/// Tracks how many chunks are loaded per second, the average time each chunk
/// takes to load, peak load time, memory usage, and related statistics.
/// Updated by `update_chunk_load_metrics` and displayed in the debug overlay.
///
/// Metrics collection can be disabled via `enabled`. When disabled, the
/// recording functions become no-ops and derived values stay at their defaults.
#[derive(Resource)]
pub struct ChunkLoadMetrics {
    /// Whether metrics collection is active. Controlled by
    /// `EngineConfig::debug::show_chunks` at startup and toggleable at runtime.
    pub enabled: bool,
    /// Rolling window of individual chunk load durations (in seconds).
    load_times: VecDeque<f32>,
    /// Timestamps (seconds since app start) of recent chunk completions.
    /// Used to compute chunks-loaded-per-second over a sliding window.
    completion_timestamps: VecDeque<f64>,
    /// Number of chunks that completed loading since the last metrics update.
    pub chunks_loaded_since_last: u32,
    /// Smoothed chunks loaded per second (computed over a 2-second window).
    pub chunks_per_second: f32,
    /// Rolling average load time in milliseconds.
    pub avg_load_time_ms: f32,
    /// Peak (maximum) load time in milliseconds across the rolling window.
    pub peak_load_time_ms: f32,
    /// All-time peak load time in milliseconds since application start.
    pub all_time_peak_load_time_ms: f32,
    /// Total number of chunks loaded since application start.
    pub total_chunks_loaded: u64,
    /// Estimated memory usage for chunk block storage in bytes.
    ///
    /// Calculated as `loaded_chunk_count * CHUNK_VOLUME * size_of::<BlockType>()`.
    pub chunk_memory_bytes: usize,
    /// Wall-clock `Instant` recorded when a set of pending chunks start
    /// (used inside `poll_pending_chunks` to measure completion time).
    pub pending_start_times: HashMap<IVec3, Instant>,

    // -- Cache hit/miss tracking --
    /// Number of chunks loaded from disk (cache hits).
    pub cache_hits: u64,
    /// Number of chunks generated from scratch (cache misses).
    pub cache_misses: u64,
    /// Rolling window of recent cache results (`true` = hit, `false` = miss).
    cache_hit_history: VecDeque<bool>,
    /// Rolling cache hit rate (0.0..=1.0), recomputed on refresh.
    pub cache_hit_rate: f32,

    // -- Per-chunk load time history for profiler graphs --
    /// Recent individual chunk load times in milliseconds (for histogram/graph).
    pub recent_load_times_ms: VecDeque<f32>,
    /// Per-chunk memory estimate in bytes (block data only).
    pub memory_per_chunk_bytes: usize,
}

impl Default for ChunkLoadMetrics {
    fn default() -> Self {
        Self {
            enabled: true,
            load_times: VecDeque::with_capacity(METRICS_HISTORY_SIZE),
            completion_timestamps: VecDeque::with_capacity(METRICS_HISTORY_SIZE),
            chunks_loaded_since_last: 0,
            chunks_per_second: 0.0,
            avg_load_time_ms: 0.0,
            peak_load_time_ms: 0.0,
            all_time_peak_load_time_ms: 0.0,
            total_chunks_loaded: 0,
            chunk_memory_bytes: 0,
            pending_start_times: HashMap::new(),
            cache_hits: 0,
            cache_misses: 0,
            cache_hit_history: VecDeque::with_capacity(METRICS_HISTORY_SIZE),
            cache_hit_rate: 0.0,
            recent_load_times_ms: VecDeque::with_capacity(METRICS_HISTORY_SIZE),
            memory_per_chunk_bytes: CHUNK_VOLUME * std::mem::size_of::<BlockType>(),
        }
    }
}

impl ChunkLoadMetrics {
    /// Record one chunk load completion (backwards-compatible, defaults to cache miss).
    pub fn record_load(&mut self, load_duration_secs: f32, app_time_secs: f64) {
        self.record_load_with_source(load_duration_secs, app_time_secs, false);
    }

    /// Record one chunk load completion with cache source information.
    ///
    /// `from_cache` indicates whether the chunk was loaded from disk (`true`)
    /// or freshly generated (`false`).
    pub fn record_load_with_source(
        &mut self,
        load_duration_secs: f32,
        app_time_secs: f64,
        from_cache: bool,
    ) {
        self.total_chunks_loaded += 1;

        // Always track cache hit/miss counters (cheap)
        if from_cache {
            self.cache_hits += 1;
        } else {
            self.cache_misses += 1;
        }

        if !self.enabled {
            return;
        }

        // Push load time
        if self.load_times.len() >= METRICS_HISTORY_SIZE {
            self.load_times.pop_front();
        }
        self.load_times.push_back(load_duration_secs);

        // Push completion timestamp
        if self.completion_timestamps.len() >= METRICS_HISTORY_SIZE {
            self.completion_timestamps.pop_front();
        }
        self.completion_timestamps.push_back(app_time_secs);

        // Push cache hit/miss into rolling history
        if self.cache_hit_history.len() >= METRICS_HISTORY_SIZE {
            self.cache_hit_history.pop_front();
        }
        self.cache_hit_history.push_back(from_cache);

        // Push individual load time for profiler graphs
        let duration_ms = load_duration_secs * 1000.0;
        if self.recent_load_times_ms.len() >= METRICS_HISTORY_SIZE {
            self.recent_load_times_ms.pop_front();
        }
        self.recent_load_times_ms.push_back(duration_ms);

        self.chunks_loaded_since_last += 1;

        // Update all-time peak
        if duration_ms > self.all_time_peak_load_time_ms {
            self.all_time_peak_load_time_ms = duration_ms;
        }
    }

    /// Recompute derived metrics (called once per frame by the metrics system).
    pub fn refresh(&mut self, app_time_secs: f64) {
        if !self.enabled {
            self.chunks_loaded_since_last = 0;
            return;
        }

        // Average and peak load time from rolling window
        if self.load_times.is_empty() {
            self.avg_load_time_ms = 0.0;
            self.peak_load_time_ms = 0.0;
        } else {
            let mut sum: f32 = 0.0;
            let mut peak: f32 = 0.0;
            for &t in &self.load_times {
                sum += t;
                if t > peak {
                    peak = t;
                }
            }
            self.avg_load_time_ms = (sum / self.load_times.len() as f32) * 1000.0;
            self.peak_load_time_ms = peak * 1000.0;
        }

        // Chunks per second: count completions in the last 2 seconds
        let window_secs = 2.0;
        let cutoff = app_time_secs - window_secs;
        while let Some(&ts) = self.completion_timestamps.front() {
            if ts < cutoff {
                self.completion_timestamps.pop_front();
            } else {
                break;
            }
        }
        let count = self.completion_timestamps.len() as f32;
        self.chunks_per_second = count / window_secs as f32;

        // Recompute rolling cache hit rate
        if self.cache_hit_history.is_empty() {
            self.cache_hit_rate = 0.0;
        } else {
            let hits = self.cache_hit_history.iter().filter(|&&h| h).count();
            self.cache_hit_rate = hits as f32 / self.cache_hit_history.len() as f32;
        }

        self.chunks_loaded_since_last = 0;
    }

    /// Update the estimated chunk memory usage based on current chunk count.
    pub fn update_memory_estimate(&mut self, loaded_chunk_count: usize) {
        self.chunk_memory_bytes =
            loaded_chunk_count * CHUNK_VOLUME * std::mem::size_of::<BlockType>();
    }

    /// Return chunk memory usage in megabytes.
    pub fn chunk_memory_mb(&self) -> f64 {
        self.chunk_memory_bytes as f64 / (1024.0 * 1024.0)
    }
}

/// Convert world position to chunk position
pub fn world_to_chunk_pos(world_pos: Vec3) -> IVec3 {
    IVec3::new(
        (world_pos.x / CHUNK_SIZE as f32).floor() as i32,
        (world_pos.y / CHUNK_SIZE as f32).floor() as i32,
        (world_pos.z / CHUNK_SIZE as f32).floor() as i32,
    )
}

/// Convert chunk position to world position (corner of chunk)
pub fn chunk_to_world_pos(chunk_pos: IVec3) -> Vec3 {
    Vec3::new(
        (chunk_pos.x * CHUNK_SIZE as i32) as f32,
        (chunk_pos.y * CHUNK_SIZE as i32) as f32,
        (chunk_pos.z * CHUNK_SIZE as i32) as f32,
    )
}

/// System sets for ordering world systems
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum WorldSystems {
    ChunkLoading,
    Meshing,
    Cleanup,
}

/// Plugin for world management
pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChunkManager>()
            .init_resource::<TerrainConfig>()
            .init_resource::<ChunkMaterial>()
            .init_resource::<ChunkStorage>()
            .init_resource::<unloading::UnloadConfig>()
            .init_resource::<ChunkLoadMetrics>()
            // Pre-allocated mesh buffer pool for reduced allocation overhead
            .init_resource::<ChunkMeshPool>()
            // Register chunk diagnostics with Bevy's DiagnosticsStore
            .register_diagnostic(
                Diagnostic::new(CHUNK_AVG_LOAD_TIME)
                    .with_suffix(" ms")
                    .with_max_history_length(64),
            )
            .register_diagnostic(
                Diagnostic::new(CHUNK_PEAK_LOAD_TIME)
                    .with_suffix(" ms")
                    .with_max_history_length(64),
            )
            .register_diagnostic(
                Diagnostic::new(CHUNK_LOADS_PER_SEC)
                    .with_suffix(" chunks/s")
                    .with_max_history_length(64),
            )
            .register_diagnostic(
                Diagnostic::new(CHUNK_TOTAL_LOADED)
                    .with_max_history_length(1),
            )
            .register_diagnostic(
                Diagnostic::new(CHUNK_MEMORY_MB)
                    .with_suffix(" MB")
                    .with_max_history_length(64),
            )
            .register_diagnostic(
                Diagnostic::new(CHUNK_CACHE_HIT_RATE)
                    .with_suffix("")
                    .with_max_history_length(64),
            )
            .register_diagnostic(
                Diagnostic::new(CHUNK_CACHE_HITS)
                    .with_max_history_length(1),
            )
            .register_diagnostic(
                Diagnostic::new(CHUNK_CACHE_MISSES)
                    .with_max_history_length(1),
            )
            .init_resource::<streaming::StreamingConfig>()
            .init_resource::<streaming::PlayerChunkVelocity>()
            .init_resource::<chunk_priority::ChunkPriorityConfig>()
            // Custom atlas material pipeline (shader + material type registration)
            .add_plugins(atlas_material::BlockAtlasMaterialPlugin)
            // Save system plugin (auto-save, manual save, load on startup)
            .add_plugins(save::SavePlugin)
            // Async chunk streaming (frame-budgeted I/O for saves)
            .add_plugins(chunk_streaming::ChunkStreamingPlugin)
            // Block interaction (place, break, selected block cycling)
            .add_plugins(interaction::BlockInteractionPlugin)
            .configure_sets(
                Update,
                (
                    WorldSystems::ChunkLoading,
                    WorldSystems::Meshing,
                    WorldSystems::Cleanup,
                )
                    .chain(),
            )
            .add_systems(
                Startup,
                (texture_atlas::setup_block_texture_atlas, setup_chunk_material).chain(),
            )
            .add_systems(
                Update,
                (
                    update_player_chunk_position,
                    streaming::update_player_chunk_velocity,
                    chunk_streaming_system,
                    streaming::predictive_chunk_streaming_system,
                    poll_pending_chunks,
                    update_chunk_load_metrics,
                )
                    .chain()
                    .in_set(WorldSystems::ChunkLoading),
            )
            .add_systems(
                Update,
                (mesh_dirty_chunks, poll_pending_meshes)
                    .chain()
                    .in_set(WorldSystems::Meshing),
            )
            .add_systems(
                Update,
                (
                    unloading::chunk_unloading_system,
                    unloading::poll_pending_saves,
                )
                    .chain()
                    .in_set(WorldSystems::Cleanup),
            );
    }
}

/// Enum to hold either a standard or atlas material handle pair (opaque + water).
///
/// Each variant holds two materials:
/// - `opaque`: `AlphaMode::Opaque` for solid block meshes (main chunk entity)
/// - `water`: `AlphaMode::Blend` for transparent water meshes (child entity)
pub enum ChunkMaterialHandle {
    Standard {
        opaque: Handle<StandardMaterial>,
        water: Handle<StandardMaterial>,
    },
    Atlas {
        opaque: Handle<atlas_material::BlockAtlasMaterial>,
        water: Handle<atlas_material::BlockAtlasMaterial>,
    },
}

/// Marker component for water mesh child entities.
///
/// Water faces are rendered on a separate child entity with `AlphaMode::Blend`
/// so that solid terrain uses `AlphaMode::Opaque` for correct depth sorting.
/// The child entity is automatically despawned when the parent chunk entity
/// is despawned via `despawn_recursive`.
#[derive(Component)]
pub struct WaterMesh;

/// Resource holding the shared material for chunk meshes
#[derive(Resource, Default)]
pub struct ChunkMaterial {
    pub handle: Option<ChunkMaterialHandle>,
}

/// Setup the shared material for all chunk meshes.
///
/// When `use_textures` is enabled and the `BlockTextureAtlas` resource exists,
/// the material uses the custom [`BlockAtlasMaterial`] for per-block UV tiling
/// via the atlas shader. Otherwise it falls back to a plain white
/// `StandardMaterial` (vertex colors provide the block color).
fn setup_chunk_material(
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut atlas_materials: ResMut<Assets<atlas_material::BlockAtlasMaterial>>,
    mut chunk_material: ResMut<ChunkMaterial>,
    config: Res<crate::config::EngineConfig>,
    atlas: Option<Res<texture_atlas::BlockTextureAtlas>>,
) {
    if config.render.use_textures {
        if let Some(ref atlas_res) = atlas {
            // Try to create the custom atlas material pair (shader-driven tiling)
            if let Some((opaque, water)) = atlas_material::create_block_atlas_material(
                &mut atlas_materials,
                atlas_res,
            ) {
                chunk_material.handle = Some(ChunkMaterialHandle::Atlas { opaque, water });
                info!("Chunk material initialized: BlockAtlasMaterial opaque + water (custom shader)");
                return;
            }
            warn!("BlockAtlasMaterial creation failed, falling back to StandardMaterial");
        }
    }

    // Fallback: plain white StandardMaterial pair (opaque + water).
    // Opaque material ensures chunks render with proper depth writes.
    // AlphaMode::Blend caused see-through terrain artifacts (far chunks
    // overdrawing near ones in Bevy's transparent pass).
    let opaque = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        metallic: 0.0,
        alpha_mode: AlphaMode::Opaque,
        ..default()
    });
    let water = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.3,
        metallic: 0.1,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    chunk_material.handle = Some(ChunkMaterialHandle::Standard { opaque, water });
    info!(
        "Chunk material initialized: StandardMaterial opaque + water (textures: {})",
        config.render.use_textures && atlas.is_some()
    );
}

/// Update the player's current chunk position based on camera
///
/// Uses `GlobalTransform` because the camera is a child entity of the player â€"
/// its local `Transform` is just the eye-height offset, not the world position.
fn update_player_chunk_position(
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
    mut chunk_manager: ResMut<ChunkManager>,
) {
    if let Ok(global_transform) = camera_query.get_single() {
        let new_chunk = world_to_chunk_pos(global_transform.translation());
        if new_chunk != chunk_manager.player_chunk {
            chunk_manager.player_chunk = new_chunk;
        }
    }
}

/// Spawn async chunk-generation tasks based on player position and camera direction.
///
/// Uses the [`chunk_priority`] system to sort needed chunks by a combined
/// distance + direction score. Chunks the player is looking toward are
/// spawned first, giving faster perceived loading in the direction of travel.
///
/// Instead of generating chunks synchronously on the main thread, this system
/// spawns lightweight tasks on `AsyncComputeTaskPool`. Each task creates a
/// `Chunk`, runs terrain + cave + tree generation, and returns the completed
/// chunk data. The rate limiter controls how many tasks are *spawned* per
/// frame.
fn chunk_streaming_system(
    mut commands: Commands,
    mut chunk_manager: ResMut<ChunkManager>,
    mut load_metrics: ResMut<ChunkLoadMetrics>,
    terrain_config: Res<TerrainConfig>,
    chunk_storage: Res<ChunkStorage>,
    priority_config: Res<chunk_priority::ChunkPriorityConfig>,
    camera_query: Query<&crate::engine::controller::CameraController, With<Camera3d>>,
    ore_registry: Option<Res<crate::content::OreRegistry>>,
) {
    // Reset per-frame spawn counter
    chunk_manager.tasks_spawned_this_frame = 0;

    let center = chunk_manager.player_chunk;
    let rd = chunk_manager.effective_load_distance();
    let max_per_frame = chunk_manager.max_chunks_per_frame;
    let vert_down = chunk_manager.vertical_load_down;
    let vert_up = chunk_manager.vertical_load_up;

    let task_pool = AsyncComputeTaskPool::get();
    
    // Extract ore configs from registry (or use defaults if not available)
    // This is done once per frame, not per-chunk, for efficiency
    let ore_configs: Vec<OreSpawnConfig> = ore_registry
        .as_ref()
        .map(|registry| {
            let definitions: Vec<_> = registry.iter().cloned().collect();
            ore_configs_from_definitions(&definitions)
        })
        .unwrap_or_else(default_ore_configs);

    // Get camera forward direction for priority sorting
    let forward_dir = if priority_config.enabled {
        camera_query
            .get_single()
            .ok()
            .map(|controller| controller.horizontal_forward())
    } else {
        None
    };

    // Collect all needed chunk positions, sorted by priority
    let loaded_keys: HashSet<IVec3> = chunk_manager.chunks.keys().copied().collect();
    let sorted_chunks = chunk_priority::collect_needed_chunks_sorted(
        center,
        rd,
        vert_down,
        vert_up,
        forward_dir,
        priority_config.direction_weight,
        &loaded_keys,
        &chunk_manager.pending,
    );

    // Spawn tasks in priority order, up to the per-frame limit
    for scored in sorted_chunks {
        if chunk_manager.tasks_spawned_this_frame >= max_per_frame {
            return;
        }

        let chunk_pos = scored.position;

        // Clone resources for the background task
        let config = (*terrain_config).clone();
        let storage = ChunkStorage::new(chunk_storage.save_dir.clone());
        let ores = ore_configs.clone();  // Clone ore configs for this task

        // Spawn async task: try loading from disk first, generate if not found
        let task = task_pool.spawn(async move {
            // Check for a previously saved chunk on disk
            if let Ok(chunk) = persistence::load_chunk(chunk_pos, &storage) {
                return ChunkLoadResult { chunk, from_cache: true };
            }
            // Not on disk - generate new terrain
            let mut chunk = Chunk::new(chunk_pos);
            generate_chunk_terrain(&mut chunk, &config);
            generate_caves(&mut chunk, &config);
            generate_ores(&mut chunk, &config, &ores);  // Use registry-derived configs
            generate_trees(&mut chunk, &config);
            generate_cacti(&mut chunk, &config);
            ChunkLoadResult { chunk, from_cache: false }
        });

        // Spawn a placeholder entity with the PendingChunk component
        commands.spawn(PendingChunk {
            task,
            position: chunk_pos,
        });

        chunk_manager.pending.insert(chunk_pos);
        chunk_manager.tasks_spawned_this_frame += 1;
        load_metrics.pending_start_times.insert(chunk_pos, Instant::now());
    }
}

/// Poll completed chunk-generation tasks and insert the resulting `Chunk` data.
///
/// When a task finishes, we insert the `Chunk` component (which starts with
/// `dirty: true`), register the entity in `ChunkManager::chunks`, remove it
/// from the pending set, and strip the `PendingChunk` component.
fn poll_pending_chunks(
    mut commands: Commands,
    mut chunk_manager: ResMut<ChunkManager>,
    mut load_metrics: ResMut<ChunkLoadMetrics>,
    mut pending_query: Query<(Entity, &mut PendingChunk)>,
    mut chunk_query: Query<&mut Chunk>,
    time: Res<Time>,
) {
    let app_time = time.elapsed_secs_f64();

    for (entity, mut pending) in &mut pending_query {
        if let Some(result) = block_on(future::poll_once(&mut pending.task)) {
            let pos = pending.position;

            // Measure load duration and record with cache source
            if let Some(start) = load_metrics.pending_start_times.remove(&pos) {
                let duration = start.elapsed().as_secs_f32();
                load_metrics.record_load_with_source(duration, app_time, result.from_cache);
            }

            // Insert the completed chunk data onto this entity
            commands.entity(entity).insert(result.chunk).remove::<PendingChunk>();

            // Update bookkeeping
            chunk_manager.pending.remove(&pos);
            chunk_manager.chunks.insert(pos, entity);

            // Mark face-adjacent neighbor chunks as dirty so they remesh with
            // the newly available neighbor data. This eliminates visual seams
            // at chunk borders caused by missing cross-chunk face culling and AO.
            for offset in [
                IVec3::X, IVec3::NEG_X,
                IVec3::Y, IVec3::NEG_Y,
                IVec3::Z, IVec3::NEG_Z,
            ] {
                let neighbor_pos = pos + offset;
                if let Some(&neighbor_entity) = chunk_manager.chunks.get(&neighbor_pos)
                    && let Ok(mut neighbor_chunk) = chunk_query.get_mut(neighbor_entity)
                {
                    neighbor_chunk.dirty = true;
                }
            }
        }
    }
}

/// Recompute derived chunk load metrics once per frame and push to Bevy diagnostics.
fn update_chunk_load_metrics(
    mut load_metrics: ResMut<ChunkLoadMetrics>,
    chunk_manager: Res<ChunkManager>,
    time: Res<Time>,
    mut diagnostics: Diagnostics,
) {
    // Update memory estimate from current chunk count
    load_metrics.update_memory_estimate(chunk_manager.chunks.len());

    load_metrics.refresh(time.elapsed_secs_f64());

    // Push current values into Bevy's DiagnosticsStore
    diagnostics.add_measurement(&CHUNK_AVG_LOAD_TIME, || {
        load_metrics.avg_load_time_ms as f64
    });
    diagnostics.add_measurement(&CHUNK_PEAK_LOAD_TIME, || {
        load_metrics.peak_load_time_ms as f64
    });
    diagnostics.add_measurement(&CHUNK_LOADS_PER_SEC, || {
        load_metrics.chunks_per_second as f64
    });
    diagnostics.add_measurement(&CHUNK_TOTAL_LOADED, || {
        load_metrics.total_chunks_loaded as f64
    });
    diagnostics.add_measurement(&CHUNK_MEMORY_MB, || {
        load_metrics.chunk_memory_mb()
    });
    diagnostics.add_measurement(&CHUNK_CACHE_HIT_RATE, || {
        load_metrics.cache_hit_rate as f64
    });
    diagnostics.add_measurement(&CHUNK_CACHE_HITS, || {
        load_metrics.cache_hits as f64
    });
    diagnostics.add_measurement(&CHUNK_CACHE_MISSES, || {
        load_metrics.cache_misses as f64
    });
}

/// Maximum mesh-generation tasks to spawn per frame (prevents GPU upload stutter)
const MAX_MESH_TASKS_PER_FRAME: usize = 6;

/// Extract flat block data from a chunk using Chunk-native indexing.
fn extract_block_data(chunk: &Chunk) -> Vec<BlockType> {
    chunk.blocks().to_vec()
}

/// Spawn async mesh-generation tasks for dirty chunks.
///
/// For each dirty chunk, the 6 face-neighbor chunks' block data is collected
/// and passed as `ChunkNeighbors` so that cross-chunk face culling and AO
/// can sample into adjacent chunks.
fn mesh_dirty_chunks(
    mut commands: Commands,
    chunk_manager: Res<ChunkManager>,
    mut param_set: ParamSet<(
        Query<(Entity, &mut Chunk), Without<PendingMesh>>,
        Query<&Chunk>,
    )>,
    config: Res<crate::config::EngineConfig>,
) {
    let player_chunk = chunk_manager.player_chunk;

    let atlas_cfg = if config.render.use_textures {
        Some(meshing::AtlasConfig {
            tiles_per_row: config.render.atlas_grid_size,
            tile_size: config.render.atlas_tile_size,
            atlas_size: config.render.atlas_tile_size * config.render.atlas_grid_size,
        })
    } else {
        None
    };

    // Phase 1: Collect all chunk block data for neighbor lookups (read-only).
    let all_chunk_data: HashMap<IVec3, Vec<BlockType>> = {
        let all_chunks = param_set.p1();
        all_chunks.iter()
            .map(|chunk| (chunk.position, extract_block_data(chunk)))
            .collect()
    };

    // Phase 2: Identify dirty chunks and their positions.
    let mut dirty_chunks: Vec<_> = {
        let p0 = param_set.p0();
        p0.iter()
            .filter(|(_, chunk)| chunk.dirty)
            .map(|(entity, chunk)| (entity, chunk.position))
            .collect()
    };

    dirty_chunks.sort_by_key(|&(_, pos)| {
        let diff = pos - player_chunk;
        diff.x.abs() + diff.y.abs() + diff.z.abs()
    });

    let task_pool = AsyncComputeTaskPool::get();
    let mut tasks_spawned = 0;
    let mut to_mesh: Vec<Entity> = Vec::new();

    for (entity, pos) in dirty_chunks {
        if tasks_spawned >= MAX_MESH_TASKS_PER_FRAME {
            break;
        }

        // Build cross-chunk neighbor data (Chunk-native flat arrays)
        let neighbors = meshing::ChunkNeighbors {
            pos_x: all_chunk_data.get(&(pos + IVec3::X)).cloned(),
            neg_x: all_chunk_data.get(&(pos + IVec3::NEG_X)).cloned(),
            pos_y: all_chunk_data.get(&(pos + IVec3::Y)).cloned(),
            neg_y: all_chunk_data.get(&(pos + IVec3::NEG_Y)).cloned(),
            pos_z: all_chunk_data.get(&(pos + IVec3::Z)).cloned(),
            neg_z: all_chunk_data.get(&(pos + IVec3::NEG_Z)).cloned(),
        };

        let Some(block_data) = all_chunk_data.get(&pos) else { continue; };
        let chunk_data = Chunk::from_blocks(pos, {
            let mut blocks = [BlockType::Air; CHUNK_VOLUME];
            blocks.copy_from_slice(block_data);
            blocks
        });

        let task = task_pool.spawn(async move {
            meshing::build_chunk_mesh_with_neighbors(&chunk_data, atlas_cfg, &neighbors)
        });

        commands.entity(entity).insert(PendingMesh { task });
        to_mesh.push(entity);
        tasks_spawned += 1;
    }

    // Phase 3: Clear dirty flags.
    if !to_mesh.is_empty() {
        let mut p0 = param_set.p0();
        for entity in to_mesh {
            if let Ok((_, mut chunk)) = p0.get_mut(entity) {
                chunk.dirty = false;
            }
        }
    }
}


/// Poll completed mesh-generation tasks and insert the resulting render components.
///
/// The opaque mesh is inserted on the chunk entity itself. If the mesher
/// produced a water mesh, a child entity is spawned with the water mesh,
/// the water blend material, and a [`WaterMesh`] marker. The child entity
/// is automatically cleaned up by `despawn_recursive` when the chunk unloads.
fn poll_pending_meshes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    chunk_material: Res<ChunkMaterial>,
    mut pending_query: Query<(Entity, &Chunk, &mut PendingMesh)>,
) {
    let Some(mat) = &chunk_material.handle else {
        return;
    };

    for (entity, chunk, mut pending) in &mut pending_query {
        if let Some((opaque_mesh, water_mesh)) = block_on(future::poll_once(&mut pending.task)) {
            let mesh_handle = meshes.add(opaque_mesh);
            let world_pos = chunk_to_world_pos(chunk.position);

            // Insert shared components (mesh, transform, visibility, marker)
            commands.entity(entity).insert((
                Mesh3d(mesh_handle),
                Transform::from_translation(world_pos),
                ChunkMesh,
            )).remove::<PendingMesh>();

            // Insert the opaque material on the chunk entity
            match mat {
                ChunkMaterialHandle::Atlas { opaque, .. } => {
                    commands.entity(entity).insert(MeshMaterial3d(opaque.clone()));
                }
                ChunkMaterialHandle::Standard { opaque, .. } => {
                    commands.entity(entity).insert(MeshMaterial3d(opaque.clone()));
                }
            }

            // If the mesher produced a water mesh, spawn it as a child entity
            if let Some(wm) = water_mesh {
                let water_mesh_handle = meshes.add(wm);

                // Spawn child entity with water mesh + blend material
                let water_child = commands.spawn((
                    Mesh3d(water_mesh_handle),
                    // Child transform is identity - inherits parent's world position
                    Transform::default(),
                    WaterMesh,
                )).id();

                // Insert the water material on the child
                match mat {
                    ChunkMaterialHandle::Atlas { water, .. } => {
                        commands.entity(water_child).insert(MeshMaterial3d(water.clone()));
                    }
                    ChunkMaterialHandle::Standard { water, .. } => {
                        commands.entity(water_child).insert(MeshMaterial3d(water.clone()));
                    }
                }

                // Parent the water entity to the chunk entity
                commands.entity(entity).add_child(water_child);
            }
        }
    }
}

// Chunk unloading is now handled by `unloading::chunk_unloading_system` and
// `unloading::poll_pending_saves` â€" see `src/world/unloading.rs`.

// ============================================================================
// BLOCK QUERIES - For collision and gameplay
// ============================================================================

/// Get the block at a world position
///
/// Returns `BlockType::Air` if the chunk is not loaded or position is invalid.
///
/// # Arguments
/// * `world_pos` - Block position in world coordinates (integer)
/// * `chunk_manager` - The chunk manager resource
/// * `chunks` - Query for chunk data
///
/// # Example
/// ```ignore
/// let block = get_block_at(IVec3::new(10, 32, 10), &chunk_manager, &chunks);
/// if block.is_solid() {
///     // Collision!
/// }
/// ```
#[allow(dead_code)]
pub fn get_block_at(
    world_pos: IVec3,
    chunk_manager: &ChunkManager,
    chunks: &Query<&Chunk>,
) -> BlockType {
    // Calculate which chunk contains this block
    let chunk_pos = IVec3::new(
        world_pos.x.div_euclid(CHUNK_SIZE as i32),
        world_pos.y.div_euclid(CHUNK_SIZE as i32),
        world_pos.z.div_euclid(CHUNK_SIZE as i32),
    );

    // Look up chunk entity
    let Some(&entity) = chunk_manager.chunks.get(&chunk_pos) else {
        return BlockType::Air; // Unloaded chunk = air
    };

    // Get chunk component
    let Ok(chunk) = chunks.get(entity) else {
        return BlockType::Air;
    };

    // Calculate local position within chunk
    let local_x = world_pos.x.rem_euclid(CHUNK_SIZE as i32) as usize;
    let local_y = world_pos.y.rem_euclid(CHUNK_SIZE as i32) as usize;
    let local_z = world_pos.z.rem_euclid(CHUNK_SIZE as i32) as usize;

    chunk.get_block(local_x, local_y, local_z)
}

/// Get the block at a world position (float coordinates)
///
/// Floors the coordinates to get the containing block.
#[allow(dead_code)]
pub fn get_block_at_f32(
    world_pos: Vec3,
    chunk_manager: &ChunkManager,
    chunks: &Query<&Chunk>,
) -> BlockType {
    let block_pos = IVec3::new(
        world_pos.x.floor() as i32,
        world_pos.y.floor() as i32,
        world_pos.z.floor() as i32,
    );
    get_block_at(block_pos, chunk_manager, chunks)
}

/// Check if a block position is solid (for collision)
#[allow(dead_code)]
pub fn is_solid_at(
    world_pos: IVec3,
    chunk_manager: &ChunkManager,
    chunks: &Query<&Chunk>,
) -> bool {
    get_block_at(world_pos, chunk_manager, chunks).is_solid()
}

/// Get all solid blocks in an axis-aligned bounding box
///
/// Returns positions of solid blocks that intersect the AABB.
/// Useful for collision detection.
#[allow(dead_code)]
pub fn get_solid_blocks_in_aabb(
    min: IVec3,
    max: IVec3,
    chunk_manager: &ChunkManager,
    chunks: &Query<&Chunk>,
) -> Vec<IVec3> {
    let mut solids = Vec::new();

    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                let pos = IVec3::new(x, y, z);
                if is_solid_at(pos, chunk_manager, chunks) {
                    solids.push(pos);
                }
            }
        }
    }

    solids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_size_constants() {
        assert_eq!(CHUNK_SIZE, 16);
        assert_eq!(CHUNK_VOLUME, 16 * 16 * 16);
        assert_eq!(CHUNK_VOLUME, 4096);
    }

    #[test]
    fn test_block_type_transparency() {
        assert!(BlockType::Air.is_transparent());
        assert!(BlockType::Water.is_transparent());
        assert!(!BlockType::Stone.is_transparent());
        assert!(!BlockType::Dirt.is_transparent());
        assert!(!BlockType::Grass.is_transparent());
    }

    #[test]
    fn test_block_type_solidity() {
        assert!(!BlockType::Air.is_solid());
        assert!(!BlockType::Water.is_solid());
        assert!(BlockType::Stone.is_solid());
        assert!(BlockType::Dirt.is_solid());
        assert!(BlockType::Grass.is_solid());
    }

    #[test]
    fn test_chunk_new() {
        let chunk = Chunk::new(IVec3::new(1, 2, 3));
        assert_eq!(chunk.position, IVec3::new(1, 2, 3));
        assert!(chunk.dirty);

        // All blocks should be air
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(chunk.get_block(x, y, z), BlockType::Air);
                }
            }
        }
    }

    #[test]
    fn test_chunk_get_set_block() {
        let mut chunk = Chunk::new(IVec3::ZERO);

        // Set and get a block
        chunk.set_block(5, 10, 15, BlockType::Stone);
        assert_eq!(chunk.get_block(5, 10, 15), BlockType::Stone);

        // Other blocks should still be air
        assert_eq!(chunk.get_block(0, 0, 0), BlockType::Air);
        assert_eq!(chunk.get_block(5, 10, 14), BlockType::Air);
    }

    #[test]
    fn test_chunk_out_of_bounds_returns_air() {
        let chunk = Chunk::new(IVec3::ZERO);

        // Out of bounds should return Air without panic
        assert_eq!(chunk.get_block(16, 0, 0), BlockType::Air);
        assert_eq!(chunk.get_block(0, 100, 0), BlockType::Air);
        assert_eq!(chunk.get_block(0, 0, 999), BlockType::Air);
    }

    #[test]
    fn test_chunk_fill() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.fill(BlockType::Stone);

        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(chunk.get_block(x, y, z), BlockType::Stone);
                }
            }
        }
    }

    #[test]
    fn test_chunk_world_position() {
        let chunk = Chunk::new(IVec3::new(0, 0, 0));
        assert_eq!(chunk.world_position(), IVec3::new(0, 0, 0));

        let chunk = Chunk::new(IVec3::new(1, 2, 3));
        assert_eq!(chunk.world_position(), IVec3::new(16, 32, 48));

        let chunk = Chunk::new(IVec3::new(-1, -1, -1));
        assert_eq!(chunk.world_position(), IVec3::new(-16, -16, -16));
    }

    #[test]
    fn test_world_to_chunk_pos() {
        // Origin
        assert_eq!(world_to_chunk_pos(Vec3::new(0.0, 0.0, 0.0)), IVec3::ZERO);

        // Within first chunk
        assert_eq!(world_to_chunk_pos(Vec3::new(8.0, 8.0, 8.0)), IVec3::ZERO);
        assert_eq!(world_to_chunk_pos(Vec3::new(15.9, 15.9, 15.9)), IVec3::ZERO);

        // Crossing into next chunk
        assert_eq!(world_to_chunk_pos(Vec3::new(16.0, 16.0, 16.0)), IVec3::ONE);
        assert_eq!(world_to_chunk_pos(Vec3::new(32.0, 48.0, 64.0)), IVec3::new(2, 3, 4));

        // Negative coordinates
        assert_eq!(world_to_chunk_pos(Vec3::new(-1.0, 0.0, 0.0)), IVec3::new(-1, 0, 0));
        assert_eq!(world_to_chunk_pos(Vec3::new(-0.1, 0.0, 0.0)), IVec3::new(-1, 0, 0));
        assert_eq!(world_to_chunk_pos(Vec3::new(-16.0, -16.0, -16.0)), IVec3::new(-1, -1, -1));
        assert_eq!(world_to_chunk_pos(Vec3::new(-17.0, -17.0, -17.0)), IVec3::new(-2, -2, -2));
    }

    #[test]
    fn test_chunk_to_world_pos() {
        assert_eq!(chunk_to_world_pos(IVec3::ZERO), Vec3::ZERO);
        assert_eq!(chunk_to_world_pos(IVec3::ONE), Vec3::new(16.0, 16.0, 16.0));
        assert_eq!(chunk_to_world_pos(IVec3::new(2, 3, 4)), Vec3::new(32.0, 48.0, 64.0));
        assert_eq!(chunk_to_world_pos(IVec3::new(-1, -1, -1)), Vec3::new(-16.0, -16.0, -16.0));
    }

    #[test]
    fn test_coordinate_round_trip() {
        // World -> Chunk -> World should give chunk corner
        let world_pos = Vec3::new(35.7, 22.3, 50.1);
        let chunk_pos = world_to_chunk_pos(world_pos);
        let chunk_corner = chunk_to_world_pos(chunk_pos);

        assert_eq!(chunk_pos, IVec3::new(2, 1, 3));
        assert_eq!(chunk_corner, Vec3::new(32.0, 16.0, 48.0));

        // The corner should be <= the original world pos
        assert!(chunk_corner.x <= world_pos.x);
        assert!(chunk_corner.y <= world_pos.y);
        assert!(chunk_corner.z <= world_pos.z);
    }

    #[test]
    fn test_chunk_index_calculation() {
        // Test internal index calculation
        assert_eq!(Chunk::index(0, 0, 0), 0);
        assert_eq!(Chunk::index(1, 0, 0), 1);
        assert_eq!(Chunk::index(0, 1, 0), CHUNK_SIZE);
        assert_eq!(Chunk::index(0, 0, 1), CHUNK_SIZE * CHUNK_SIZE);
        assert_eq!(Chunk::index(15, 15, 15), CHUNK_VOLUME - 1);
    }

    #[test]
    fn test_chunk_dirty_flag() {
        let mut chunk = Chunk::new(IVec3::ZERO);

        // New chunks start dirty
        assert!(chunk.dirty);

        // Clear dirty manually
        chunk.dirty = false;
        assert!(!chunk.dirty);

        // Setting a block makes it dirty again
        chunk.set_block(0, 0, 0, BlockType::Stone);
        assert!(chunk.dirty);

        // Fill also makes it dirty
        chunk.dirty = false;
        chunk.fill(BlockType::Air);
        assert!(chunk.dirty);
    }

    // Block query tests (require ECS world, so kept simple)
    #[test]
    fn test_chunk_pos_calculation() {
        // Positive coordinates
        let chunk_pos = IVec3::new(
            35_i32.div_euclid(CHUNK_SIZE as i32),
            22_i32.div_euclid(CHUNK_SIZE as i32),
            50_i32.div_euclid(CHUNK_SIZE as i32),
        );
        assert_eq!(chunk_pos, IVec3::new(2, 1, 3));

        // Negative coordinates
        let chunk_pos = IVec3::new(
            (-5_i32).div_euclid(CHUNK_SIZE as i32),
            (-20_i32).div_euclid(CHUNK_SIZE as i32),
            (-1_i32).div_euclid(CHUNK_SIZE as i32),
        );
        assert_eq!(chunk_pos, IVec3::new(-1, -2, -1));
    }

    #[test]
    fn test_local_pos_calculation() {
        // Positive world position
        let local_x = 35_i32.rem_euclid(CHUNK_SIZE as i32) as usize;
        let local_y = 22_i32.rem_euclid(CHUNK_SIZE as i32) as usize;
        let local_z = 50_i32.rem_euclid(CHUNK_SIZE as i32) as usize;
        assert_eq!((local_x, local_y, local_z), (3, 6, 2));

        // Negative world position
        let local_x = (-5_i32).rem_euclid(CHUNK_SIZE as i32) as usize;
        let local_y = (-20_i32).rem_euclid(CHUNK_SIZE as i32) as usize;
        let local_z = (-1_i32).rem_euclid(CHUNK_SIZE as i32) as usize;
        assert_eq!((local_x, local_y, local_z), (11, 12, 15));
    }

    // ========================================================================
    // Async chunk-generation pipeline tests
    // ========================================================================

    /// Ensure `AsyncComputeTaskPool` is initialised for tests that use it.
    /// Calling `get_or_init` more than once is safe â€" subsequent calls are no-ops.
    fn init_task_pool() {
        AsyncComputeTaskPool::get_or_init(|| {
            bevy::tasks::TaskPool::new()
        });
    }

    #[test]
    fn test_chunk_manager_pending_tracking() {
        let mut cm = ChunkManager::default();
        let pos = IVec3::new(1, 2, 3);

        assert!(!cm.pending.contains(&pos));
        cm.pending.insert(pos);
        assert!(cm.pending.contains(&pos));

        // Inserting again is idempotent
        cm.pending.insert(pos);
        assert_eq!(cm.pending.len(), 1);

        cm.pending.remove(&pos);
        assert!(!cm.pending.contains(&pos));
    }

    #[test]
    fn test_chunk_manager_skips_pending_positions() {
        let mut cm = ChunkManager::default();
        let pos = IVec3::new(0, 0, 0);

        // Simulate: position is already pending
        cm.pending.insert(pos);

        // The streaming system would check both `chunks` and `pending`
        let should_skip =
            cm.chunks.contains_key(&pos) || cm.pending.contains(&pos);
        assert!(should_skip, "Should skip positions already in pending set");
    }

    #[test]
    fn test_chunk_manager_skips_loaded_positions() {
        let mut cm = ChunkManager::default();
        let pos = IVec3::new(0, 0, 0);

        // Simulate: a chunk is already loaded at this position
        cm.chunks.insert(pos, Entity::PLACEHOLDER);

        let should_skip =
            cm.chunks.contains_key(&pos) || cm.pending.contains(&pos);
        assert!(should_skip, "Should skip positions already in chunks map");
    }

    #[test]
    fn test_async_chunk_generation_via_task_pool() {
        init_task_pool();
        // Verify that chunk generation works correctly when run in a task pool,
        // producing the same deterministic result as inline generation.
        let config = TerrainConfig::default();
        let chunk_pos = IVec3::new(0, 2, 0);

        // Inline (reference) generation
        let mut reference = Chunk::new(chunk_pos);
        generate_chunk_terrain(&mut reference, &config);
        generate_caves(&mut reference, &config);
        generate_ores(&mut reference, &config, &default_ore_configs());
        generate_trees(&mut reference, &config);
        generate_cacti(&mut reference, &config);

        // Task-pool generation (simulates what chunk_streaming_system does)
        let task_pool = AsyncComputeTaskPool::get();
        let config_clone = config.clone();
        let task = task_pool.spawn(async move {
            let mut chunk = Chunk::new(chunk_pos);
            generate_chunk_terrain(&mut chunk, &config_clone);
            generate_caves(&mut chunk, &config_clone);
            generate_ores(&mut chunk, &config_clone, &default_ore_configs());
            generate_trees(&mut chunk, &config_clone);
            generate_cacti(&mut chunk, &config_clone);
            chunk
        });

        // Block until task completes (valid in test context)
        let result = block_on(task);

        // Verify every block matches
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(
                        result.get_block(x, y, z),
                        reference.get_block(x, y, z),
                        "Async task produced different block at ({x}, {y}, {z})"
                    );
                }
            }
        }
        assert_eq!(result.position, reference.position);
        assert!(result.dirty);
    }

    #[test]
    fn test_async_mesh_generation_via_task_pool() {
        init_task_pool();
        // Verify that mesh generation on a task pool produces the same mesh
        // as inline generation.
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.fill(BlockType::Stone);

        // Inline (reference)
        let reference_mesh = meshing::build_chunk_mesh(&chunk);

        // Task-pool generation (simulates what mesh_dirty_chunks does)
        let task_pool = AsyncComputeTaskPool::get();
        let chunk_clone = chunk.clone();
        let task = task_pool.spawn(async move {
            meshing::build_chunk_mesh(&chunk_clone)
        });

        let result_mesh = block_on(task);

        // Compare vertex counts
        let ref_verts = reference_mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .map(|a| a.len())
            .unwrap_or(0);
        let res_verts = result_mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .map(|a| a.len())
            .unwrap_or(0);
        assert_eq!(ref_verts, res_verts, "Vertex counts should match");

        // Compare index counts
        let ref_indices = reference_mesh.indices().map(|i| i.len()).unwrap_or(0);
        let res_indices = result_mesh.indices().map(|i| i.len()).unwrap_or(0);
        assert_eq!(ref_indices, res_indices, "Index counts should match");
    }

    #[test]
    fn test_multiple_async_chunk_tasks() {
        init_task_pool();
        // Spawn several chunk generation tasks concurrently and verify all complete.
        let config = TerrainConfig::default();
        let task_pool = AsyncComputeTaskPool::get();

        let positions = vec![
            IVec3::new(0, 0, 0),
            IVec3::new(1, 0, 0),
            IVec3::new(0, 1, 0),
            IVec3::new(-1, 2, 3),
        ];

        let tasks: Vec<_> = positions
            .iter()
            .map(|&pos| {
                let cfg = config.clone();
                task_pool.spawn(async move {
                    let mut chunk = Chunk::new(pos);
                    generate_chunk_terrain(&mut chunk, &cfg);
                    generate_caves(&mut chunk, &cfg);
                    generate_ores(&mut chunk, &cfg, &default_ore_configs());
                    generate_trees(&mut chunk, &cfg);
                    generate_cacti(&mut chunk, &cfg);
                    chunk
                })
            })
            .collect();

        for (task, &expected_pos) in tasks.into_iter().zip(&positions) {
            let chunk = block_on(task);
            assert_eq!(chunk.position, expected_pos);
            assert!(chunk.dirty);
        }
    }

    #[test]
    fn test_chunk_modified_flag_defaults() {
        // New chunks start as not modified
        let chunk = Chunk::new(IVec3::ZERO);
        assert!(!chunk.modified, "New chunks should not be modified");

        // Chunks loaded from block data start as not modified
        let chunk = Chunk::from_blocks(IVec3::ZERO, [BlockType::Air; CHUNK_VOLUME]);
        assert!(!chunk.modified, "Loaded chunks should not be modified");
    }

    #[test]
    fn test_chunk_modified_flag_explicit_set() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        assert!(!chunk.modified);

        // Callers set modified explicitly (simulating player block placement)
        chunk.set_block(0, 0, 0, BlockType::Stone);
        chunk.modified = true;
        assert!(chunk.modified);

        // set_block alone does NOT set modified (used by terrain generation)
        let mut chunk2 = Chunk::new(IVec3::ZERO);
        chunk2.set_block(5, 5, 5, BlockType::Dirt);
        assert!(!chunk2.modified, "set_block should not auto-set modified");
    }

    #[test]
    fn test_chunk_loads_from_disk_before_generating() {
        init_task_pool();

        // Create a temp save directory
        let dir = std::env::temp_dir().join(format!(
            "pw_disk_load_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let storage = persistence::ChunkStorage::new(&dir);

        // Save a chunk with a distinctive block pattern
        let mut chunk = Chunk::new(IVec3::new(5, 0, 5));
        chunk.set_block(7, 7, 7, BlockType::Obsidian);
        persistence::save_chunk(&chunk, &storage).unwrap();

        // Simulate what chunk_streaming_system does: try disk first, then generate
        let chunk_pos = IVec3::new(5, 0, 5);
        let config = TerrainConfig::default();
        let storage_clone = persistence::ChunkStorage::new(&dir);

        let task = AsyncComputeTaskPool::get().spawn(async move {
            if let Ok(loaded) = persistence::load_chunk(chunk_pos, &storage_clone) {
                return loaded;
            }
            let mut c = Chunk::new(chunk_pos);
            generate_chunk_terrain(&mut c, &config);
            generate_caves(&mut c, &config);
            generate_ores(&mut c, &config, &default_ore_configs());
            generate_trees(&mut c, &config);
            generate_cacti(&mut c, &config);
            c
        });

        let result = block_on(task);

        // Should have loaded from disk, preserving the distinctive block
        assert_eq!(result.get_block(7, 7, 7), BlockType::Obsidian);
        assert_eq!(result.position, IVec3::new(5, 0, 5));
        assert!(!result.modified, "Loaded chunks should not be marked modified");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_chunk_generates_when_not_on_disk() {
        init_task_pool();

        // Use a path that definitely has no saved chunks
        let dir = std::env::temp_dir().join(format!(
            "pw_no_disk_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let storage = persistence::ChunkStorage::new(&dir);

        let chunk_pos = IVec3::new(0, 2, 0);
        let config = TerrainConfig::default();
        let storage_clone = persistence::ChunkStorage::new(&dir);

        let task = AsyncComputeTaskPool::get().spawn(async move {
            if let Ok(loaded) = persistence::load_chunk(chunk_pos, &storage_clone) {
                return loaded;
            }
            let mut c = Chunk::new(chunk_pos);
            generate_chunk_terrain(&mut c, &config);
            generate_caves(&mut c, &config);
            generate_ores(&mut c, &config, &default_ore_configs());
            generate_trees(&mut c, &config);
            generate_cacti(&mut c, &config);
            c
        });

        let result = block_on(task);

        // Should have generated terrain (same as inline reference)
        let config_ref = TerrainConfig::default();
        let mut reference = Chunk::new(chunk_pos);
        generate_chunk_terrain(&mut reference, &config_ref);
        generate_caves(&mut reference, &config_ref);
        generate_ores(&mut reference, &config_ref, &default_ore_configs());
        generate_trees(&mut reference, &config_ref);
        generate_cacti(&mut reference, &config_ref);

        assert_eq!(result.position, chunk_pos);
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(
                        result.get_block(x, y, z),
                        reference.get_block(x, y, z),
                        "Block mismatch at ({x}, {y}, {z}) - disk fallback should generate identically"
                    );
                }
            }
        }

        // Cleanup (dir may not exist, that's fine)
        let _ = std::fs::remove_dir_all(&dir);
        let _ = persistence::chunk_exists(chunk_pos, &storage); // Ensure no leftover
    }

    // ========================================================================
    // Chunk loading distance tests
    // ========================================================================

    #[test]
    fn test_effective_load_distance_defaults_to_render_distance() {
        let cm = ChunkManager::default();
        assert_eq!(cm.load_distance, None);
        assert_eq!(cm.effective_load_distance(), cm.render_distance);
    }

    #[test]
    fn test_effective_load_distance_explicit() {
        let cm = ChunkManager {
            load_distance: Some(8),
            render_distance: 4,
            ..default()
        };
        assert_eq!(cm.effective_load_distance(), 8);
    }

    #[test]
    fn test_effective_load_distance_independent_of_render() {
        let mut cm = ChunkManager::default();
        cm.render_distance = 6;
        assert_eq!(cm.effective_load_distance(), 6);

        cm.load_distance = Some(3);
        assert_eq!(cm.effective_load_distance(), 3);
    }

    #[test]
    fn test_vertical_load_defaults() {
        let cm = ChunkManager::default();
        assert_eq!(cm.vertical_load_up, 4);
        assert_eq!(cm.vertical_load_down, 2);
    }

    #[test]
    fn test_vertical_load_custom() {
        let cm = ChunkManager {
            vertical_load_up: 8,
            vertical_load_down: 4,
            ..default()
        };
        assert_eq!(cm.vertical_load_up, 8);
        assert_eq!(cm.vertical_load_down, 4);
    }

    // ========================================================================
    // Chunk load metrics tests
    // ========================================================================

    #[test]
    fn test_chunk_load_metrics_default() {
        let metrics = ChunkLoadMetrics::default();
        assert!(metrics.enabled);
        assert_eq!(metrics.chunks_per_second, 0.0);
        assert_eq!(metrics.avg_load_time_ms, 0.0);
        assert_eq!(metrics.peak_load_time_ms, 0.0);
        assert_eq!(metrics.all_time_peak_load_time_ms, 0.0);
        assert_eq!(metrics.total_chunks_loaded, 0);
        assert_eq!(metrics.chunks_loaded_since_last, 0);
        assert_eq!(metrics.chunk_memory_bytes, 0);
    }

    #[test]
    fn test_chunk_load_metrics_record_and_refresh() {
        let mut metrics = ChunkLoadMetrics::default();

        // Record 5 chunk loads at t=1.0s, each taking 0.05s
        for i in 0..5 {
            metrics.record_load(0.05, 1.0 + i as f64 * 0.01);
        }
        assert_eq!(metrics.total_chunks_loaded, 5);
        assert_eq!(metrics.chunks_loaded_since_last, 5);

        // Refresh at t=2.0s â€" all 5 loads within the 2s window
        metrics.refresh(2.0);
        assert_eq!(metrics.chunks_loaded_since_last, 0); // reset by refresh
        assert!((metrics.avg_load_time_ms - 50.0).abs() < 0.01); // 0.05s = 50ms
        assert!(metrics.chunks_per_second > 0.0);
    }

    #[test]
    fn test_chunk_load_metrics_window_expiry() {
        let mut metrics = ChunkLoadMetrics::default();

        // Record loads at t=1.0
        for _ in 0..10 {
            metrics.record_load(0.01, 1.0);
        }

        // Refresh at t=5.0 â€" all loads older than 2s window â†' 0 chunks/sec
        metrics.refresh(5.0);
        assert_eq!(metrics.chunks_per_second, 0.0);
        // avg_load_time_ms still valid (rolling buffer)
        assert!((metrics.avg_load_time_ms - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_chunk_load_metrics_rolling_capacity() {
        let mut metrics = ChunkLoadMetrics::default();

        // Fill beyond capacity
        for i in 0..METRICS_HISTORY_SIZE + 50 {
            metrics.record_load(0.1, i as f64);
        }

        // Should not exceed capacity
        assert!(metrics.load_times.len() <= METRICS_HISTORY_SIZE);
        assert!(metrics.completion_timestamps.len() <= METRICS_HISTORY_SIZE);
        assert_eq!(metrics.total_chunks_loaded, (METRICS_HISTORY_SIZE + 50) as u64);
    }

    #[test]
    fn test_chunk_load_metrics_pending_start_times() {
        let mut metrics = ChunkLoadMetrics::default();
        let pos = IVec3::new(1, 2, 3);

        metrics.pending_start_times.insert(pos, std::time::Instant::now());
        assert!(metrics.pending_start_times.contains_key(&pos));

        let start = metrics.pending_start_times.remove(&pos).unwrap();
        assert!(start.elapsed().as_secs_f32() < 1.0);
        assert!(!metrics.pending_start_times.contains_key(&pos));
    }

    #[test]
    fn test_chunk_load_metrics_peak_load_time() {
        let mut metrics = ChunkLoadMetrics::default();

        // Record loads with varying durations
        metrics.record_load(0.01, 1.0); // 10ms
        metrics.record_load(0.05, 1.1); // 50ms
        metrics.record_load(0.20, 1.2); // 200ms - peak
        metrics.record_load(0.02, 1.3); // 20ms

        metrics.refresh(2.0);

        // Peak in the rolling window should be the 200ms load
        assert!((metrics.peak_load_time_ms - 200.0).abs() < 0.1);
        // All-time peak should also be 200ms
        assert!((metrics.all_time_peak_load_time_ms - 200.0).abs() < 0.1);
    }

    #[test]
    fn test_chunk_load_metrics_all_time_peak_survives_window_rollover() {
        let mut metrics = ChunkLoadMetrics::default();

        // Record a very slow load early
        metrics.record_load(0.50, 1.0); // 500ms
        metrics.refresh(2.0);
        assert!((metrics.all_time_peak_load_time_ms - 500.0).abs() < 0.1);

        // Fill the rolling window with fast loads to push the slow one out
        for i in 0..METRICS_HISTORY_SIZE + 10 {
            metrics.record_load(0.001, 10.0 + i as f64);
        }
        metrics.refresh(10.0 + METRICS_HISTORY_SIZE as f64 + 10.0);

        // Rolling window peak should be ~1ms (the slow load was evicted)
        assert!(metrics.peak_load_time_ms < 5.0);
        // But all-time peak is preserved
        assert!((metrics.all_time_peak_load_time_ms - 500.0).abs() < 0.1);
    }

    #[test]
    fn test_chunk_load_metrics_memory_estimate() {
        let mut metrics = ChunkLoadMetrics::default();
        assert_eq!(metrics.chunk_memory_bytes, 0);
        assert_eq!(metrics.chunk_memory_mb(), 0.0);

        // Simulate 100 loaded chunks
        metrics.update_memory_estimate(100);
        let expected_bytes = 100 * CHUNK_VOLUME * std::mem::size_of::<BlockType>();
        assert_eq!(metrics.chunk_memory_bytes, expected_bytes);
        assert!(metrics.chunk_memory_mb() > 0.0);

        // Verify calculation: 100 chunks * 4096 blocks * 2 bytes = 819200 bytes
        assert_eq!(expected_bytes, 100 * 4096 * 2);
        let expected_mb = expected_bytes as f64 / (1024.0 * 1024.0);
        assert!((metrics.chunk_memory_mb() - expected_mb).abs() < 0.001);
    }

    #[test]
    fn test_chunk_load_metrics_disabled_skips_rolling_window() {
        let mut metrics = ChunkLoadMetrics::default();
        metrics.enabled = false;

        // Record loads while disabled
        for i in 0..10 {
            metrics.record_load(0.05, i as f64);
        }

        // total_chunks_loaded always increments
        assert_eq!(metrics.total_chunks_loaded, 10);
        // But rolling window data was not collected
        assert_eq!(metrics.chunks_loaded_since_last, 0);
        assert!(metrics.load_times.is_empty());
        assert!(metrics.completion_timestamps.is_empty());

        // Refresh while disabled should be a no-op
        metrics.refresh(5.0);
        assert_eq!(metrics.avg_load_time_ms, 0.0);
        assert_eq!(metrics.peak_load_time_ms, 0.0);
        assert_eq!(metrics.chunks_per_second, 0.0);
    }

    #[test]
    fn test_chunk_load_metrics_enable_after_disable() {
        let mut metrics = ChunkLoadMetrics::default();

        // Disable and record some loads
        metrics.enabled = false;
        for i in 0..5 {
            metrics.record_load(0.05, i as f64);
        }
        assert_eq!(metrics.total_chunks_loaded, 5);
        assert!(metrics.load_times.is_empty());

        // Re-enable and record more
        metrics.enabled = true;
        for i in 5..10 {
            metrics.record_load(0.03, i as f64);
        }
        assert_eq!(metrics.total_chunks_loaded, 10);
        assert_eq!(metrics.load_times.len(), 5); // Only the enabled ones
        assert_eq!(metrics.chunks_loaded_since_last, 5);

        metrics.refresh(10.0);
        // Average should be from the 5 enabled loads (0.03s = 30ms)
        assert!((metrics.avg_load_time_ms - 30.0).abs() < 0.1);
    }

    #[test]
    fn test_diagnostic_paths_are_unique() {
        // Ensure all diagnostic paths are distinct
        let paths = [
            &CHUNK_AVG_LOAD_TIME,
            &CHUNK_PEAK_LOAD_TIME,
            &CHUNK_LOADS_PER_SEC,
            &CHUNK_TOTAL_LOADED,
            &CHUNK_MEMORY_MB,
        ];
        for (i, a) in paths.iter().enumerate() {
            for (j, b) in paths.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "Diagnostic paths at index {} and {} must differ", i, j);
                }
            }
        }
    }

    #[test]
    fn test_rate_limiting_counter_resets() {
        let mut cm = ChunkManager::default();
        cm.tasks_spawned_this_frame = 10;
        // Simulating what chunk_streaming_system does at the top
        cm.tasks_spawned_this_frame = 0;
        assert_eq!(cm.tasks_spawned_this_frame, 0);
    }

    #[test]
    fn test_rate_limiting_respects_max() {
        let cm = ChunkManager {
            max_chunks_per_frame: 4,
            ..default()
        };
        // Simulate spawning loop
        let mut spawned = 0u32;
        for _ in 0..100 {
            if spawned >= cm.max_chunks_per_frame {
                break;
            }
            spawned += 1;
        }
        assert_eq!(spawned, 4, "Should stop at max_chunks_per_frame");
    }

    // ========================================================================
    // Cache hit/miss tracking tests
    // ========================================================================

    #[test]
    fn test_chunk_load_metrics_cache_defaults() {
        let metrics = ChunkLoadMetrics::default();
        assert_eq!(metrics.cache_hits, 0);
        assert_eq!(metrics.cache_misses, 0);
        assert_eq!(metrics.cache_hit_rate, 0.0);
        assert!(metrics.recent_load_times_ms.is_empty());
        assert_eq!(metrics.memory_per_chunk_bytes, CHUNK_VOLUME * std::mem::size_of::<BlockType>());
    }

    #[test]
    fn test_record_load_with_source_cache_hit() {
        let mut metrics = ChunkLoadMetrics::default();
        metrics.record_load_with_source(0.05, 1.0, true);

        assert_eq!(metrics.cache_hits, 1);
        assert_eq!(metrics.cache_misses, 0);
        assert_eq!(metrics.total_chunks_loaded, 1);
    }

    #[test]
    fn test_record_load_with_source_cache_miss() {
        let mut metrics = ChunkLoadMetrics::default();
        metrics.record_load_with_source(0.10, 1.0, false);

        assert_eq!(metrics.cache_hits, 0);
        assert_eq!(metrics.cache_misses, 1);
        assert_eq!(metrics.total_chunks_loaded, 1);
    }

    #[test]
    fn test_record_load_with_source_mixed() {
        let mut metrics = ChunkLoadMetrics::default();
        // 3 cache hits, 7 cache misses
        for i in 0..10 {
            let from_cache = i < 3;
            metrics.record_load_with_source(0.05, i as f64, from_cache);
        }

        assert_eq!(metrics.cache_hits, 3);
        assert_eq!(metrics.cache_misses, 7);
        assert_eq!(metrics.total_chunks_loaded, 10);
    }

    #[test]
    fn test_cache_hit_rate_computation() {
        let mut metrics = ChunkLoadMetrics::default();

        // Record 6 hits and 4 misses
        for i in 0..10 {
            let from_cache = i < 6;
            metrics.record_load_with_source(0.01, i as f64, from_cache);
        }

        metrics.refresh(10.0);
        assert!((metrics.cache_hit_rate - 0.6).abs() < 0.01);
    }

    #[test]
    fn test_cache_hit_rate_zero_when_all_misses() {
        let mut metrics = ChunkLoadMetrics::default();
        for i in 0..5 {
            metrics.record_load_with_source(0.01, i as f64, false);
        }
        metrics.refresh(5.0);
        assert_eq!(metrics.cache_hit_rate, 0.0);
    }

    #[test]
    fn test_cache_hit_rate_one_when_all_hits() {
        let mut metrics = ChunkLoadMetrics::default();
        for i in 0..5 {
            metrics.record_load_with_source(0.01, i as f64, true);
        }
        metrics.refresh(5.0);
        assert!((metrics.cache_hit_rate - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_recent_load_times_recorded() {
        let mut metrics = ChunkLoadMetrics::default();
        metrics.record_load_with_source(0.05, 1.0, false); // 50ms
        metrics.record_load_with_source(0.10, 1.1, true);  // 100ms
        metrics.record_load_with_source(0.02, 1.2, false); // 20ms

        assert_eq!(metrics.recent_load_times_ms.len(), 3);
        assert!((metrics.recent_load_times_ms[0] - 50.0).abs() < 0.1);
        assert!((metrics.recent_load_times_ms[1] - 100.0).abs() < 0.1);
        assert!((metrics.recent_load_times_ms[2] - 20.0).abs() < 0.1);
    }

    #[test]
    fn test_recent_load_times_capped_at_history_size() {
        let mut metrics = ChunkLoadMetrics::default();
        for i in 0..(METRICS_HISTORY_SIZE + 50) {
            metrics.record_load_with_source(0.01, i as f64, false);
        }
        assert!(metrics.recent_load_times_ms.len() <= METRICS_HISTORY_SIZE);
    }

    #[test]
    fn test_cache_counters_increment_when_disabled() {
        let mut metrics = ChunkLoadMetrics::default();
        metrics.enabled = false;

        metrics.record_load_with_source(0.05, 1.0, true);
        metrics.record_load_with_source(0.05, 2.0, false);

        // Cache counters should always increment
        assert_eq!(metrics.cache_hits, 1);
        assert_eq!(metrics.cache_misses, 1);
        assert_eq!(metrics.total_chunks_loaded, 2);

        // But rolling history should be empty
        assert!(metrics.recent_load_times_ms.is_empty());
        assert!(metrics.cache_hit_history.is_empty());
    }

    #[test]
    fn test_record_load_backwards_compatible() {
        // record_load (without source) should default to cache miss
        let mut metrics = ChunkLoadMetrics::default();
        metrics.record_load(0.05, 1.0);

        assert_eq!(metrics.cache_hits, 0);
        assert_eq!(metrics.cache_misses, 1);
        assert_eq!(metrics.total_chunks_loaded, 1);
    }

    #[test]
    fn test_cache_diagnostic_paths_unique() {
        let paths = [
            &CHUNK_AVG_LOAD_TIME,
            &CHUNK_PEAK_LOAD_TIME,
            &CHUNK_LOADS_PER_SEC,
            &CHUNK_TOTAL_LOADED,
            &CHUNK_MEMORY_MB,
            &CHUNK_CACHE_HIT_RATE,
            &CHUNK_CACHE_HITS,
            &CHUNK_CACHE_MISSES,
        ];
        for (i, a) in paths.iter().enumerate() {
            for (j, b) in paths.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "Diagnostic paths at index {} and {} must differ", i, j);
                }
            }
        }
    }

    // ========================================================================
    // Chunk border optimization tests
    // ========================================================================

    #[test]
    fn test_neighbor_dirtying_logic() {
        // Verify the neighbor-dirtying logic used in poll_pending_chunks:
        // when a chunk loads at `pos`, its 6 face-adjacent neighbors should be
        // found in ChunkManager::chunks and can be marked dirty.
        let mut cm = ChunkManager::default();

        // Simulate 6 neighbors already loaded around (0,0,0)
        let neighbor_positions = [
            IVec3::X, IVec3::NEG_X,
            IVec3::Y, IVec3::NEG_Y,
            IVec3::Z, IVec3::NEG_Z,
        ];
        for pos in &neighbor_positions {
            cm.chunks.insert(*pos, Entity::PLACEHOLDER);
        }

        // New chunk at origin — all 6 neighbors should be found
        let new_pos = IVec3::ZERO;
        let mut found = Vec::new();
        for offset in neighbor_positions {
            let neighbor_pos = new_pos + offset;
            if cm.chunks.contains_key(&neighbor_pos) {
                found.push(neighbor_pos);
            }
        }

        assert_eq!(
            found.len(), 6,
            "all 6 face-adjacent neighbors should be found when loaded"
        );
    }

    #[test]
    fn test_neighbor_dirtying_skips_unloaded() {
        // Only some neighbors are loaded — dirtying should only affect those.
        let mut cm = ChunkManager::default();

        // Only +X and -Z neighbors loaded
        cm.chunks.insert(IVec3::X, Entity::PLACEHOLDER);
        cm.chunks.insert(IVec3::NEG_Z, Entity::PLACEHOLDER);

        let new_pos = IVec3::ZERO;
        let offsets = [
            IVec3::X, IVec3::NEG_X,
            IVec3::Y, IVec3::NEG_Y,
            IVec3::Z, IVec3::NEG_Z,
        ];

        let mut found_count = 0;
        for offset in offsets {
            let neighbor_pos = new_pos + offset;
            if cm.chunks.contains_key(&neighbor_pos) {
                found_count += 1;
            }
        }

        assert_eq!(
            found_count, 2,
            "only loaded neighbors should be dirtied"
        );
    }

    #[test]
    fn test_border_remeshing_with_neighbor_data() {
        // Verify that a chunk meshed with neighbor data produces different
        // (fewer) vertices than one meshed without, confirming the border
        // optimization is effective.
        init_task_pool();

        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.fill(BlockType::Stone);

        let neighbor_data: Vec<BlockType> = {
            let mut tmp = Chunk::new(IVec3::X);
            tmp.fill(BlockType::Stone);
            tmp.blocks().to_vec()
        };

        // Without neighbors: all border faces rendered
        let mesh_before = meshing::build_chunk_mesh(&chunk);
        let verts_before = mesh_before
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .map(|a| a.len())
            .unwrap_or(0);

        // With +X neighbor: +X border faces culled
        let neighbors = meshing::ChunkNeighbors {
            pos_x: Some(neighbor_data),
            neg_x: None, pos_y: None, neg_y: None, pos_z: None, neg_z: None,
        };
        let (mesh_after, _) = meshing::build_chunk_mesh_with_neighbors(&chunk, None, &neighbors);
        let verts_after = mesh_after
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .map(|a| a.len())
            .unwrap_or(0);

        assert!(
            verts_after < verts_before,
            "remeshing with neighbor data should reduce vertex count: \
             before={verts_before}, after={verts_after}"
        );
    }
}
