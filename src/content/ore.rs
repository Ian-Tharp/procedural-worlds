//! Ore Content Definitions
//!
//! Defines ore types that can spawn underground. Ores are loaded from
//! RON files in `assets/content/ores/` and can be created/edited via
//! the in-game content editor.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// ORE DEFINITION
// ============================================================================

/// Which biomes an ore can spawn in
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum BiomeFilter {
    /// Spawns in all biomes
    #[default]
    All,
    /// Only spawns in specific biomes (by name)
    Only(Vec<String>),
    /// Spawns everywhere except these biomes
    Except(Vec<String>),
}

/// How an ore generates in the world
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OreGeneration {
    /// Minimum Y level for spawning
    pub min_y: i32,
    /// Maximum Y level for spawning
    pub max_y: i32,
    /// Average number of blocks per vein
    pub vein_size: u32,
    /// Spawn frequency (0.0 = never, 1.0 = very common)
    /// Typical values: 0.001 (rare) to 0.05 (common)
    pub frequency: f64,
    /// Which biomes this ore spawns in
    #[serde(default)]
    pub biomes: BiomeFilter,
}

impl Default for OreGeneration {
    fn default() -> Self {
        Self {
            min_y: 0,
            max_y: 64,
            vein_size: 8,
            frequency: 0.01,
            biomes: BiomeFilter::All,
        }
    }
}

/// What an ore drops when mined
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OreDrop {
    /// Item ID to drop
    pub item: String,
    /// Minimum drop count
    pub min_count: u32,
    /// Maximum drop count
    pub max_count: u32,
}

impl Default for OreDrop {
    fn default() -> Self {
        Self {
            item: "raw_ore".to_string(),
            min_count: 1,
            max_count: 1,
        }
    }
}

/// Complete definition of an ore type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OreDefinition {
    /// Unique identifier (e.g., "iron_ore")
    pub id: String,
    /// Display name for UI (e.g., "Iron Ore")
    pub display_name: String,
    /// Index in the texture atlas (temporary until we have proper textures)
    #[serde(default)]
    pub texture_index: u32,
    /// How hard the ore is to mine (1.0 = dirt, 5.0 = obsidian)
    #[serde(default = "default_hardness")]
    pub hardness: f32,
    /// Tool type required to mine ("pickaxe", "any", etc.)
    #[serde(default = "default_tool")]
    pub tool_required: String,
    /// Generation parameters
    #[serde(default)]
    pub generation: OreGeneration,
    /// What the ore drops when mined
    #[serde(default)]
    pub drop: OreDrop,
    /// Whether this is user-created content (vs built-in)
    #[serde(default)]
    pub user_content: bool,
    /// Optional description for the editor
    #[serde(default)]
    pub description: String,
}

fn default_hardness() -> f32 {
    3.0
}

fn default_tool() -> String {
    "pickaxe".to_string()
}

impl Default for OreDefinition {
    fn default() -> Self {
        Self {
            id: "new_ore".to_string(),
            display_name: "New Ore".to_string(),
            texture_index: 0,
            hardness: 3.0,
            tool_required: "pickaxe".to_string(),
            generation: OreGeneration::default(),
            drop: OreDrop::default(),
            user_content: true,
            description: String::new(),
        }
    }
}

impl OreDefinition {
    /// Create a new ore definition with the given ID
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        let display_name = id
            .replace('_', " ")
            .split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        Self {
            id: id.clone(),
            display_name,
            drop: OreDrop {
                item: format!("raw_{}", id.replace("_ore", "")),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

// ============================================================================
// ORE REGISTRY
// ============================================================================

/// Registry holding all loaded ore definitions
#[derive(Resource, Default)]
pub struct OreRegistry {
    /// All ore definitions, keyed by ID
    ores: HashMap<String, OreDefinition>,
    /// Path to the content directory
    content_path: PathBuf,
    /// Path to user content directory
    user_content_path: PathBuf,
}

impl OreRegistry {
    /// Create a new registry with the given base path
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        let base = base_path.into();
        Self {
            ores: HashMap::new(),
            content_path: base.join("content/ores"),
            user_content_path: base.join("user_content/ores"),
        }
    }

    /// Load all ore definitions from disk
    pub fn load_all(&mut self) -> Result<usize, String> {
        self.ores.clear();
        let mut loaded = 0;

        // Load built-in ores
        if self.content_path.exists() {
            loaded += self.load_from_directory(&self.content_path.clone(), false)?;
        }

        // Load user ores (these override built-in if same ID)
        if self.user_content_path.exists() {
            loaded += self.load_from_directory(&self.user_content_path.clone(), true)?;
        }

        Ok(loaded)
    }

    /// Load ores from a specific directory
    fn load_from_directory(&mut self, dir: &Path, is_user_content: bool) -> Result<usize, String> {
        let mut loaded = 0;

        let entries = fs::read_dir(dir)
            .map_err(|e| format!("Failed to read directory {:?}: {}", dir, e))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "ron") {
                match self.load_file(&path, is_user_content) {
                    Ok(ore) => {
                        info!("Loaded ore: {} from {:?}", ore.id, path);
                        self.ores.insert(ore.id.clone(), ore);
                        loaded += 1;
                    }
                    Err(e) => {
                        warn!("Failed to load ore from {:?}: {}", path, e);
                    }
                }
            }
        }

        Ok(loaded)
    }

    /// Load a single ore definition from a file
    fn load_file(&self, path: &Path, is_user_content: bool) -> Result<OreDefinition, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read file: {}", e))?;

        let mut ore: OreDefinition = ron::from_str(&content)
            .map_err(|e| format!("Failed to parse RON: {}", e))?;

        ore.user_content = is_user_content;
        Ok(ore)
    }

    /// Get an ore by ID
    pub fn get(&self, id: &str) -> Option<&OreDefinition> {
        self.ores.get(id)
    }

    /// Get a mutable reference to an ore by ID
    pub fn get_mut(&mut self, id: &str) -> Option<&mut OreDefinition> {
        self.ores.get_mut(id)
    }

    /// Iterate over all ores
    pub fn iter(&self) -> impl Iterator<Item = &OreDefinition> {
        self.ores.values()
    }

    /// Get the number of registered ores
    pub fn len(&self) -> usize {
        self.ores.len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.ores.is_empty()
    }

    /// Save an ore definition to disk
    pub fn save(&mut self, ore: OreDefinition) -> Result<(), String> {
        let dir = if ore.user_content {
            &self.user_content_path
        } else {
            &self.content_path
        };

        // Ensure directory exists
        fs::create_dir_all(dir)
            .map_err(|e| format!("Failed to create directory: {}", e))?;

        let path = dir.join(format!("{}.ron", ore.id));
        let content = ron::ser::to_string_pretty(&ore, ron::ser::PrettyConfig::default())
            .map_err(|e| format!("Failed to serialize: {}", e))?;

        fs::write(&path, content)
            .map_err(|e| format!("Failed to write file: {}", e))?;

        info!("Saved ore {} to {:?}", ore.id, path);
        self.ores.insert(ore.id.clone(), ore);
        Ok(())
    }

    /// Delete an ore definition
    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        let ore = self.ores.get(id)
            .ok_or_else(|| format!("Ore '{}' not found", id))?;

        let dir = if ore.user_content {
            &self.user_content_path
        } else {
            &self.content_path
        };

        let path = dir.join(format!("{}.ron", id));
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|e| format!("Failed to delete file: {}", e))?;
        }

        self.ores.remove(id);
        info!("Deleted ore: {}", id);
        Ok(())
    }

    /// Create a new ore with a unique ID
    pub fn create_new(&mut self) -> OreDefinition {
        let mut counter = 1;
        let mut id = "custom_ore".to_string();
        
        while self.ores.contains_key(&id) {
            counter += 1;
            id = format!("custom_ore_{}", counter);
        }

        let mut ore = OreDefinition::new(&id);
        ore.user_content = true;
        ore
    }

    /// Get all ore IDs sorted alphabetically
    pub fn ids_sorted(&self) -> Vec<String> {
        let mut ids: Vec<_> = self.ores.keys().cloned().collect();
        ids.sort();
        ids
    }
}

// ============================================================================
// DEFAULT ORES
// ============================================================================

/// Generate default ore definitions if none exist
pub fn create_default_ores(registry: &mut OreRegistry) {
    let defaults = vec![
        // Tier 1: Common metals (surface to mid-depth)
        OreDefinition {
            id: "copper_ore".to_string(),
            display_name: "Copper Ore".to_string(),
            texture_index: 1,
            hardness: 2.0,
            description: "Soft, abundant metal. The foundation of early crafting.".to_string(),
            generation: OreGeneration {
                min_y: 16,
                max_y: 96,
                vein_size: 12,
                frequency: 0.035,
                biomes: BiomeFilter::All,
            },
            drop: OreDrop {
                item: "raw_copper".to_string(),
                min_count: 2,
                max_count: 4,
            },
            ..Default::default()
        },
        // Tier 2: Standard metals (mid-depth)
        OreDefinition {
            id: "iron_ore".to_string(),
            display_name: "Iron Ore".to_string(),
            texture_index: 2,
            hardness: 3.0,
            description: "Strong and reliable. The backbone of industry.".to_string(),
            generation: OreGeneration {
                min_y: 0,
                max_y: 64,
                vein_size: 8,
                frequency: 0.025,
                biomes: BiomeFilter::All,
            },
            drop: OreDrop {
                item: "raw_iron".to_string(),
                min_count: 1,
                max_count: 2,
            },
            ..Default::default()
        },
        // Tier 3: Precious metals (deeper)
        OreDefinition {
            id: "silver_ore".to_string(),
            display_name: "Silver Ore".to_string(),
            texture_index: 3,
            hardness: 2.5,
            description: "Lustrous and magical. Said to ward off dark creatures.".to_string(),
            generation: OreGeneration {
                min_y: 0,
                max_y: 40,
                vein_size: 5,
                frequency: 0.012,
                biomes: BiomeFilter::All,
            },
            drop: OreDrop {
                item: "raw_silver".to_string(),
                min_count: 1,
                max_count: 2,
            },
            ..Default::default()
        },
        // Tier 4: Rare metals (deep)
        OreDefinition {
            id: "gold_ore".to_string(),
            display_name: "Gold Ore".to_string(),
            texture_index: 4,
            hardness: 2.5,
            description: "Precious and coveted. Currency of kingdoms.".to_string(),
            generation: OreGeneration {
                min_y: 0,
                max_y: 32,
                vein_size: 4,
                frequency: 0.006,
                biomes: BiomeFilter::All,
            },
            drop: OreDrop {
                item: "raw_gold".to_string(),
                min_count: 1,
                max_count: 1,
            },
            ..Default::default()
        },
    ];

    for ore in defaults {
        if let Err(e) = registry.save(ore) {
            warn!("Failed to save default ore: {}", e);
        }
    }
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin for the ore content system
pub struct OrePlugin;

impl Plugin for OrePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_ore_registry);
    }
}

/// Initialize the ore registry on startup
fn setup_ore_registry(mut commands: Commands) {
    let assets_path = std::env::current_dir()
        .unwrap_or_default()
        .join("assets");

    let mut registry = OreRegistry::new(assets_path);

    // Load existing ores
    match registry.load_all() {
        Ok(count) => {
            info!("Loaded {} ore definitions", count);
        }
        Err(e) => {
            warn!("Error loading ores: {}", e);
        }
    }

    // Create defaults if empty
    if registry.is_empty() {
        info!("No ores found, creating defaults...");
        create_default_ores(&mut registry);
        
        // Reload after creating defaults
        if let Err(e) = registry.load_all() {
            warn!("Error reloading ores: {}", e);
        }
    }

    info!("Ore registry initialized with {} ores", registry.len());
    commands.insert_resource(registry);
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ore_definition_new() {
        let ore = OreDefinition::new("test_ore");
        assert_eq!(ore.id, "test_ore");
        assert_eq!(ore.display_name, "Test Ore");
        assert_eq!(ore.drop.item, "raw_test");
    }

    #[test]
    fn test_ore_serialization() {
        let ore = OreDefinition::new("iron_ore");
        let serialized = ron::to_string(&ore).unwrap();
        let deserialized: OreDefinition = ron::from_str(&serialized).unwrap();
        assert_eq!(ore.id, deserialized.id);
    }

    #[test]
    fn test_biome_filter_default() {
        let filter = BiomeFilter::default();
        assert!(matches!(filter, BiomeFilter::All));
    }

    #[test]
    fn test_ore_generation_default() {
        let ore_gen = OreGeneration::default();
        assert_eq!(ore_gen.min_y, 0);
        assert_eq!(ore_gen.max_y, 64);
        assert_eq!(ore_gen.vein_size, 8);
    }
}
