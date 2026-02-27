//! Block Content Definitions
//!
//! Defines block types for the world. Currently wraps the existing BlockType enum
//! but provides data-driven properties and prepares for full dynamic blocks.
//!
//! Phase 1: BlockDefinition + BlockRegistry wrapping BlockType enum
//! Phase 2: Replace BlockType enum with BlockId(u16) for unlimited types

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::world::BlockType;

// ============================================================================
// BLOCK DEFINITION
// ============================================================================

/// Physical properties of a block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockPhysics {
    /// Is this block solid (collision)
    pub solid: bool,
    /// Is this block transparent (for face culling)
    pub transparent: bool,
    /// Can this block be walked through
    pub passable: bool,
}

impl Default for BlockPhysics {
    fn default() -> Self {
        Self {
            solid: true,
            transparent: false,
            passable: false,
        }
    }
}

/// Visual properties of a block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockVisuals {
    /// Base color for vertex coloring [R, G, B, A]
    pub color: [f32; 4],
    /// Texture tile indices [top, bottom, side]
    pub texture_tiles: [u32; 3],
    /// Does this block emit light
    pub light_level: u8,
}

impl Default for BlockVisuals {
    fn default() -> Self {
        Self {
            color: [0.5, 0.5, 0.5, 1.0],
            texture_tiles: [0, 0, 0],
            light_level: 0,
        }
    }
}

/// Complete definition of a block type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockDefinition {
    /// Unique identifier (e.g., "stone", "iron_ore")
    pub id: String,
    /// Display name for UI
    pub display_name: String,
    /// The underlying BlockType enum value (Phase 1 compatibility)
    /// This will be removed in Phase 2 when we switch to BlockId
    #[serde(skip)]
    pub block_type: Option<BlockType>,
    /// Numeric ID for storage (matches BlockType enum value for now)
    pub numeric_id: u16,
    /// Physical properties
    #[serde(default)]
    pub physics: BlockPhysics,
    /// Visual properties
    #[serde(default)]
    pub visuals: BlockVisuals,
    /// Light emission level (0–15, 0 = no light).
    /// Used by the light propagation system to seed BFS.
    #[serde(default)]
    pub light_emission: u8,
    /// Mining hardness (1.0 = dirt, 5.0 = obsidian)
    #[serde(default = "default_hardness")]
    pub hardness: f32,
    /// Tool required to break ("any", "pickaxe", "shovel", "axe")
    #[serde(default = "default_tool")]
    pub tool_required: String,
    /// Category for organization
    #[serde(default)]
    pub category: BlockCategory,
}

fn default_hardness() -> f32 { 1.0 }
fn default_tool() -> String { "any".to_string() }

/// Block categories for editor organization
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum BlockCategory {
    #[default]
    Natural,
    Ore,
    Building,
    Decoration,
    Fluid,
    Special,
}

impl BlockDefinition {
    /// Create a definition from an existing BlockType
    pub fn from_block_type(block_type: BlockType) -> Self {
        let id = block_type_to_id(block_type);
        let display_name = block_type.display_name().to_string();
        let numeric_id = block_type as u16;
        
        let (physics, visuals, category) = block_type_properties(block_type);
        
        Self {
            id,
            display_name,
            block_type: Some(block_type),
            numeric_id,
            physics,
            visuals,
            light_emission: crate::world::light_propagation::block_light_emission(block_type),
            hardness: block_type_hardness(block_type),
            tool_required: block_type_tool(block_type),
            category,
        }
    }
}

/// Map BlockType to string ID
fn block_type_to_id(bt: BlockType) -> String {
    match bt {
        BlockType::Air => "air",
        BlockType::Stone => "stone",
        BlockType::Dirt => "dirt",
        BlockType::Grass => "grass",
        BlockType::Sand => "sand",
        BlockType::Water => "water",
        BlockType::Wood => "wood",
        BlockType::Leaves => "leaves",
        BlockType::Sandstone => "sandstone",
        BlockType::Snow => "snow",
        BlockType::Ice => "ice",
        BlockType::Obsidian => "obsidian",
        BlockType::VolcanicRock => "volcanic_rock",
        BlockType::Cactus => "cactus",
        BlockType::SandDunes => "sand_dunes",
        BlockType::CopperOre => "copper_ore",
        BlockType::IronOre => "iron_ore",
        BlockType::SilverOre => "silver_ore",
        BlockType::GoldOre => "gold_ore",
        BlockType::Mud => "mud",
        BlockType::Clay => "clay",
        BlockType::Mycelium => "mycelium",
        BlockType::TerracottaRed => "terracotta_red",
        BlockType::TerracottaOrange => "terracotta_orange",
        BlockType::PackedDirt => "packed_dirt",
    }.to_string()
}

/// Get physics, visuals, and category for a BlockType
fn block_type_properties(bt: BlockType) -> (BlockPhysics, BlockVisuals, BlockCategory) {
    let physics = BlockPhysics {
        solid: bt.is_solid(),
        transparent: bt.is_transparent(),
        passable: !bt.is_solid() || bt == BlockType::Water,
    };
    
    // Get colors from meshing module
    let color = crate::world::meshing::block_color(bt);
    
    // Get texture tiles
    let tex = crate::world::texture_atlas::block_textures(bt);
    let visuals = BlockVisuals {
        color,
        texture_tiles: [tex.top, tex.bottom, tex.side],
        light_level: 0,
    };
    
    let category = match bt {
        BlockType::CopperOre | BlockType::IronOre | 
        BlockType::SilverOre | BlockType::GoldOre => BlockCategory::Ore,
        BlockType::Water => BlockCategory::Fluid,
        BlockType::Air => BlockCategory::Special,
        BlockType::Stone | BlockType::Dirt | BlockType::Grass |
        BlockType::Sand | BlockType::Snow | BlockType::Ice |
        BlockType::SandDunes | BlockType::VolcanicRock => BlockCategory::Natural,
        BlockType::Wood | BlockType::Leaves | BlockType::Cactus => BlockCategory::Natural,
        BlockType::Sandstone | BlockType::Obsidian => BlockCategory::Building,
        BlockType::Mud | BlockType::Clay | BlockType::Mycelium | BlockType::TerracottaRed | BlockType::TerracottaOrange | BlockType::PackedDirt => BlockCategory::Natural,
    };
    
    (physics, visuals, category)
}

/// Get hardness for a BlockType
fn block_type_hardness(bt: BlockType) -> f32 {
    match bt {
        BlockType::Air => 0.0,
        BlockType::Leaves => 0.2,
        BlockType::Grass | BlockType::Dirt | BlockType::Sand | 
        BlockType::Snow | BlockType::SandDunes | BlockType::Mud | BlockType::Clay | BlockType::Mycelium | BlockType::PackedDirt => 0.5,
        BlockType::Wood | BlockType::Cactus => 2.0,
        BlockType::Stone | BlockType::Sandstone | BlockType::Ice |
        BlockType::VolcanicRock => 3.0,
        BlockType::CopperOre => 2.0,
        BlockType::IronOre => 3.0,
        BlockType::SilverOre => 2.5,
        BlockType::GoldOre => 2.5,
        BlockType::Obsidian => 5.0,
        BlockType::TerracottaRed | BlockType::TerracottaOrange => 3.0,
        BlockType::Water => 0.0,
    }
}

/// Get required tool for a BlockType
fn block_type_tool(bt: BlockType) -> String {
    match bt {
        BlockType::Stone | BlockType::Sandstone | BlockType::Obsidian |
        BlockType::CopperOre | BlockType::IronOre | BlockType::SilverOre |
        BlockType::GoldOre | BlockType::VolcanicRock | BlockType::Ice => "pickaxe",
        BlockType::Dirt | BlockType::Grass | BlockType::Sand |
        BlockType::Snow | BlockType::SandDunes | BlockType::Mud | BlockType::Clay | BlockType::Mycelium | BlockType::PackedDirt => "shovel",
        BlockType::Wood | BlockType::Leaves | BlockType::Cactus => "axe",
        _ => "any",
    }.to_string()
}

// ============================================================================
// BLOCK REGISTRY
// ============================================================================

/// Registry of all block definitions
#[derive(Resource)]
pub struct BlockRegistry {
    /// Blocks by string ID
    blocks: HashMap<String, BlockDefinition>,
    /// Blocks by numeric ID (for fast lookup from chunks)
    by_numeric: HashMap<u16, String>,
    /// Next available numeric ID for custom blocks (Phase 2)
    #[allow(dead_code)]
    next_id: u16,
}

impl Default for BlockRegistry {
    fn default() -> Self {
        let mut registry = Self {
            blocks: HashMap::new(),
            by_numeric: HashMap::new(),
            next_id: 100, // Reserve 0-99 for built-in blocks
        };
        
        // Register all built-in BlockType variants
        registry.register_builtin_blocks();
        
        registry
    }
}

impl BlockRegistry {
    /// Register all built-in block types
    fn register_builtin_blocks(&mut self) {
        let builtins = [
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
            BlockType::CopperOre,
            BlockType::IronOre,
            BlockType::SilverOre,
            BlockType::GoldOre,
        ];
        
        for bt in builtins {
            let def = BlockDefinition::from_block_type(bt);
            self.blocks.insert(def.id.clone(), def.clone());
            self.by_numeric.insert(def.numeric_id, def.id.clone());
        }
        
        info!("BlockRegistry initialized with {} built-in blocks", self.blocks.len());
    }
    
    /// Get a block definition by string ID
    pub fn get(&self, id: &str) -> Option<&BlockDefinition> {
        self.blocks.get(id)
    }
    
    /// Get a block definition by numeric ID
    pub fn get_by_numeric(&self, id: u16) -> Option<&BlockDefinition> {
        self.by_numeric.get(&id).and_then(|s| self.blocks.get(s))
    }
    
    /// Get BlockType for a string ID (Phase 1 compatibility)
    pub fn get_block_type(&self, id: &str) -> Option<BlockType> {
        self.blocks.get(id).and_then(|def| def.block_type)
    }
    
    /// Iterate all blocks
    pub fn iter(&self) -> impl Iterator<Item = &BlockDefinition> {
        self.blocks.values()
    }
    
    /// Get all block IDs sorted
    pub fn ids_sorted(&self) -> Vec<String> {
        let mut ids: Vec<_> = self.blocks.keys().cloned().collect();
        ids.sort();
        ids
    }
    
    /// Count of registered blocks
    pub fn len(&self) -> usize {
        self.blocks.len()
    }
    
    /// Is registry empty
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
    
    /// Get blocks by category
    pub fn by_category(&self, category: BlockCategory) -> Vec<&BlockDefinition> {
        self.blocks.values()
            .filter(|b| b.category == category)
            .collect()
    }
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin for the block registry system
pub struct BlockPlugin;

impl Plugin for BlockPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BlockRegistry>();
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_block_registry_default() {
        let registry = BlockRegistry::default();
        assert!(registry.len() >= 19, "Should have at least 19 built-in blocks");
    }
    
    #[test]
    fn test_block_registry_get() {
        let registry = BlockRegistry::default();
        let stone = registry.get("stone");
        assert!(stone.is_some());
        assert_eq!(stone.unwrap().display_name, "Stone");
    }
    
    #[test]
    fn test_block_registry_get_by_numeric() {
        let registry = BlockRegistry::default();
        let stone = registry.get_by_numeric(1); // Stone = 1
        assert!(stone.is_some());
        assert_eq!(stone.unwrap().id, "stone");
    }
    
    #[test]
    fn test_block_definition_from_block_type() {
        let def = BlockDefinition::from_block_type(BlockType::IronOre);
        assert_eq!(def.id, "iron_ore");
        assert_eq!(def.category, BlockCategory::Ore);
        assert!(def.physics.solid);
    }
    
    #[test]
    fn test_block_categories() {
        let registry = BlockRegistry::default();
        let ores = registry.by_category(BlockCategory::Ore);
        assert!(ores.len() >= 4, "Should have 4 ore blocks");
    }
}
