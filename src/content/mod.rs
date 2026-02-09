//! Content System - Data-driven game content management
//!
//! This module provides a modular, extensible system for defining game content
//! (ores, blocks, biomes, structures) as data files rather than hardcoded values.
//!
//! # Architecture
//!
//! ```text
//! assets/content/
//!   ├── ores/           <- OreDefinition files (.ron)
//!   ├── blocks/         <- BlockDefinition files (.ron)
//!   └── user_content/   <- Player-created content (preserved on updates)
//!
//! ContentRegistry<T>
//!   ├── load_all()      <- Scan directory, parse RON files
//!   ├── get(id)         <- Lookup by string ID
//!   ├── iter()          <- Iterate all definitions
//!   └── save(def)       <- Write definition to disk
//! ```
//!
//! # Usage
//!
//! ```ignore
//! // Access ore registry
//! fn my_system(ores: Res<OreRegistry>) {
//!     if let Some(iron) = ores.get("iron_ore") {
//!         println!("Iron spawns at Y {}-{}", iron.generation.min_y, iron.generation.max_y);
//!     }
//! }
//! ```

pub mod ore;

use bevy::prelude::*;

pub use ore::{OreDefinition, OreGeneration, OreRegistry, BiomeFilter};

/// Plugin that initializes all content registries
pub struct ContentPlugin;

impl Plugin for ContentPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ore::OrePlugin);
    }
}
