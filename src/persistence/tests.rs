//! Tests for the persistence module.

use std::fs;
use std::path::PathBuf;

use super::save_system::*;
use super::world_state::*;

// Allow unused — PathBuf used in cleanup only
#[allow(unused_imports)]
use std::path::Path;

// ============================================================================
// HELPERS
// ============================================================================

fn sample_world_state() -> WorldState {
    let mut state = WorldState::new("test_world");

    // Add chunks
    state.chunks = vec![
        ChunkRef::new(0, 0, 0),
        ChunkRef::new(1, 0, -1),
        ChunkRef::new(-3, 2, 5),
    ];

    // Add creatures
    state.entities.push(EntityData::creature("Zombie", [10.0, 64.0, 20.0], 15.0));
    state.entities.push(EntityData::creature("Cow", [30.0, 65.0, 40.0], 10.0));
    state.entities.push(EntityData::creature("Skeleton", [-5.0, 62.0, 8.0], 20.0));

    // Add dropped items
    state.entities.push(EntityData::dropped_item("Block:Stone", [12.0, 64.5, 22.0], 3));
    state.entities.push(EntityData::dropped_item("Tool:IronPickaxe", [15.0, 64.0, 25.0], 1));

    // Time state
    state.time = TimeState {
        time_of_day: 300.0,
        day_length: 600.0,
        day_count: 7,
    };

    // Player inventory
    state.player_inventories.push(PlayerInventoryState {
        player_id: "player1".to_string(),
        slots: vec![
            Some(InventorySlotData { item_id: "Block:Stone".to_string(), count: 32 }),
            Some(InventorySlotData { item_id: "Tool:IronPickaxe".to_string(), count: 1 }),
            None,
            Some(InventorySlotData { item_id: "Food:Apple".to_string(), count: 5 }),
        ],
        selected_slot: 1,
    });

    // Session info
    state.session = SessionInfo {
        session_id: "test-session-123".to_string(),
        is_multiplayer: true,
        max_players: 4,
        connected_players: vec!["player1".to_string(), "player2".to_string()],
        host_player_id: Some("player1".to_string()),
    };

    state
}

// ============================================================================
// WorldState construction tests
// ============================================================================

#[test]
fn test_world_state_new() {
    let state = WorldState::new("my_world");
    assert_eq!(state.metadata.name, "my_world");
    assert_eq!(state.metadata.version, SaveMetadata::CURRENT_VERSION);
    assert!(state.chunks.is_empty());
    assert!(state.entities.is_empty());
    assert_eq!(state.time.day_count, 0);
    assert!(state.player_inventories.is_empty());
    assert!(!state.session.session_id.is_empty());
}

#[test]
fn test_world_state_entity_count() {
    let state = sample_world_state();
    assert_eq!(state.entity_count(), 5); // 3 creatures + 2 items
}

#[test]
fn test_world_state_count_by_kind() {
    let state = sample_world_state();
    assert_eq!(state.count_entities_of_kind(EntityKind::Creature), 3);
    assert_eq!(state.count_entities_of_kind(EntityKind::DroppedItem), 2);
    assert_eq!(state.count_entities_of_kind(EntityKind::Structure), 0);
}

// ============================================================================
// Serialization: WorldState serializes without panic
// ============================================================================

#[test]
fn test_serialize_bincode_no_panic() {
    let state = sample_world_state();
    let bytes = serialize_world_state(&state, StateFormat::Bincode);
    assert!(bytes.is_ok());
    assert!(!bytes.unwrap().is_empty());
}

#[test]
fn test_serialize_json_no_panic() {
    let state = sample_world_state();
    let bytes = serialize_world_state(&state, StateFormat::Json);
    assert!(bytes.is_ok());
    assert!(!bytes.unwrap().is_empty());
}

#[test]
fn test_serialize_empty_world_state() {
    let state = WorldState::new("empty");
    let bytes = serialize_world_state(&state, StateFormat::Bincode).unwrap();
    assert!(!bytes.is_empty());
}

// ============================================================================
// Round-trip: serialize → deserialize → equals original
// ============================================================================

#[test]
fn test_bincode_round_trip() {
    let state = sample_world_state();
    let bytes = serialize_world_state(&state, StateFormat::Bincode).unwrap();
    let loaded = deserialize_world_state(&bytes, StateFormat::Bincode).unwrap();
    assert_eq!(state, loaded);
}

#[test]
fn test_json_round_trip() {
    let state = sample_world_state();
    let bytes = serialize_world_state(&state, StateFormat::Json).unwrap();
    let loaded = deserialize_world_state(&bytes, StateFormat::Json).unwrap();
    assert_eq!(state, loaded);
}

#[test]
fn test_chunk_refs_round_trip() {
    let state = sample_world_state();
    let bytes = serialize_world_state(&state, StateFormat::Bincode).unwrap();
    let loaded = deserialize_world_state(&bytes, StateFormat::Bincode).unwrap();

    assert_eq!(loaded.chunks.len(), 3);
    assert_eq!(loaded.chunks[0].position, [0, 0, 0]);
    assert_eq!(loaded.chunks[1].position, [1, 0, -1]);
    assert_eq!(loaded.chunks[2].position, [-3, 2, 5]);
}

// ============================================================================
// Entity counts match after deserialize
// ============================================================================

#[test]
fn test_entity_counts_match_after_deserialize() {
    let state = sample_world_state();
    let bytes = serialize_world_state(&state, StateFormat::Bincode).unwrap();
    let loaded = deserialize_world_state(&bytes, StateFormat::Bincode).unwrap();

    assert_eq!(loaded.entity_count(), state.entity_count());
    assert_eq!(
        loaded.count_entities_of_kind(EntityKind::Creature),
        state.count_entities_of_kind(EntityKind::Creature)
    );
    assert_eq!(
        loaded.count_entities_of_kind(EntityKind::DroppedItem),
        state.count_entities_of_kind(EntityKind::DroppedItem)
    );
}

#[test]
fn test_entity_data_preserved() {
    let state = sample_world_state();
    let bytes = serialize_world_state(&state, StateFormat::Bincode).unwrap();
    let loaded = deserialize_world_state(&bytes, StateFormat::Bincode).unwrap();

    let zombie = &loaded.entities[0];
    assert_eq!(zombie.kind, EntityKind::Creature);
    assert_eq!(zombie.type_id, "Zombie");
    assert_eq!(zombie.position, [10.0, 64.0, 20.0]);
    assert_eq!(zombie.health, 15.0);

    let stone_drop = &loaded.entities[3];
    assert_eq!(stone_drop.kind, EntityKind::DroppedItem);
    assert_eq!(stone_drop.type_id, "Block:Stone");
    assert_eq!(stone_drop.metadata[0], ("count".to_string(), "3".to_string()));
}

// ============================================================================
// Time state
// ============================================================================

#[test]
fn test_time_state_round_trip() {
    let state = sample_world_state();
    let bytes = serialize_world_state(&state, StateFormat::Bincode).unwrap();
    let loaded = deserialize_world_state(&bytes, StateFormat::Bincode).unwrap();

    assert_eq!(loaded.time.time_of_day, 300.0);
    assert_eq!(loaded.time.day_length, 600.0);
    assert_eq!(loaded.time.day_count, 7);
}

// ============================================================================
// Inventory state
// ============================================================================

#[test]
fn test_inventory_state_round_trip() {
    let state = sample_world_state();
    let bytes = serialize_world_state(&state, StateFormat::Bincode).unwrap();
    let loaded = deserialize_world_state(&bytes, StateFormat::Bincode).unwrap();

    assert_eq!(loaded.player_inventories.len(), 1);
    let inv = &loaded.player_inventories[0];
    assert_eq!(inv.player_id, "player1");
    assert_eq!(inv.selected_slot, 1);
    assert_eq!(inv.slots.len(), 4);
    assert_eq!(inv.slots[0].as_ref().unwrap().item_id, "Block:Stone");
    assert_eq!(inv.slots[0].as_ref().unwrap().count, 32);
    assert!(inv.slots[2].is_none());
}

// ============================================================================
// Session info
// ============================================================================

#[test]
fn test_session_info_round_trip() {
    let state = sample_world_state();
    let bytes = serialize_world_state(&state, StateFormat::Bincode).unwrap();
    let loaded = deserialize_world_state(&bytes, StateFormat::Bincode).unwrap();

    assert_eq!(loaded.session.session_id, "test-session-123");
    assert!(loaded.session.is_multiplayer);
    assert_eq!(loaded.session.max_players, 4);
    assert_eq!(loaded.session.connected_players.len(), 2);
    assert_eq!(loaded.session.host_player_id, Some("player1".to_string()));
}

// ============================================================================
// File I/O
// ============================================================================

#[test]
fn test_save_and_load_bincode() {
    // Override SAVES_ROOT by using save_world_state which writes to saves/<name>/
    let state = sample_world_state();
    let session = format!("test_bincode_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());

    let result = save_world_state(&state, &session, StateFormat::Bincode);
    assert!(result.is_ok());

    let loaded = load_world_state(&session, StateFormat::Bincode);
    assert!(loaded.is_ok());
    assert_eq!(loaded.unwrap(), state);

    // Cleanup
    let _ = fs::remove_dir_all(PathBuf::from("saves").join(&session));
}

#[test]
fn test_save_and_load_json() {
    let state = sample_world_state();
    let session = format!("test_json_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());

    save_world_state(&state, &session, StateFormat::Json).unwrap();
    let loaded = load_world_state(&session, StateFormat::Json).unwrap();
    assert_eq!(loaded, state);

    let _ = fs::remove_dir_all(PathBuf::from("saves").join(&session));
}

#[test]
fn test_load_auto_detect() {
    let state = sample_world_state();
    let session = format!("test_auto_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());

    save_world_state(&state, &session, StateFormat::Bincode).unwrap();
    let loaded = load_world_state_auto(&session).unwrap();
    assert_eq!(loaded, state);

    let _ = fs::remove_dir_all(PathBuf::from("saves").join(&session));
}

#[test]
fn test_load_nonexistent_session() {
    let result = load_world_state_auto("nonexistent_session_xyz");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::NotFound);
}

// ============================================================================
// Bincode is more compact than JSON
// ============================================================================

#[test]
fn test_bincode_smaller_than_json() {
    let state = sample_world_state();
    let bin_bytes = serialize_world_state(&state, StateFormat::Bincode).unwrap();
    let json_bytes = serialize_world_state(&state, StateFormat::Json).unwrap();

    assert!(
        bin_bytes.len() < json_bytes.len(),
        "Bincode ({} bytes) should be smaller than JSON ({} bytes)",
        bin_bytes.len(),
        json_bytes.len(),
    );
}

// ============================================================================
// Metadata
// ============================================================================

#[test]
fn test_metadata_touch() {
    let mut meta = SaveMetadata::new("test");
    let _original = meta.last_modified.clone();
    // Sleep briefly to ensure timestamp changes
    std::thread::sleep(std::time::Duration::from_millis(10));
    meta.touch();
    // Timestamps may or may not differ at second granularity, but touch shouldn't panic
    assert!(!meta.last_modified.is_empty());
}

#[test]
fn test_metadata_version() {
    let meta = SaveMetadata::new("test");
    assert_eq!(meta.version, SaveMetadata::CURRENT_VERSION);
    assert_eq!(meta.version, 1);
}

// ============================================================================
// Collection helpers
// ============================================================================

#[test]
fn test_collect_chunk_refs() {
    let positions = vec![[0, 0, 0], [1, 2, 3], [-1, -1, -1]];
    let refs = collect_chunk_refs(&positions);
    assert_eq!(refs.len(), 3);
    assert_eq!(refs[0].position, [0, 0, 0]);
    assert_eq!(refs[2].position, [-1, -1, -1]);
}

#[test]
fn test_creature_type_string_roundtrip() {
    use crate::creatures::CreatureType;
    let types = [
        CreatureType::Cow, CreatureType::Sheep, CreatureType::Chicken,
        CreatureType::Zombie, CreatureType::Skeleton, CreatureType::Spider,
    ];
    for ct in types {
        let s = match ct {
            CreatureType::Cow => "Cow",
            CreatureType::Sheep => "Sheep",
            CreatureType::Chicken => "Chicken",
            CreatureType::Zombie => "Zombie",
            CreatureType::Skeleton => "Skeleton",
            CreatureType::Spider => "Spider",
        };
        let back = string_to_creature_type(s);
        assert_eq!(back, Some(ct));
    }
}

#[test]
fn test_string_to_creature_type_unknown() {
    assert_eq!(string_to_creature_type("Dragon"), None);
}

// ============================================================================
// StateFormat
// ============================================================================

#[test]
fn test_state_format_extension() {
    assert_eq!(StateFormat::Bincode.extension(), "bin");
    assert_eq!(StateFormat::Json.extension(), "json");
}

#[test]
fn test_state_format_default_is_bincode() {
    assert_eq!(StateFormat::default(), StateFormat::Bincode);
}

// ============================================================================
// Save Management Utilities
// ============================================================================

#[test]
fn test_save_exists_false_for_nonexistent() {
    assert!(!save_exists("definitely_not_a_real_save_xyz123"));
}

#[test]
fn test_save_exists_true_after_save() {
    let session = format!("test_exists_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    
    assert!(!save_exists(&session));
    
    let state = WorldState::new("test");
    save_world_state(&state, &session, StateFormat::Bincode).unwrap();
    
    assert!(save_exists(&session));
    
    // Cleanup
    let _ = fs::remove_dir_all(PathBuf::from("saves").join(&session));
}

#[test]
fn test_get_save_info_nonexistent() {
    let info = get_save_info("nonexistent_save_xyz456");
    assert!(!info.is_valid);
    assert_eq!(info.size_bytes, 0);
}

#[test]
fn test_get_save_info_valid() {
    let session = format!("test_info_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    
    let state = sample_world_state();
    save_world_state(&state, &session, StateFormat::Bincode).unwrap();
    
    let info = get_save_info(&session);
    assert!(info.is_valid);
    assert!(info.size_bytes > 0);
    assert_eq!(info.format, StateFormat::Bincode);
    assert_eq!(info.name, session);
    assert!(info.modified_timestamp > 0);
    
    // Cleanup
    let _ = fs::remove_dir_all(PathBuf::from("saves").join(&session));
}

#[test]
fn test_delete_save_nonexistent_ok() {
    // Deleting a nonexistent save should succeed (idempotent)
    let result = delete_save("nonexistent_session_to_delete");
    assert!(result.is_ok());
}

#[test]
fn test_delete_save_removes_session() {
    let session = format!("test_delete_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    
    let state = WorldState::new("test");
    save_world_state(&state, &session, StateFormat::Bincode).unwrap();
    
    assert!(save_exists(&session));
    
    let result = delete_save(&session);
    assert!(result.is_ok());
    assert!(!save_exists(&session));
}

#[test]
fn test_rename_save() {
    let old_name = format!("test_rename_old_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    let new_name = format!("test_rename_new_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    
    let state = sample_world_state();
    save_world_state(&state, &old_name, StateFormat::Bincode).unwrap();
    
    assert!(save_exists(&old_name));
    assert!(!save_exists(&new_name));
    
    let result = rename_save(&old_name, &new_name);
    assert!(result.is_ok());
    
    assert!(!save_exists(&old_name));
    assert!(save_exists(&new_name));
    
    // Verify data is preserved
    let loaded = load_world_state(&new_name, StateFormat::Bincode).unwrap();
    assert_eq!(loaded.entity_count(), state.entity_count());
    
    // Cleanup
    let _ = fs::remove_dir_all(PathBuf::from("saves").join(&new_name));
}

#[test]
fn test_rename_save_nonexistent_fails() {
    let result = rename_save("nonexistent_source_xyz", "some_dest");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn test_rename_save_to_existing_fails() {
    let name_a = format!("test_rename_a_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    let name_b = format!("test_rename_b_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    
    let state = WorldState::new("test");
    save_world_state(&state, &name_a, StateFormat::Bincode).unwrap();
    save_world_state(&state, &name_b, StateFormat::Bincode).unwrap();
    
    let result = rename_save(&name_a, &name_b);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::AlreadyExists);
    
    // Cleanup
    let _ = fs::remove_dir_all(PathBuf::from("saves").join(&name_a));
    let _ = fs::remove_dir_all(PathBuf::from("saves").join(&name_b));
}

#[test]
fn test_duplicate_save() {
    let source = format!("test_dup_src_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    let dest = format!("test_dup_dst_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    
    let state = sample_world_state();
    save_world_state(&state, &source, StateFormat::Bincode).unwrap();
    
    let result = duplicate_save(&source, &dest);
    assert!(result.is_ok());
    
    // Both should exist
    assert!(save_exists(&source));
    assert!(save_exists(&dest));
    
    // Both should have same data
    let loaded_src = load_world_state(&source, StateFormat::Bincode).unwrap();
    let loaded_dst = load_world_state(&dest, StateFormat::Bincode).unwrap();
    assert_eq!(loaded_src, loaded_dst);
    
    // Cleanup
    let _ = fs::remove_dir_all(PathBuf::from("saves").join(&source));
    let _ = fs::remove_dir_all(PathBuf::from("saves").join(&dest));
}

#[test]
fn test_duplicate_save_nonexistent_fails() {
    let result = duplicate_save("nonexistent_source_dup", "some_dest_dup");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn test_list_saves_with_info() {
    let session = format!("test_list_info_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    
    let state = WorldState::new("test");
    save_world_state(&state, &session, StateFormat::Bincode).unwrap();
    
    let saves = list_saves_with_info().unwrap();
    let our_save = saves.iter().find(|s| s.name == session);
    assert!(our_save.is_some());
    assert!(our_save.unwrap().is_valid);
    
    // Cleanup
    let _ = fs::remove_dir_all(PathBuf::from("saves").join(&session));
}
