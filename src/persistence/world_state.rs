//! World state data structures for full-world serialization
//!
//! [`WorldState`] captures the complete snapshot of a world session:
//! chunk references, entity data, time, inventories, and session metadata.

use serde::{Deserialize, Serialize};

// ============================================================================
// WORLD STATE — top-level snapshot
// ============================================================================

/// Complete serializable snapshot of a world session.
///
/// This is the root structure written to `session.bin` (bincode) or
/// `session.json` (human-readable fallback). Chunk block data is stored
/// separately by the existing `world::persistence` system; this struct
/// tracks which chunks are saved and adds entity/time/inventory layers.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct WorldState {
    /// Session and save metadata.
    pub metadata: SaveMetadata,

    /// References to saved chunk positions (block data lives in chunk files).
    pub chunks: Vec<ChunkRef>,

    /// All serialized entities (creatures, dropped items, structures).
    pub entities: Vec<EntityData>,

    /// Current in-game time state.
    pub time: TimeState,

    /// Per-player inventory snapshots (keyed by player ID string).
    pub player_inventories: Vec<PlayerInventoryState>,

    /// Multiplayer session information.
    pub session: SessionInfo,
}

impl WorldState {
    /// Create an empty world state with the given save name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            metadata: SaveMetadata::new(name),
            chunks: Vec::new(),
            entities: Vec::new(),
            time: TimeState::default(),
            player_inventories: Vec::new(),
            session: SessionInfo::default(),
        }
    }

    /// Total entity count across all types.
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// Count entities of a specific kind.
    pub fn count_entities_of_kind(&self, kind: EntityKind) -> usize {
        self.entities.iter().filter(|e| e.kind == kind).count()
    }
}

// ============================================================================
// METADATA
// ============================================================================

/// Save file metadata for identification and versioning.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SaveMetadata {
    /// Human-readable save/session name.
    pub name: String,
    /// Save format version (for migration support).
    pub version: u32,
    /// ISO-8601 or epoch timestamp of creation.
    pub created_at: String,
    /// ISO-8601 or epoch timestamp of last modification.
    pub last_modified: String,
}

impl SaveMetadata {
    /// Current save format version.
    pub const CURRENT_VERSION: u32 = 1;

    pub fn new(name: impl Into<String>) -> Self {
        let now = epoch_timestamp();
        Self {
            name: name.into(),
            version: Self::CURRENT_VERSION,
            created_at: now.clone(),
            last_modified: now,
        }
    }

    /// Update the `last_modified` timestamp to now.
    pub fn touch(&mut self) {
        self.last_modified = epoch_timestamp();
    }
}

// ============================================================================
// CHUNK REFERENCES
// ============================================================================

/// Lightweight reference to a saved chunk (position only).
///
/// Actual block data is managed by `world::persistence::ChunkStorage`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ChunkRef {
    /// Chunk coordinates `[x, y, z]`.
    pub position: [i32; 3],
}

impl ChunkRef {
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self { position: [x, y, z] }
    }
}

// ============================================================================
// ENTITY DATA
// ============================================================================

/// The kind/category of a serialized entity.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityKind {
    Creature,
    DroppedItem,
    Structure,
}

/// Serializable snapshot of a world entity.
///
/// Captures enough state to respawn the entity on load.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct EntityData {
    /// What kind of entity this is.
    pub kind: EntityKind,
    /// Type identifier string (e.g. `"Zombie"`, `"Stone"`, `"WoodHouse"`).
    pub type_id: String,
    /// World position `[x, y, z]`.
    pub position: [f32; 3],
    /// Current health (if applicable, 0.0 for items/structures).
    pub health: f32,
    /// Extra key-value metadata (e.g. item count, structure variant).
    pub metadata: Vec<(String, String)>,
}

impl EntityData {
    /// Create a creature entity snapshot.
    pub fn creature(type_id: impl Into<String>, position: [f32; 3], health: f32) -> Self {
        Self {
            kind: EntityKind::Creature,
            type_id: type_id.into(),
            position,
            health,
            metadata: Vec::new(),
        }
    }

    /// Create a dropped item entity snapshot.
    pub fn dropped_item(type_id: impl Into<String>, position: [f32; 3], count: u32) -> Self {
        Self {
            kind: EntityKind::DroppedItem,
            type_id: type_id.into(),
            position,
            health: 0.0,
            metadata: vec![("count".to_string(), count.to_string())],
        }
    }
}

// ============================================================================
// TIME STATE
// ============================================================================

/// In-game time tracking.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TimeState {
    /// Current in-game time of day in seconds (0.0 .. day_length).
    pub time_of_day: f32,
    /// Length of one in-game day in seconds.
    pub day_length: f32,
    /// Number of in-game days elapsed.
    pub day_count: u32,
}

impl Default for TimeState {
    fn default() -> Self {
        Self {
            time_of_day: 0.0,
            day_length: 600.0, // 10 real minutes = 1 game day
            day_count: 0,
        }
    }
}

// ============================================================================
// PLAYER INVENTORY STATE
// ============================================================================

/// Serialized inventory slot.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct InventorySlotData {
    /// Item type identifier string (e.g. `"Block:Stone"`, `"Tool:IronPickaxe"`).
    pub item_id: String,
    /// Stack count.
    pub count: u32,
}

/// Per-player inventory snapshot.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PlayerInventoryState {
    /// Player identifier (username or UUID string).
    pub player_id: String,
    /// Inventory slots (index-ordered, `None` represented by absence).
    pub slots: Vec<Option<InventorySlotData>>,
    /// Currently selected hotbar slot index.
    pub selected_slot: usize,
}

// ============================================================================
// SESSION INFO (multiplayer)
// ============================================================================

/// Multiplayer session metadata.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SessionInfo {
    /// Unique session identifier.
    pub session_id: String,
    /// Whether this is a multiplayer session.
    pub is_multiplayer: bool,
    /// Maximum number of players allowed.
    pub max_players: u32,
    /// Currently connected player IDs.
    pub connected_players: Vec<String>,
    /// Host player ID (if multiplayer).
    pub host_player_id: Option<String>,
}

impl Default for SessionInfo {
    fn default() -> Self {
        Self {
            session_id: generate_session_id(),
            is_multiplayer: false,
            max_players: 1,
            connected_players: vec!["local".to_string()],
            host_player_id: Some("local".to_string()),
        }
    }
}

// ============================================================================
// HELPERS
// ============================================================================

/// Generate an epoch-based timestamp string.
fn epoch_timestamp() -> String {
    use std::time::SystemTime;
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => format!("epoch:{}", d.as_secs()),
        Err(_) => "unknown".to_string(),
    }
}

/// Generate a simple session ID from epoch nanos.
fn generate_session_id() -> String {
    use std::time::SystemTime;
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => format!("session-{}", d.as_nanos()),
        Err(_) => "session-unknown".to_string(),
    }
}
