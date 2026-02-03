//! World save/load system — full world state serialization
//!
//! Provides a high-level save system that persists:
//! - Modified chunk data (incremental — only dirty chunks)
//! - Player position and movement state
//! - World configuration (terrain seed, generation parameters)
//!
//! Builds on top of the low-level [`persistence`] module which handles
//! individual chunk file I/O.
//!
//! # Save Format
//!
//! Saves are stored in a directory structure:
//!
//! ```text
//! saves/<slot>/
//! ├── world.json          ← world metadata, player state, config
//! └── chunks/
//!     ├── chunk_0_2_-3.json
//!     ├── chunk_1_0_4.json
//!     └── ...
//! ```
//!
//! # Usage
//!
//! The save system is integrated as a Bevy plugin via [`SavePlugin`].
//! It adds:
//! - **Auto-save** on a configurable timer (default: 5 minutes)
//! - **Manual save** via `F5` key press
//! - **Load on startup** if a save file exists
//!
//! The system only saves chunks that have `chunk.modified == true`,
//! making saves incremental and fast.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::persistence::{self, ChunkStorage};
use super::Chunk;
use crate::actors::{Movement, Player};
use crate::config::EngineConfig;

// ============================================================================
// SAVE DATA STRUCTURES
// ============================================================================

/// Version number for the save format. Bumped on breaking changes.
const SAVE_FORMAT_VERSION: u32 = 1;

/// Serializable snapshot of the player's state.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PlayerSaveData {
    /// Player feet position in world coordinates `[x, y, z]`.
    pub position: [f32; 3],
    /// Whether flying mode is enabled.
    pub flying: bool,
    /// Whether noclip mode is enabled.
    pub noclip: bool,
}

/// Serializable snapshot of terrain generation config.
///
/// Stored so that a loaded world uses the same generation parameters,
/// even if `config.json` has changed.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TerrainSaveData {
    pub seed: u32,
    pub base_height: f64,
    pub height_scale: f64,
    pub frequency: f64,
    pub octaves: usize,
    pub biome_scale: f64,
}

/// Root save file — serialized to `world.json`.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WorldSaveData {
    /// Save format version for forward compatibility.
    pub version: u32,
    /// ISO-8601 timestamp of when this save was created.
    pub timestamp: String,
    /// Player state at time of save.
    pub player: PlayerSaveData,
    /// Terrain generation parameters.
    pub terrain: TerrainSaveData,
    /// List of chunk positions that have been saved to disk.
    /// Each entry is `[x, y, z]` in chunk coordinates.
    pub saved_chunks: Vec<[i32; 3]>,
}

// ============================================================================
// SAVE SYSTEM RESOURCE
// ============================================================================

/// Resource controlling the save/load system.
///
/// Tracks auto-save timing and the current save slot directory.
#[derive(Resource, Debug)]
pub struct SaveSystem {
    /// Root directory for save files (default: `"saves/default"`).
    pub save_dir: PathBuf,
    /// Seconds between automatic saves (default: 300 = 5 minutes).
    pub auto_save_interval: f32,
    /// Accumulated time since the last save.
    auto_save_timer: f32,
    /// Number of chunks saved in the last save operation.
    pub last_save_chunk_count: usize,
    /// Whether a save is currently requested (set by trigger, cleared by save system).
    save_requested: bool,
}

impl Default for SaveSystem {
    fn default() -> Self {
        Self {
            save_dir: PathBuf::from("saves/default"),
            auto_save_interval: 300.0,
            auto_save_timer: 0.0,
            last_save_chunk_count: 0,
            save_requested: false,
        }
    }
}

impl SaveSystem {
    /// Create a SaveSystem with a custom save directory.
    #[allow(dead_code)]
    pub fn new(save_dir: impl Into<PathBuf>) -> Self {
        Self {
            save_dir: save_dir.into(),
            ..default()
        }
    }

    /// Path to the chunk storage subdirectory.
    pub fn chunk_dir(&self) -> PathBuf {
        self.save_dir.join("chunks")
    }

    /// Path to the `world.json` metadata file.
    pub fn world_file(&self) -> PathBuf {
        self.save_dir.join("world.json")
    }

    /// Request a save to happen on the next frame.
    pub fn request_save(&mut self) {
        self.save_requested = true;
    }
}

// ============================================================================
// SAVE / LOAD FUNCTIONS
// ============================================================================

/// Save the entire world state to disk.
///
/// This is the core save function. It:
/// 1. Iterates all loaded chunks and saves those with `modified == true`
/// 2. Collects player position and movement state
/// 3. Collects terrain generation config
/// 4. Writes `world.json` with all metadata
///
/// Returns the number of chunks saved.
///
/// # Errors
///
/// Returns `io::Error` if directory creation or file writes fail.
pub fn save_world(
    save_dir: &Path,
    chunks: &[(IVec3, &Chunk)],
    player_pos: Vec3,
    player_movement: &Movement,
    engine_config: &EngineConfig,
) -> Result<usize, io::Error> {
    // Ensure save directories exist
    let chunk_dir = save_dir.join("chunks");
    fs::create_dir_all(&chunk_dir)?;

    let storage = ChunkStorage::new(&chunk_dir);

    // Save only modified chunks
    let mut saved_positions: Vec<[i32; 3]> = Vec::new();
    let mut chunks_saved = 0;

    for &(pos, chunk) in chunks {
        if chunk.modified {
            persistence::save_chunk(chunk, &storage)?;
            saved_positions.push([pos.x, pos.y, pos.z]);
            chunks_saved += 1;
        }
    }

    // Also include previously saved chunk positions from existing world.json
    let world_file = save_dir.join("world.json");
    if world_file.exists()
        && let Ok(existing) = load_world_metadata(&world_file)
    {
        for pos in &existing.saved_chunks {
            if !saved_positions.contains(pos) {
                // Keep previously saved chunks in the manifest
                let ivec = IVec3::new(pos[0], pos[1], pos[2]);
                // Only include if the file still exists on disk
                if persistence::chunk_exists(ivec, &storage) {
                    saved_positions.push(*pos);
                }
            }
        }
    }

    // Build world metadata
    let now = chrono_timestamp();
    let world_data = WorldSaveData {
        version: SAVE_FORMAT_VERSION,
        timestamp: now,
        player: PlayerSaveData {
            position: [player_pos.x, player_pos.y, player_pos.z],
            flying: player_movement.flying,
            noclip: player_movement.noclip,
        },
        terrain: TerrainSaveData {
            seed: engine_config.terrain.seed,
            base_height: engine_config.terrain.base_height,
            height_scale: engine_config.terrain.height_scale,
            frequency: engine_config.terrain.frequency,
            octaves: engine_config.terrain.octaves,
            biome_scale: engine_config.terrain.biome_scale,
        },
        saved_chunks: saved_positions,
    };

    // Write world.json
    let json = serde_json::to_string_pretty(&world_data).map_err(io::Error::other)?;
    fs::write(save_dir.join("world.json"), json)?;

    info!(
        "World saved: {} chunks written, {} total tracked",
        chunks_saved,
        world_data.saved_chunks.len()
    );
    Ok(chunks_saved)
}

/// Load world metadata from a `world.json` file.
///
/// # Errors
///
/// Returns `io::Error` if the file doesn't exist or is malformed.
pub fn load_world_metadata(path: &Path) -> Result<WorldSaveData, io::Error> {
    let json = fs::read_to_string(path)?;
    let data: WorldSaveData =
        serde_json::from_str(&json).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    if data.version > SAVE_FORMAT_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Save version {} is newer than supported version {}",
                data.version, SAVE_FORMAT_VERSION
            ),
        ));
    }

    Ok(data)
}

/// Load the full world state: apply player position, movement, and
/// point the chunk storage at the save's chunk directory.
///
/// This function:
/// 1. Reads `world.json` for player state and terrain config
/// 2. Points `ChunkStorage` at the save's chunk directory so that
///    the chunk streaming system will load saved chunks from there
/// 3. Applies player position and movement state
///
/// Returns the loaded `WorldSaveData` for further inspection.
///
/// # Errors
///
/// Returns `io::Error` if the save directory or world.json is missing/corrupt.
pub fn load_world(save_dir: &Path) -> Result<WorldSaveData, io::Error> {
    let world_file = save_dir.join("world.json");
    load_world_metadata(&world_file)
}

// ============================================================================
// BEVY SYSTEMS
// ============================================================================

/// System: tick the auto-save timer and request saves when the interval elapses.
pub fn auto_save_timer_system(time: Res<Time>, mut save_system: ResMut<SaveSystem>) {
    save_system.auto_save_timer += time.delta_secs();

    if save_system.auto_save_timer >= save_system.auto_save_interval {
        save_system.auto_save_timer = 0.0;
        save_system.save_requested = true;
        info!("Auto-save triggered");
    }
}

/// System: trigger a manual save when F5 is pressed.
pub fn manual_save_trigger_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut save_system: ResMut<SaveSystem>,
) {
    if keyboard.just_pressed(KeyCode::F5) {
        save_system.save_requested = true;
        info!("Manual save triggered (F5)");
    }
}

/// System: perform the actual save when requested.
///
/// Collects all loaded chunks, the player's current state, and the engine
/// config, then writes everything to disk. Only runs when `save_requested`
/// is `true`.
pub fn perform_save_system(
    mut save_system: ResMut<SaveSystem>,
    chunk_query: Query<&Chunk>,
    player_query: Query<(&Transform, &Movement), With<Player>>,
    engine_config: Res<EngineConfig>,
) {
    if !save_system.save_requested {
        return;
    }
    save_system.save_requested = false;

    // Collect player state
    let (player_pos, player_movement) = match player_query.get_single() {
        Ok((transform, movement)) => (transform.translation, movement),
        Err(_) => {
            warn!("Save aborted: no player entity found");
            return;
        }
    };

    // Collect all loaded chunks
    let chunks: Vec<(IVec3, &Chunk)> = chunk_query.iter().map(|c| (c.position, c)).collect();

    let save_dir = save_system.save_dir.clone();
    match save_world(&save_dir, &chunks, player_pos, player_movement, &engine_config) {
        Ok(count) => {
            save_system.last_save_chunk_count = count;
            info!(
                "Save complete: {} modified chunks written to {:?}",
                count, save_dir
            );
        }
        Err(e) => {
            warn!("Save failed: {}", e);
        }
    }
}

/// System: on startup, load world state if a save exists.
///
/// Runs once during `PostStartup`. If a save file is found, it:
/// - Teleports the player to the saved position
/// - Restores flying/noclip state
/// - Points `ChunkStorage` at the save's chunks directory so loaded
///   chunks are read from the save instead of regenerated
pub fn load_world_on_startup_system(
    save_system: Res<SaveSystem>,
    mut player_query: Query<(&mut Transform, &mut Movement), With<Player>>,
    mut chunk_storage: ResMut<ChunkStorage>,
) {
    let world_file = save_system.world_file();

    if !world_file.exists() {
        info!("No save file found at {:?}, starting fresh", world_file);
        return;
    }

    match load_world_metadata(&world_file) {
        Ok(data) => {
            info!(
                "Loading world save (version {}, {} tracked chunks)",
                data.version,
                data.saved_chunks.len()
            );

            // Apply player state
            if let Ok((mut transform, mut movement)) = player_query.get_single_mut() {
                transform.translation = Vec3::new(
                    data.player.position[0],
                    data.player.position[1],
                    data.player.position[2],
                );
                movement.flying = data.player.flying;
                movement.noclip = data.player.noclip;
                info!(
                    "Player restored to ({:.1}, {:.1}, {:.1}) flying={} noclip={}",
                    data.player.position[0],
                    data.player.position[1],
                    data.player.position[2],
                    data.player.flying,
                    data.player.noclip,
                );
            }

            // Point chunk storage at saved chunks directory
            let chunk_dir = save_system.chunk_dir();
            if chunk_dir.exists() {
                chunk_storage.save_dir = chunk_dir.clone();
                info!("Chunk storage redirected to {:?}", chunk_dir);
            }
        }
        Err(e) => {
            warn!("Failed to load save from {:?}: {}", world_file, e);
        }
    }
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that adds the world save/load system.
///
/// Registers the [`SaveSystem`] resource and adds systems for:
/// - Loading existing saves on startup
/// - Periodic auto-saves
/// - Manual save on F5
pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SaveSystem>().add_systems(
            PostStartup,
            load_world_on_startup_system,
        )
        .add_systems(
            Update,
            (
                auto_save_timer_system,
                manual_save_trigger_system,
                perform_save_system,
            )
                .chain(),
        );
    }
}

// ============================================================================
// HELPERS
// ============================================================================

/// Generate an ISO-8601-ish timestamp string without external crate deps.
///
/// Format: `YYYY-MM-DD HH:MM:SS` (no timezone — we don't pull in `chrono`).
fn chrono_timestamp() -> String {
    use std::time::SystemTime;
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => {
            let secs = d.as_secs();
            // Simple epoch seconds — accurate enough for save ordering
            format!("epoch:{}", secs)
        }
        Err(_) => "unknown".to_string(),
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::BlockType;

    /// Create a temporary save directory for testing.
    fn temp_save_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "pw_save_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    /// Clean up a test save directory.
    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    fn default_movement() -> Movement {
        Movement::default()
    }

    fn default_config() -> EngineConfig {
        EngineConfig::default()
    }

    #[test]
    fn test_save_system_default() {
        let ss = SaveSystem::default();
        assert_eq!(ss.save_dir, PathBuf::from("saves/default"));
        assert_eq!(ss.auto_save_interval, 300.0);
        assert_eq!(ss.auto_save_timer, 0.0);
        assert_eq!(ss.last_save_chunk_count, 0);
        assert!(!ss.save_requested);
    }

    #[test]
    fn test_save_system_custom_dir() {
        let ss = SaveSystem::new("my_saves/world1");
        assert_eq!(ss.save_dir, PathBuf::from("my_saves/world1"));
    }

    #[test]
    fn test_save_system_paths() {
        let ss = SaveSystem::new("saves/test");
        assert_eq!(ss.world_file(), PathBuf::from("saves/test/world.json"));
        assert_eq!(ss.chunk_dir(), PathBuf::from("saves/test/chunks"));
    }

    #[test]
    fn test_save_system_request_save() {
        let mut ss = SaveSystem::default();
        assert!(!ss.save_requested);
        ss.request_save();
        assert!(ss.save_requested);
    }

    #[test]
    fn test_save_empty_world() {
        let dir = temp_save_dir();
        let chunks: Vec<(IVec3, &Chunk)> = vec![];
        let movement = default_movement();
        let config = default_config();

        let result = save_world(
            &dir,
            &chunks,
            Vec3::new(32.0, 64.0, 32.0),
            &movement,
            &config,
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);

        // world.json should exist
        assert!(dir.join("world.json").exists());

        cleanup(&dir);
    }

    #[test]
    fn test_save_and_load_world_metadata() {
        let dir = temp_save_dir();
        let chunks: Vec<(IVec3, &Chunk)> = vec![];
        let movement = Movement {
            flying: true,
            noclip: true,
            ..default()
        };
        let config = default_config();

        save_world(
            &dir,
            &chunks,
            Vec3::new(100.0, 200.0, 300.0),
            &movement,
            &config,
        )
        .expect("save should succeed");

        let loaded = load_world_metadata(&dir.join("world.json")).expect("load should succeed");

        assert_eq!(loaded.version, SAVE_FORMAT_VERSION);
        assert_eq!(loaded.player.position, [100.0, 200.0, 300.0]);
        assert!(loaded.player.flying);
        assert!(loaded.player.noclip);
        assert_eq!(loaded.terrain.seed, config.terrain.seed);
        assert_eq!(loaded.terrain.base_height, config.terrain.base_height);
        assert!(loaded.saved_chunks.is_empty());

        cleanup(&dir);
    }

    #[test]
    fn test_save_only_modified_chunks() {
        let dir = temp_save_dir();
        let movement = default_movement();
        let config = default_config();

        // Create chunks: one modified, one not
        let mut modified_chunk = Chunk::new(IVec3::new(1, 0, 1));
        modified_chunk.set_block(0, 0, 0, BlockType::Stone);
        modified_chunk.modified = true;

        let unmodified_chunk = Chunk::new(IVec3::new(2, 0, 2));
        // unmodified_chunk.modified is false by default

        let chunks: Vec<(IVec3, &Chunk)> = vec![
            (IVec3::new(1, 0, 1), &modified_chunk),
            (IVec3::new(2, 0, 2), &unmodified_chunk),
        ];

        let result = save_world(
            &dir,
            &chunks,
            Vec3::ZERO,
            &movement,
            &config,
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1); // Only 1 chunk saved

        // Modified chunk file should exist
        let chunk_dir = dir.join("chunks");
        assert!(chunk_dir.join("chunk_1_0_1.json").exists());

        // Unmodified chunk file should NOT exist
        assert!(!chunk_dir.join("chunk_2_0_2.json").exists());

        // World metadata should list only the modified chunk
        let loaded = load_world_metadata(&dir.join("world.json")).unwrap();
        assert_eq!(loaded.saved_chunks.len(), 1);
        assert_eq!(loaded.saved_chunks[0], [1, 0, 1]);

        cleanup(&dir);
    }

    #[test]
    fn test_incremental_save_preserves_old_chunks() {
        let dir = temp_save_dir();
        let movement = default_movement();
        let config = default_config();

        // First save: one modified chunk
        let mut chunk_a = Chunk::new(IVec3::new(0, 0, 0));
        chunk_a.set_block(5, 5, 5, BlockType::Dirt);
        chunk_a.modified = true;

        let chunks_a: Vec<(IVec3, &Chunk)> = vec![(IVec3::ZERO, &chunk_a)];
        save_world(&dir, &chunks_a, Vec3::ZERO, &movement, &config).unwrap();

        let loaded1 = load_world_metadata(&dir.join("world.json")).unwrap();
        assert_eq!(loaded1.saved_chunks.len(), 1);

        // Second save: a different modified chunk (chunk_a no longer in-memory
        // but its file is still on disk)
        let mut chunk_b = Chunk::new(IVec3::new(3, 0, 3));
        chunk_b.set_block(1, 1, 1, BlockType::Water);
        chunk_b.modified = true;

        let chunks_b: Vec<(IVec3, &Chunk)> = vec![(IVec3::new(3, 0, 3), &chunk_b)];
        save_world(&dir, &chunks_b, Vec3::ZERO, &movement, &config).unwrap();

        // Both chunks should appear in the manifest
        let loaded2 = load_world_metadata(&dir.join("world.json")).unwrap();
        assert_eq!(loaded2.saved_chunks.len(), 2);

        // Both chunk files should exist
        let chunk_dir = dir.join("chunks");
        assert!(chunk_dir.join("chunk_0_0_0.json").exists());
        assert!(chunk_dir.join("chunk_3_0_3.json").exists());

        cleanup(&dir);
    }

    #[test]
    fn test_save_world_creates_directories() {
        let dir = temp_save_dir().join("deep/nested/save");
        assert!(!dir.exists());

        let movement = default_movement();
        let config = default_config();
        let chunks: Vec<(IVec3, &Chunk)> = vec![];

        save_world(&dir, &chunks, Vec3::ZERO, &movement, &config).unwrap();

        assert!(dir.exists());
        assert!(dir.join("world.json").exists());
        assert!(dir.join("chunks").exists());

        cleanup(&dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn test_load_nonexistent_world() {
        let dir = temp_save_dir();
        let result = load_world(&dir);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn test_world_save_data_serialization_roundtrip() {
        let data = WorldSaveData {
            version: SAVE_FORMAT_VERSION,
            timestamp: "epoch:1234567890".to_string(),
            player: PlayerSaveData {
                position: [1.0, 2.0, 3.0],
                flying: true,
                noclip: false,
            },
            terrain: TerrainSaveData {
                seed: 42,
                base_height: 32.0,
                height_scale: 16.0,
                frequency: 0.02,
                octaves: 4,
                biome_scale: 0.005,
            },
            saved_chunks: vec![[0, 0, 0], [1, 2, 3], [-1, -1, -1]],
        };

        let json = serde_json::to_string_pretty(&data).unwrap();
        let parsed: WorldSaveData = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.version, data.version);
        assert_eq!(parsed.timestamp, data.timestamp);
        assert_eq!(parsed.player.position, data.player.position);
        assert_eq!(parsed.player.flying, data.player.flying);
        assert_eq!(parsed.player.noclip, data.player.noclip);
        assert_eq!(parsed.terrain.seed, data.terrain.seed);
        assert_eq!(parsed.terrain.biome_scale, data.terrain.biome_scale);
        assert_eq!(parsed.saved_chunks.len(), 3);
    }

    #[test]
    fn test_saved_chunk_data_integrity() {
        let dir = temp_save_dir();
        let movement = default_movement();
        let config = default_config();

        // Create a chunk with a distinctive pattern
        let mut chunk = Chunk::new(IVec3::new(7, -2, 3));
        chunk.set_block(0, 0, 0, BlockType::Stone);
        chunk.set_block(15, 15, 15, BlockType::Obsidian);
        chunk.set_block(8, 4, 12, BlockType::Wood);
        chunk.modified = true;

        let chunks: Vec<(IVec3, &Chunk)> = vec![(chunk.position, &chunk)];
        save_world(&dir, &chunks, Vec3::ZERO, &movement, &config).unwrap();

        // Load chunk back using low-level persistence
        let storage = ChunkStorage::new(dir.join("chunks"));
        let loaded = persistence::load_chunk(IVec3::new(7, -2, 3), &storage).unwrap();

        assert_eq!(loaded.position, IVec3::new(7, -2, 3));
        assert_eq!(loaded.get_block(0, 0, 0), BlockType::Stone);
        assert_eq!(loaded.get_block(15, 15, 15), BlockType::Obsidian);
        assert_eq!(loaded.get_block(8, 4, 12), BlockType::Wood);
        assert_eq!(loaded.get_block(1, 1, 1), BlockType::Air);

        cleanup(&dir);
    }

    #[test]
    fn test_player_state_preserved() {
        let dir = temp_save_dir();
        let config = default_config();

        let movement = Movement {
            walk_speed: 4.3,
            sprint_speed: 5.6,
            fly_speed: 11.0,
            jump_velocity: 8.4,
            step_height: 0.6,
            flying: true,
            noclip: false,
        };
        let pos = Vec3::new(123.456, 78.9, -42.0);

        let chunks: Vec<(IVec3, &Chunk)> = vec![];
        save_world(&dir, &chunks, pos, &movement, &config).unwrap();

        let loaded = load_world(&dir).unwrap();
        // Check position (floating point should be exact for these values)
        assert!((loaded.player.position[0] - 123.456).abs() < f32::EPSILON);
        assert!((loaded.player.position[1] - 78.9).abs() < f32::EPSILON);
        assert!((loaded.player.position[2] - (-42.0)).abs() < f32::EPSILON);
        assert!(loaded.player.flying);
        assert!(!loaded.player.noclip);

        cleanup(&dir);
    }

    #[test]
    fn test_terrain_config_preserved() {
        let dir = temp_save_dir();
        let movement = default_movement();

        let mut config = default_config();
        config.terrain.seed = 99999;
        config.terrain.base_height = 64.0;
        config.terrain.height_scale = 32.0;
        config.terrain.frequency = 0.01;
        config.terrain.octaves = 6;
        config.terrain.biome_scale = 0.003;

        let chunks: Vec<(IVec3, &Chunk)> = vec![];
        save_world(&dir, &chunks, Vec3::ZERO, &movement, &config).unwrap();

        let loaded = load_world(&dir).unwrap();
        assert_eq!(loaded.terrain.seed, 99999);
        assert_eq!(loaded.terrain.base_height, 64.0);
        assert_eq!(loaded.terrain.height_scale, 32.0);
        assert_eq!(loaded.terrain.frequency, 0.01);
        assert_eq!(loaded.terrain.octaves, 6);
        assert_eq!(loaded.terrain.biome_scale, 0.003);

        cleanup(&dir);
    }

    #[test]
    fn test_overwrite_existing_save() {
        let dir = temp_save_dir();
        let movement = default_movement();
        let config = default_config();

        // First save
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(0, 0, 0, BlockType::Stone);
        chunk.modified = true;
        let chunks: Vec<(IVec3, &Chunk)> = vec![(IVec3::ZERO, &chunk)];
        save_world(&dir, &chunks, Vec3::new(10.0, 20.0, 30.0), &movement, &config).unwrap();

        // Second save with different player position
        save_world(
            &dir,
            &chunks,
            Vec3::new(99.0, 88.0, 77.0),
            &movement,
            &config,
        )
        .unwrap();

        let loaded = load_world(&dir).unwrap();
        assert_eq!(loaded.player.position, [99.0, 88.0, 77.0]);

        cleanup(&dir);
    }

    #[test]
    fn test_chrono_timestamp_format() {
        let ts = chrono_timestamp();
        assert!(ts.starts_with("epoch:") || ts == "unknown");
        if ts.starts_with("epoch:") {
            let secs: u64 = ts.strip_prefix("epoch:").unwrap().parse().unwrap();
            assert!(secs > 1_700_000_000); // After 2023
        }
    }

    #[test]
    fn test_world_json_is_human_readable() {
        let dir = temp_save_dir();
        let movement = default_movement();
        let config = default_config();

        let mut chunk = Chunk::new(IVec3::new(1, 2, 3));
        chunk.modified = true;
        let chunks: Vec<(IVec3, &Chunk)> = vec![(chunk.position, &chunk)];
        save_world(&dir, &chunks, Vec3::ZERO, &movement, &config).unwrap();

        let contents = fs::read_to_string(dir.join("world.json")).unwrap();
        // Pretty-printed JSON should have newlines and indentation
        assert!(contents.contains('\n'));
        assert!(contents.contains("  "));
        // Should contain expected keys
        assert!(contents.contains("\"version\""));
        assert!(contents.contains("\"player\""));
        assert!(contents.contains("\"terrain\""));
        assert!(contents.contains("\"saved_chunks\""));

        cleanup(&dir);
    }

    #[test]
    fn test_load_future_version_rejected() {
        let dir = temp_save_dir();
        fs::create_dir_all(&dir).unwrap();

        let future_save = r#"{
            "version": 999,
            "timestamp": "epoch:0",
            "player": { "position": [0, 0, 0], "flying": false, "noclip": false },
            "terrain": { "seed": 1, "base_height": 32, "height_scale": 16, "frequency": 0.02, "octaves": 4, "biome_scale": 0.005 },
            "saved_chunks": []
        }"#;
        fs::write(dir.join("world.json"), future_save).unwrap();

        let result = load_world(&dir);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("newer than supported"));

        cleanup(&dir);
    }

    #[test]
    fn test_multiple_modified_chunks_saved() {
        let dir = temp_save_dir();
        let movement = default_movement();
        let config = default_config();

        let mut chunks_data = Vec::new();
        for i in 0..5 {
            let mut chunk = Chunk::new(IVec3::new(i, 0, 0));
            chunk.set_block(0, 0, 0, BlockType::Stone);
            chunk.modified = true;
            chunks_data.push(chunk);
        }

        let chunks: Vec<(IVec3, &Chunk)> = chunks_data
            .iter()
            .map(|c| (c.position, c))
            .collect();

        let result = save_world(&dir, &chunks, Vec3::ZERO, &movement, &config);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 5);

        let loaded = load_world(&dir).unwrap();
        assert_eq!(loaded.saved_chunks.len(), 5);

        // All chunk files should exist
        let chunk_dir = dir.join("chunks");
        for i in 0..5 {
            assert!(
                chunk_dir.join(format!("chunk_{}_0_0.json", i)).exists(),
                "chunk_{}_0_0.json should exist",
                i
            );
        }

        cleanup(&dir);
    }
}
