//! Chunk persistence — save/load chunk block data to/from disk
//!
//! Provides primitives for serializing chunk data to JSON files on disk.
//! Each chunk is saved as a separate file named by its chunk coordinates
//! (e.g., `chunk_0_2_-3.json`).
//!
//! # Usage
//!
//! ```ignore
//! let storage = ChunkStorage::new("world/saves");
//! save_chunk(&chunk, &storage)?;
//! let loaded = load_chunk(IVec3::new(0, 2, -3), &storage)?;
//! ```
//!
//! This module only provides the save/load primitives.
//! Integration with the chunk streaming system will come later.

use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::{BlockType, Chunk, CHUNK_VOLUME};

// ============================================================================
// SAVE FORMAT
// ============================================================================

/// Serialization format for chunk data on disk.
///
/// The `world.json` metadata file is always JSON (human-readable).
/// This enum controls only the chunk data file format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SaveFormat {
    /// JSON — human-readable, larger files (~100KB per chunk).
    Json,
    /// Binary via bincode — compact (~8KB per chunk), faster I/O.
    #[default]
    Binary,
}

impl fmt::Display for SaveFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveFormat::Json => write!(f, "json"),
            SaveFormat::Binary => write!(f, "binary"),
        }
    }
}

impl SaveFormat {
    /// Parse a format from a string (case-insensitive).
    ///
    /// Returns `Binary` for unrecognised values (with a log warning).
    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "json" => SaveFormat::Json,
            "binary" | "bin" | "bincode" => SaveFormat::Binary,
            other => {
                warn!("Unknown chunk_format '{}', defaulting to binary", other);
                SaveFormat::Binary
            }
        }
    }

    /// File extension for this format.
    pub fn extension(&self) -> &'static str {
        match self {
            SaveFormat::Json => "json",
            SaveFormat::Binary => "bin",
        }
    }
}

// ============================================================================
// DATA STRUCTURES
// ============================================================================

/// Serializable representation of a chunk's data
///
/// Stores block data as a `Vec<u16>` for compact JSON serialization
/// rather than serializing each `BlockType` enum variant by name.
#[derive(Serialize, Deserialize, Debug)]
pub struct SavedChunk {
    /// Chunk position in chunk coordinates `[x, y, z]`
    pub position: [i32; 3],
    /// Block data as u16 values (matches `BlockType` repr)
    pub blocks: Vec<u16>,
}

/// Resource for configuring chunk storage location on disk
///
/// Insert this as a Bevy resource to configure where chunks are saved.
/// Defaults to `"world/chunks"` relative to the working directory.
#[derive(Resource, Clone, Debug)]
pub struct ChunkStorage {
    /// Directory path where chunk files are saved
    pub save_dir: PathBuf,
}

impl Default for ChunkStorage {
    fn default() -> Self {
        Self {
            save_dir: PathBuf::from("world/chunks"),
        }
    }
}

impl ChunkStorage {
    /// Create a new `ChunkStorage` with a custom save directory
    pub fn new(save_dir: impl Into<PathBuf>) -> Self {
        Self {
            save_dir: save_dir.into(),
        }
    }
}

// ============================================================================
// FILE PATH HELPERS
// ============================================================================

/// Get the file path for a chunk at the given position (JSON format).
///
/// Returns a path like `<save_dir>/chunk_0_2_-3.json`.
pub fn chunk_file_path(position: IVec3, storage: &ChunkStorage) -> PathBuf {
    chunk_file_path_fmt(position, storage, SaveFormat::Json)
}

/// Get the file path for a chunk at the given position with a specific format.
///
/// Returns a path like `<save_dir>/chunk_0_2_-3.json` or `<save_dir>/chunk_0_2_-3.bin`.
pub fn chunk_file_path_fmt(position: IVec3, storage: &ChunkStorage, format: SaveFormat) -> PathBuf {
    storage.save_dir.join(format!(
        "chunk_{}_{}_{}.{}",
        position.x, position.y, position.z,
        format.extension()
    ))
}

// ============================================================================
// SAVE / LOAD
// ============================================================================

/// Save a chunk's block data to disk as a JSON file.
///
/// Creates the save directory if it doesn't exist. The chunk is serialized
/// to a file named by its position (e.g., `chunk_0_2_-3.json`).
///
/// This is the legacy JSON-only entry point. Prefer [`save_chunk_fmt`] for
/// format-aware saving.
///
/// # Errors
///
/// Returns `io::Error` if the directory can't be created or the file can't be written.
pub fn save_chunk(chunk: &Chunk, storage: &ChunkStorage) -> Result<(), io::Error> {
    save_chunk_fmt(chunk, storage, SaveFormat::Json)
}

/// Save a chunk's block data to disk in the specified format.
///
/// Creates the save directory if it doesn't exist. The chunk is serialized
/// to a file named by its position with the appropriate extension
/// (e.g., `chunk_0_2_-3.json` or `chunk_0_2_-3.bin`).
///
/// # Errors
///
/// Returns `io::Error` if the directory can't be created or the file can't be written.
pub fn save_chunk_fmt(chunk: &Chunk, storage: &ChunkStorage, format: SaveFormat) -> Result<(), io::Error> {
    let saved = SavedChunk {
        position: [chunk.position.x, chunk.position.y, chunk.position.z],
        blocks: chunk.blocks().iter().map(|&b| u16::from(b)).collect(),
    };

    // Ensure the save directory exists
    fs::create_dir_all(&storage.save_dir)?;

    let path = chunk_file_path_fmt(chunk.position, storage, format);

    match format {
        SaveFormat::Json => {
            let json = serde_json::to_string(&saved).map_err(io::Error::other)?;
            fs::write(&path, json)?;
        }
        SaveFormat::Binary => {
            let bytes = bincode::serialize(&saved)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
            fs::write(&path, bytes)?;
        }
    }

    info!("Saved chunk at {:?} to {:?} ({})", chunk.position, path, format);
    Ok(())
}

/// Check if a saved chunk file exists on disk for the given position.
///
/// Checks for both JSON and binary formats. Returns `true` if either exists.
///
/// Useful for debugging and determining whether a chunk needs generation
/// or can be loaded from a previous save.
pub fn chunk_exists(position: IVec3, storage: &ChunkStorage) -> bool {
    chunk_file_path_fmt(position, storage, SaveFormat::Json).exists()
        || chunk_file_path_fmt(position, storage, SaveFormat::Binary).exists()
}

/// Load a chunk's block data from disk (JSON format).
///
/// This is the legacy JSON-only entry point. For format-aware loading that
/// auto-detects binary or JSON files, use [`load_chunk_auto`].
///
/// # Errors
///
/// Returns `io::Error` if the file doesn't exist, is malformed, or has
/// an incorrect block count.
pub fn load_chunk(position: IVec3, storage: &ChunkStorage) -> Result<Chunk, io::Error> {
    // Try auto-detection first: binary, then JSON
    load_chunk_auto(position, storage)
}

/// Load a chunk from disk, auto-detecting the format.
///
/// Tries binary (`.bin`) first (since it's the default), then JSON (`.json`).
/// Returns `NotFound` if neither file exists.
///
/// # Errors
///
/// Returns `io::Error` if:
/// - Neither file exists (`NotFound`)
/// - The file is malformed (`InvalidData`)
/// - The block count doesn't match `CHUNK_VOLUME` (`InvalidData`)
pub fn load_chunk_auto(position: IVec3, storage: &ChunkStorage) -> Result<Chunk, io::Error> {
    // Try binary first (default & more common)
    let bin_path = chunk_file_path_fmt(position, storage, SaveFormat::Binary);
    if bin_path.exists() {
        return load_chunk_fmt(position, storage, SaveFormat::Binary);
    }

    // Fall back to JSON
    let json_path = chunk_file_path_fmt(position, storage, SaveFormat::Json);
    if json_path.exists() {
        return load_chunk_fmt(position, storage, SaveFormat::Json);
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("No saved chunk at {:?} (checked .bin and .json)", position),
    ))
}

/// Load a chunk's block data from disk in a specific format.
///
/// Reads and deserializes a chunk file by position. The returned chunk
/// is marked dirty so its mesh will be rebuilt.
///
/// # Errors
///
/// Returns `io::Error` if:
/// - The file doesn't exist (`NotFound`)
/// - The data is malformed (`InvalidData`)
/// - The block count doesn't match `CHUNK_VOLUME` (`InvalidData`)
pub fn load_chunk_fmt(position: IVec3, storage: &ChunkStorage, format: SaveFormat) -> Result<Chunk, io::Error> {
    let path = chunk_file_path_fmt(position, storage, format);

    let saved: SavedChunk = match format {
        SaveFormat::Json => {
            let json = fs::read_to_string(&path)?;
            serde_json::from_str(&json)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
        }
        SaveFormat::Binary => {
            let bytes = fs::read(&path)?;
            bincode::deserialize(&bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?
        }
    };

    // Validate block data length
    if saved.blocks.len() != CHUNK_VOLUME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Expected {} blocks, got {}",
                CHUNK_VOLUME,
                saved.blocks.len()
            ),
        ));
    }

    // Convert u16 values back to BlockType array
    let mut blocks = [BlockType::Air; CHUNK_VOLUME];
    for (i, &val) in saved.blocks.iter().enumerate() {
        blocks[i] = BlockType::from(val);
    }

    let chunk_pos = IVec3::new(saved.position[0], saved.position[1], saved.position[2]);

    info!("Loaded chunk at {:?} from {:?} ({})", chunk_pos, path, format);
    Ok(Chunk::from_blocks(chunk_pos, blocks))
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Create a temporary storage directory for testing
    fn temp_storage() -> ChunkStorage {
        // Tests run in parallel; timestamps alone can collide on some platforms.
        // Add a monotonic counter to guarantee uniqueness.
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);

        let dir = std::env::temp_dir().join(format!(
            "procedural_worlds_test_{}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            unique,
        ));
        ChunkStorage::new(dir)
    }

    /// Clean up a test storage directory
    fn cleanup(storage: &ChunkStorage) {
        let _ = fs::remove_dir_all(&storage.save_dir);
    }

    #[test]
    fn test_chunk_file_path_positive() {
        let storage = ChunkStorage::new("saves");
        let path = chunk_file_path(IVec3::new(0, 2, 3), &storage);
        assert_eq!(path, Path::new("saves").join("chunk_0_2_3.json"));
    }

    #[test]
    fn test_chunk_file_path_negative() {
        let storage = ChunkStorage::new("saves");
        let path = chunk_file_path(IVec3::new(-1, -5, 3), &storage);
        assert_eq!(path, Path::new("saves").join("chunk_-1_-5_3.json"));
    }

    #[test]
    fn test_chunk_storage_default() {
        let storage = ChunkStorage::default();
        assert_eq!(storage.save_dir, PathBuf::from("world/chunks"));
    }

    #[test]
    fn test_round_trip_empty_chunk() {
        let storage = temp_storage();

        let chunk = Chunk::new(IVec3::new(1, 2, 3));
        save_chunk(&chunk, &storage).expect("save should succeed");

        let loaded = load_chunk(IVec3::new(1, 2, 3), &storage).expect("load should succeed");

        assert_eq!(loaded.position, chunk.position);
        for i in 0..CHUNK_VOLUME {
            assert_eq!(loaded.blocks()[i], chunk.blocks()[i]);
        }

        cleanup(&storage);
    }

    #[test]
    fn test_round_trip_with_blocks() {
        let storage = temp_storage();

        let mut chunk = Chunk::new(IVec3::new(-3, 0, 7));
        chunk.set_block(0, 0, 0, BlockType::Stone);
        chunk.set_block(5, 10, 15, BlockType::Grass);
        chunk.set_block(15, 15, 15, BlockType::Water);
        chunk.set_block(8, 4, 2, BlockType::Wood);
        chunk.set_block(1, 1, 1, BlockType::Leaves);
        chunk.set_block(0, 0, 1, BlockType::Sand);
        chunk.set_block(2, 3, 4, BlockType::Dirt);

        save_chunk(&chunk, &storage).expect("save should succeed");
        let loaded = load_chunk(IVec3::new(-3, 0, 7), &storage).expect("load should succeed");

        assert_eq!(loaded.position, IVec3::new(-3, 0, 7));
        assert_eq!(loaded.get_block(0, 0, 0), BlockType::Stone);
        assert_eq!(loaded.get_block(5, 10, 15), BlockType::Grass);
        assert_eq!(loaded.get_block(15, 15, 15), BlockType::Water);
        assert_eq!(loaded.get_block(8, 4, 2), BlockType::Wood);
        assert_eq!(loaded.get_block(1, 1, 1), BlockType::Leaves);
        assert_eq!(loaded.get_block(0, 0, 1), BlockType::Sand);
        assert_eq!(loaded.get_block(2, 3, 4), BlockType::Dirt);
        // Unset blocks should still be Air
        assert_eq!(loaded.get_block(7, 7, 7), BlockType::Air);

        cleanup(&storage);
    }

    #[test]
    fn test_round_trip_filled_chunk() {
        let storage = temp_storage();

        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.fill(BlockType::Stone);

        save_chunk(&chunk, &storage).expect("save should succeed");
        let loaded = load_chunk(IVec3::ZERO, &storage).expect("load should succeed");

        for i in 0..CHUNK_VOLUME {
            assert_eq!(loaded.blocks()[i], BlockType::Stone, "block {} mismatch", i);
        }

        cleanup(&storage);
    }

    #[test]
    fn test_load_nonexistent_chunk_returns_error() {
        let storage = temp_storage();

        let result = load_chunk(IVec3::new(99, 99, 99), &storage);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);

        cleanup(&storage);
    }

    #[test]
    fn test_block_data_integrity_all_types() {
        let storage = temp_storage();

        let all_types = [
            BlockType::Air,
            BlockType::Stone,
            BlockType::Dirt,
            BlockType::Grass,
            BlockType::Sand,
            BlockType::Water,
            BlockType::Wood,
            BlockType::Leaves,
        ];

        let mut chunk = Chunk::new(IVec3::new(0, 0, 0));
        // Place each block type at a known position
        for (i, &block_type) in all_types.iter().enumerate() {
            chunk.set_block(i, 0, 0, block_type);
        }

        save_chunk(&chunk, &storage).expect("save should succeed");
        let loaded = load_chunk(IVec3::ZERO, &storage).expect("load should succeed");

        for (i, &block_type) in all_types.iter().enumerate() {
            assert_eq!(
                loaded.get_block(i, 0, 0),
                block_type,
                "Block type {:?} at index {} did not survive round-trip",
                block_type,
                i
            );
        }

        cleanup(&storage);
    }

    #[test]
    fn test_overwrite_existing_chunk() {
        let storage = temp_storage();

        // Save a chunk with stone
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.fill(BlockType::Stone);
        save_chunk(&chunk, &storage).expect("first save should succeed");

        // Overwrite with dirt
        let mut chunk2 = Chunk::new(IVec3::ZERO);
        chunk2.fill(BlockType::Dirt);
        save_chunk(&chunk2, &storage).expect("overwrite save should succeed");

        // Load should return the overwritten data
        let loaded = load_chunk(IVec3::ZERO, &storage).expect("load should succeed");
        assert_eq!(loaded.blocks()[0], BlockType::Dirt);

        cleanup(&storage);
    }

    #[test]
    fn test_saved_chunk_serialization_format() {
        // Verify the JSON structure is what we expect
        let saved = SavedChunk {
            position: [1, 2, 3],
            blocks: vec![0, 1, 2, 3],
        };

        let json = serde_json::to_string(&saved).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["position"], serde_json::json!([1, 2, 3]));
        assert_eq!(parsed["blocks"], serde_json::json!([0, 1, 2, 3]));
    }

    #[test]
    fn test_blocktype_u16_conversion_roundtrip() {
        let types = [
            BlockType::Air,
            BlockType::Stone,
            BlockType::Dirt,
            BlockType::Grass,
            BlockType::Sand,
            BlockType::Water,
            BlockType::Wood,
            BlockType::Leaves,
        ];

        for block in types {
            let val: u16 = block.into();
            let back: BlockType = val.into();
            assert_eq!(back, block, "u16 round-trip failed for {:?} (u16={})", block, val);
        }
    }

    #[test]
    fn test_unknown_u16_defaults_to_air() {
        let block: BlockType = 255u16.into();
        assert_eq!(block, BlockType::Air);

        let block: BlockType = 100u16.into();
        assert_eq!(block, BlockType::Air);
    }

    #[test]
    fn test_chunk_exists() {
        let storage = temp_storage();

        assert!(!chunk_exists(IVec3::ZERO, &storage));

        let chunk = Chunk::new(IVec3::ZERO);
        save_chunk(&chunk, &storage).expect("save should succeed");

        assert!(chunk_exists(IVec3::ZERO, &storage));
        assert!(!chunk_exists(IVec3::ONE, &storage));

        cleanup(&storage);
    }

    #[test]
    fn test_chunk_exists_binary() {
        let storage = temp_storage();

        assert!(!chunk_exists(IVec3::ZERO, &storage));

        let chunk = Chunk::new(IVec3::ZERO);
        save_chunk_fmt(&chunk, &storage, SaveFormat::Binary).expect("save should succeed");

        assert!(chunk_exists(IVec3::ZERO, &storage));
        assert!(!chunk_exists(IVec3::ONE, &storage));

        cleanup(&storage);
    }

    #[test]
    fn test_loaded_chunk_is_dirty() {
        let storage = temp_storage();

        let chunk = Chunk::new(IVec3::ZERO);
        save_chunk(&chunk, &storage).expect("save should succeed");

        let loaded = load_chunk(IVec3::ZERO, &storage).expect("load should succeed");
        assert!(loaded.dirty, "Loaded chunks should be marked dirty for mesh rebuild");

        cleanup(&storage);
    }

    // ========================================================================
    // Binary format tests
    // ========================================================================

    #[test]
    fn test_binary_round_trip_empty_chunk() {
        let storage = temp_storage();

        let chunk = Chunk::new(IVec3::new(1, 2, 3));
        save_chunk_fmt(&chunk, &storage, SaveFormat::Binary).expect("save should succeed");

        let loaded = load_chunk_fmt(IVec3::new(1, 2, 3), &storage, SaveFormat::Binary)
            .expect("load should succeed");

        assert_eq!(loaded.position, chunk.position);
        for i in 0..CHUNK_VOLUME {
            assert_eq!(loaded.blocks()[i], chunk.blocks()[i]);
        }

        cleanup(&storage);
    }

    #[test]
    fn test_binary_round_trip_with_blocks() {
        let storage = temp_storage();

        let mut chunk = Chunk::new(IVec3::new(-3, 0, 7));
        chunk.set_block(0, 0, 0, BlockType::Stone);
        chunk.set_block(5, 10, 15, BlockType::Grass);
        chunk.set_block(15, 15, 15, BlockType::Water);
        chunk.set_block(8, 4, 2, BlockType::Wood);

        save_chunk_fmt(&chunk, &storage, SaveFormat::Binary).expect("save should succeed");
        let loaded = load_chunk_fmt(IVec3::new(-3, 0, 7), &storage, SaveFormat::Binary)
            .expect("load should succeed");

        assert_eq!(loaded.position, IVec3::new(-3, 0, 7));
        assert_eq!(loaded.get_block(0, 0, 0), BlockType::Stone);
        assert_eq!(loaded.get_block(5, 10, 15), BlockType::Grass);
        assert_eq!(loaded.get_block(15, 15, 15), BlockType::Water);
        assert_eq!(loaded.get_block(8, 4, 2), BlockType::Wood);
        assert_eq!(loaded.get_block(7, 7, 7), BlockType::Air);

        cleanup(&storage);
    }

    #[test]
    fn test_binary_round_trip_filled_chunk() {
        let storage = temp_storage();

        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.fill(BlockType::Stone);

        save_chunk_fmt(&chunk, &storage, SaveFormat::Binary).expect("save should succeed");
        let loaded = load_chunk_fmt(IVec3::ZERO, &storage, SaveFormat::Binary)
            .expect("load should succeed");

        for i in 0..CHUNK_VOLUME {
            assert_eq!(loaded.blocks()[i], BlockType::Stone, "block {} mismatch", i);
        }

        cleanup(&storage);
    }

    #[test]
    fn test_binary_is_smaller_than_json() {
        let storage = temp_storage();

        let mut chunk = Chunk::new(IVec3::ZERO);
        // Fill with a variety of blocks for a realistic comparison
        for x in 0..16 {
            for z in 0..16 {
                chunk.set_block(x, 0, z, BlockType::Stone);
                chunk.set_block(x, 1, z, BlockType::Dirt);
                chunk.set_block(x, 2, z, BlockType::Grass);
            }
        }

        // Save in both formats
        save_chunk_fmt(&chunk, &storage, SaveFormat::Json).expect("json save");
        save_chunk_fmt(&chunk, &storage, SaveFormat::Binary).expect("binary save");

        let json_path = chunk_file_path_fmt(IVec3::ZERO, &storage, SaveFormat::Json);
        let bin_path = chunk_file_path_fmt(IVec3::ZERO, &storage, SaveFormat::Binary);

        let json_size = fs::metadata(&json_path).unwrap().len();
        let bin_size = fs::metadata(&bin_path).unwrap().len();

        assert!(
            bin_size < json_size,
            "Binary ({} bytes) should be smaller than JSON ({} bytes)",
            bin_size,
            json_size
        );

        cleanup(&storage);
    }

    #[test]
    fn test_auto_detect_loads_binary_first() {
        let storage = temp_storage();

        // Save in binary format
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(0, 0, 0, BlockType::Obsidian);
        save_chunk_fmt(&chunk, &storage, SaveFormat::Binary).expect("binary save");

        // load_chunk_auto should find the .bin file
        let loaded = load_chunk_auto(IVec3::ZERO, &storage).expect("auto load");
        assert_eq!(loaded.get_block(0, 0, 0), BlockType::Obsidian);

        cleanup(&storage);
    }

    #[test]
    fn test_auto_detect_falls_back_to_json() {
        let storage = temp_storage();

        // Save in JSON format only
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(0, 0, 0, BlockType::Wood);
        save_chunk_fmt(&chunk, &storage, SaveFormat::Json).expect("json save");

        // load_chunk_auto should find the .json file
        let loaded = load_chunk_auto(IVec3::ZERO, &storage).expect("auto load");
        assert_eq!(loaded.get_block(0, 0, 0), BlockType::Wood);

        cleanup(&storage);
    }

    #[test]
    fn test_auto_detect_returns_not_found() {
        let storage = temp_storage();

        let result = load_chunk_auto(IVec3::new(99, 99, 99), &storage);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);

        cleanup(&storage);
    }

    #[test]
    fn test_save_format_from_str_lossy() {
        assert_eq!(SaveFormat::from_str_lossy("json"), SaveFormat::Json);
        assert_eq!(SaveFormat::from_str_lossy("JSON"), SaveFormat::Json);
        assert_eq!(SaveFormat::from_str_lossy("binary"), SaveFormat::Binary);
        assert_eq!(SaveFormat::from_str_lossy("bin"), SaveFormat::Binary);
        assert_eq!(SaveFormat::from_str_lossy("bincode"), SaveFormat::Binary);
        assert_eq!(SaveFormat::from_str_lossy("BINARY"), SaveFormat::Binary);
        // Unknown defaults to binary
        assert_eq!(SaveFormat::from_str_lossy("unknown"), SaveFormat::Binary);
    }

    #[test]
    fn test_save_format_extension() {
        assert_eq!(SaveFormat::Json.extension(), "json");
        assert_eq!(SaveFormat::Binary.extension(), "bin");
    }

    #[test]
    fn test_save_format_display() {
        assert_eq!(format!("{}", SaveFormat::Json), "json");
        assert_eq!(format!("{}", SaveFormat::Binary), "binary");
    }

    #[test]
    fn test_binary_all_block_types() {
        let storage = temp_storage();

        let all_types = [
            BlockType::Air,
            BlockType::Stone,
            BlockType::Dirt,
            BlockType::Grass,
            BlockType::Sand,
            BlockType::Water,
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

        let mut chunk = Chunk::new(IVec3::new(0, 0, 0));
        for (i, &block_type) in all_types.iter().enumerate() {
            chunk.set_block(i, 0, 0, block_type);
        }

        save_chunk_fmt(&chunk, &storage, SaveFormat::Binary).expect("save should succeed");
        let loaded = load_chunk_fmt(IVec3::ZERO, &storage, SaveFormat::Binary)
            .expect("load should succeed");

        for (i, &block_type) in all_types.iter().enumerate() {
            assert_eq!(
                loaded.get_block(i, 0, 0),
                block_type,
                "Block type {:?} at index {} did not survive binary round-trip",
                block_type,
                i
            );
        }

        cleanup(&storage);
    }
}
