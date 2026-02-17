//! Save system — serialize/deserialize [`WorldState`] to/from disk
//!
//! Provides functions to:
//! - Collect world state from Bevy ECS queries into a [`WorldState`]
//! - Serialize to bincode (fast, compact) or JSON (human-readable)
//! - Write/read save files from the `saves/` directory
//! - Manage multiple save slots (one file per session)

use std::fs;
use std::io;
use std::path::PathBuf;

use bevy::prelude::*;

use super::world_state::*;
use crate::creatures::{Creature, CreatureType};
use crate::drops::DroppedItem;
use crate::health::Health;
use crate::inventory::{Hotbar, Inventory, ItemId};

// ============================================================================
// SERIALIZATION FORMAT
// ============================================================================

/// Output format for WorldState serialization.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StateFormat {
    /// Bincode — fast, compact binary. Default choice.
    #[default]
    Bincode,
    /// JSON — human-readable, larger files.
    Json,
}

impl StateFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            StateFormat::Bincode => "bin",
            StateFormat::Json => "json",
        }
    }
}

// ============================================================================
// SAVE DIRECTORY MANAGEMENT
// ============================================================================

/// Root directory for all save files.
const SAVES_ROOT: &str = "saves";

/// Get the save file path for a session.
pub fn save_file_path(session_name: &str, format: StateFormat) -> PathBuf {
    PathBuf::from(SAVES_ROOT)
        .join(session_name)
        .join(format!("session.{}", format.extension()))
}

/// List all available save sessions (subdirectories of `saves/`).
pub fn list_saves() -> io::Result<Vec<String>> {
    let root = PathBuf::from(SAVES_ROOT);
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut saves = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && let Some(name) = entry.file_name().to_str()
        {
            saves.push(name.to_string());
        }
    }
    saves.sort();
    Ok(saves)
}

// ============================================================================
// SAVE MANAGEMENT UTILITIES
// ============================================================================

/// Check if a save session exists (has a save directory).
pub fn save_exists(session_name: &str) -> bool {
    let session_dir = PathBuf::from(SAVES_ROOT).join(session_name);
    session_dir.exists() && session_dir.is_dir()
}

/// Summary information about a save file (without loading full state).
#[derive(Debug, Clone)]
pub struct SaveInfo {
    /// Session/save name.
    pub name: String,
    /// File size in bytes (0 if not found).
    pub size_bytes: u64,
    /// Serialization format detected.
    pub format: StateFormat,
    /// Last modified timestamp (Unix epoch seconds, 0 if unavailable).
    pub modified_timestamp: u64,
    /// Whether the save file exists and is readable.
    pub is_valid: bool,
}

/// Get summary information about a save without loading the full state.
///
/// Useful for save selection UI to display file size and modification time.
pub fn get_save_info(session_name: &str) -> SaveInfo {
    // Try bincode first, then JSON
    for format in [StateFormat::Bincode, StateFormat::Json] {
        let path = save_file_path(session_name, format);
        if let Ok(metadata) = fs::metadata(&path) {
            let modified_timestamp = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);

            return SaveInfo {
                name: session_name.to_string(),
                size_bytes: metadata.len(),
                format,
                modified_timestamp,
                is_valid: true,
            };
        }
    }

    // No save file found
    SaveInfo {
        name: session_name.to_string(),
        size_bytes: 0,
        format: StateFormat::Bincode,
        modified_timestamp: 0,
        is_valid: false,
    }
}

/// List all saves with their summary information.
pub fn list_saves_with_info() -> io::Result<Vec<SaveInfo>> {
    let saves = list_saves()?;
    Ok(saves.iter().map(|name| get_save_info(name)).collect())
}

/// Delete a save session and all its associated files.
///
/// This removes the entire session directory under `saves/`.
/// Returns `Ok(())` if deleted successfully, or if the save doesn't exist.
pub fn delete_save(session_name: &str) -> io::Result<()> {
    let session_dir = PathBuf::from(SAVES_ROOT).join(session_name);
    
    if !session_dir.exists() {
        // Already deleted or never existed — success
        return Ok(());
    }

    if !session_dir.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Save path exists but is not a directory: {:?}", session_dir),
        ));
    }

    // Remove the entire session directory
    fs::remove_dir_all(&session_dir)?;
    info!("Deleted save session: {}", session_name);
    Ok(())
}

/// Rename a save session.
///
/// Moves the session directory from `old_name` to `new_name`.
pub fn rename_save(old_name: &str, new_name: &str) -> io::Result<()> {
    if old_name == new_name {
        return Ok(());
    }

    let old_dir = PathBuf::from(SAVES_ROOT).join(old_name);
    let new_dir = PathBuf::from(SAVES_ROOT).join(new_name);

    if !old_dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Save session '{}' not found", old_name),
        ));
    }

    if new_dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("Save session '{}' already exists", new_name),
        ));
    }

    fs::rename(&old_dir, &new_dir)?;
    info!("Renamed save session: {} -> {}", old_name, new_name);
    Ok(())
}

/// Duplicate a save session.
///
/// Creates a copy of the session directory with a new name.
pub fn duplicate_save(source_name: &str, dest_name: &str) -> io::Result<()> {
    let source_dir = PathBuf::from(SAVES_ROOT).join(source_name);
    let dest_dir = PathBuf::from(SAVES_ROOT).join(dest_name);

    if !source_dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Save session '{}' not found", source_name),
        ));
    }

    if dest_dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("Save session '{}' already exists", dest_name),
        ));
    }

    // Create destination directory
    fs::create_dir_all(&dest_dir)?;

    // Copy all files from source to destination
    for entry in fs::read_dir(&source_dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dest_path = dest_dir.join(entry.file_name());

        if file_type.is_file() {
            fs::copy(&src_path, &dest_path)?;
        }
        // Note: We don't recursively copy subdirectories for now.
        // If chunk data lives in subdirs, this would need to be extended.
    }

    info!("Duplicated save session: {} -> {}", source_name, dest_name);
    Ok(())
}

// ============================================================================
// SERIALIZE / DESERIALIZE
// ============================================================================

/// Serialize a [`WorldState`] to bytes using the specified format.
pub fn serialize_world_state(state: &WorldState, format: StateFormat) -> Result<Vec<u8>, io::Error> {
    match format {
        StateFormat::Bincode => bincode::serialize(state)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
        StateFormat::Json => serde_json::to_vec_pretty(state).map_err(io::Error::other),
    }
}

/// Deserialize a [`WorldState`] from bytes using the specified format.
pub fn deserialize_world_state(bytes: &[u8], format: StateFormat) -> Result<WorldState, io::Error> {
    match format {
        StateFormat::Bincode => bincode::deserialize(bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
        StateFormat::Json => {
            serde_json::from_slice(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        }
    }
}

/// Save a [`WorldState`] to disk.
pub fn save_world_state(
    state: &WorldState,
    session_name: &str,
    format: StateFormat,
) -> Result<PathBuf, io::Error> {
    let path = save_file_path(session_name, format);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serialize_world_state(state, format)?;
    fs::write(&path, bytes)?;
    info!("WorldState saved to {:?} ({} bytes)", path, path.metadata()?.len());
    Ok(path)
}

/// Load a [`WorldState`] from disk.
pub fn load_world_state(session_name: &str, format: StateFormat) -> Result<WorldState, io::Error> {
    let path = save_file_path(session_name, format);
    let bytes = fs::read(&path)?;
    let state = deserialize_world_state(&bytes, format)?;
    info!("WorldState loaded from {:?}", path);
    Ok(state)
}

/// Load a [`WorldState`], auto-detecting format by trying bincode then JSON.
pub fn load_world_state_auto(session_name: &str) -> Result<WorldState, io::Error> {
    // Try bincode first
    let bin_path = save_file_path(session_name, StateFormat::Bincode);
    if bin_path.exists() {
        return load_world_state(session_name, StateFormat::Bincode);
    }
    // Fall back to JSON
    let json_path = save_file_path(session_name, StateFormat::Json);
    if json_path.exists() {
        return load_world_state(session_name, StateFormat::Json);
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("No save found for session '{}'", session_name),
    ))
}

// ============================================================================
// ECS COLLECTION — gather WorldState from live Bevy world
// ============================================================================

/// Map a [`CreatureType`] to its string identifier for serialization.
fn creature_type_to_string(ct: CreatureType) -> &'static str {
    match ct {
        CreatureType::Cow => "Cow",
        CreatureType::Sheep => "Sheep",
        CreatureType::Chicken => "Chicken",
        CreatureType::Zombie => "Zombie",
        CreatureType::Skeleton => "Skeleton",
        CreatureType::Spider => "Spider",
    }
}

/// Map a string identifier back to [`CreatureType`].
pub fn string_to_creature_type(s: &str) -> Option<CreatureType> {
    match s {
        "Cow" => Some(CreatureType::Cow),
        "Sheep" => Some(CreatureType::Sheep),
        "Chicken" => Some(CreatureType::Chicken),
        "Zombie" => Some(CreatureType::Zombie),
        "Skeleton" => Some(CreatureType::Skeleton),
        "Spider" => Some(CreatureType::Spider),
        _ => None,
    }
}

/// Map an [`ItemId`] to a string identifier for serialization.
fn item_id_to_string(item: &ItemId) -> String {
    match item {
        ItemId::Block(b) => format!("Block:{}", b.display_name()),
        ItemId::Tool(t) => format!("Tool:{:?}", t),
        ItemId::Material(m) => format!("Material:{:?}", m),
        ItemId::Food(f) => format!("Food:{:?}", f),
    }
}

/// Collect creature entities into [`EntityData`] snapshots.
pub fn collect_creatures(
    creature_query: &Query<(&Creature, &Transform, &Health)>,
) -> Vec<EntityData> {
    creature_query
        .iter()
        .map(|(creature, transform, health)| {
            let pos = transform.translation;
            EntityData::creature(
                creature_type_to_string(creature.creature_type),
                [pos.x, pos.y, pos.z],
                health.current,
            )
        })
        .collect()
}

/// Collect dropped item entities into [`EntityData`] snapshots.
pub fn collect_dropped_items(
    item_query: &Query<(&DroppedItem, &Transform)>,
) -> Vec<EntityData> {
    item_query
        .iter()
        .map(|(item, transform)| {
            let pos = transform.translation;
            EntityData::dropped_item(
                item_id_to_string(&item.item),
                [pos.x, pos.y, pos.z],
                1, // Each DroppedItem entity represents 1 item
            )
        })
        .collect()
}

/// Collect player inventory into a [`PlayerInventoryState`].
pub fn collect_player_inventory(
    inventory: &Inventory,
    hotbar: &Hotbar,
    player_id: &str,
) -> PlayerInventoryState {
    let slots: Vec<Option<InventorySlotData>> = inventory
        .slots
        .iter()
        .map(|slot| {
            slot.as_ref().map(|stack| InventorySlotData {
                item_id: item_id_to_string(&stack.item),
                count: stack.count,
            })
        })
        .collect();

    PlayerInventoryState {
        player_id: player_id.to_string(),
        slots,
        selected_slot: hotbar.selected_slot,
    }
}

/// Collect saved chunk positions into [`ChunkRef`] entries.
pub fn collect_chunk_refs(chunk_positions: &[[i32; 3]]) -> Vec<ChunkRef> {
    chunk_positions
        .iter()
        .map(|&[x, y, z]| ChunkRef::new(x, y, z))
        .collect()
}

// ============================================================================
// BEVY SYSTEMS — placeholder hooks for UI integration
// ============================================================================

/// Event to trigger a full world state save.
#[derive(Event, Debug)]
pub struct SaveWorldStateEvent {
    /// Session name / save slot.
    pub session_name: String,
    /// Serialization format.
    pub format: StateFormat,
}

/// Event to trigger a world state load.
#[derive(Event, Debug)]
pub struct LoadWorldStateEvent {
    /// Session name / save slot.
    pub session_name: String,
}

/// Bevy system: handle [`SaveWorldStateEvent`] by collecting and writing state.
pub fn handle_save_world_state(
    mut events: EventReader<SaveWorldStateEvent>,
    creature_query: Query<(&Creature, &Transform, &Health)>,
    item_query: Query<(&DroppedItem, &Transform)>,
) {
    for event in events.read() {
        let mut state = WorldState::new(&event.session_name);

        // Collect entities
        let mut entities = collect_creatures(&creature_query);
        entities.extend(collect_dropped_items(&item_query));
        state.entities = entities;

        match save_world_state(&state, &event.session_name, event.format) {
            Ok(path) => info!("World state saved to {:?}", path),
            Err(e) => warn!("Failed to save world state: {}", e),
        }
    }
}

/// Bevy system: handle [`LoadWorldStateEvent`] (placeholder — logs loaded data).
pub fn handle_load_world_state(mut events: EventReader<LoadWorldStateEvent>) {
    for event in events.read() {
        match load_world_state_auto(&event.session_name) {
            Ok(state) => {
                info!(
                    "World state loaded: '{}' — {} entities, {} chunks, day {}",
                    state.metadata.name,
                    state.entity_count(),
                    state.chunks.len(),
                    state.time.day_count,
                );
            }
            Err(e) => warn!("Failed to load world state '{}': {}", event.session_name, e),
        }
    }
}

/// Plugin that registers the world state save/load events and systems.
pub struct WorldStatePersistencePlugin;

impl Plugin for WorldStatePersistencePlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<SaveWorldStateEvent>()
            .add_event::<LoadWorldStateEvent>()
            .add_systems(
                Update,
                (handle_save_world_state, handle_load_world_state),
            );
    }
}
