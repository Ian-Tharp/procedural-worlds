# Procedural Worlds Engine

AI-driven voxel game engine for collaborative world-building, built with Rust and Bevy.

## Features

- Procedural terrain generation with multiple biomes
- Voxel-based world with chunk streaming
- Creature AI and combat system
- Crafting and inventory systems
- Weather and day/night cycles
- Content editor with ore definitions
- World persistence (save/load)

## World Persistence

**New in v0.2.0** — Save and load your worlds with full chunk and entity state.

### Saving
- Press **Ctrl+S** at any time to save the current world
- Saves are stored in the `saves/` directory as binary files

### Loading
- On startup, a world selector lets you choose from existing saves or create a new world
- Select any previously saved world to continue where you left off

### Notes
- Save format uses **bincode** for fast, compact binary serialization
- Saves from v0.1.x are **not compatible** with v0.2.0 — start a fresh world after upgrading
- Each save captures chunk data, player position, inventory, and world generation seed

## Building

```bash
cargo build --release
```

## Running

```bash
cargo run --release
```

## License

MIT
