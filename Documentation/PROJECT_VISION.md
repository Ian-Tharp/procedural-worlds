# Procedural Worlds - Project Vision & Design Document

## Overview

Procedural Worlds is an AI-driven voxel game engine combining elements from:
- **World of Warcraft**: Rich world, progression, multiplayer potential
- **Minecraft**: Voxel building, procedural generation, exploration
- **Fire Emblem**: Strategic combat, character relationships, narrative depth

Built with **Rust + Bevy 0.15 + wgpu** for performance and modern architecture.

---

## Core Design Principles

### 1. Game Feel First
Movement and controls must feel responsive and satisfying. Reference games:
- Minecraft: Snappy auto-jump, precise block placement
- Titanfall: Fluid movement, momentum preservation
- Dark Souls: Weighty, deliberate actions with clear feedback

### 2. Emergent Gameplay
Systems should interact to create unexpected possibilities. The world should feel alive and reactive.

### 3. AI-Driven Content
Use AI for:
- Procedural narrative generation
- Dynamic NPC behaviors
- Adaptive difficulty
- World events that respond to player actions

### 4. Performance Without Compromise
Target 60+ FPS with:
- Efficient chunk streaming
- LOD systems for terrain and collision
- Async generation off main thread

---

## Player Controller Design

### Movement Values (Minecraft-inspired)
| Parameter | Value | Notes |
|-----------|-------|-------|
| Eye Height | 1.62 blocks | Minecraft standard |
| Player Height | 1.8 blocks | Full collision box |
| Walk Speed | 4.3 blocks/sec | Minecraft walking |
| Sprint Speed | 5.6 blocks/sec | Minecraft sprinting |
| Jump Height | 1.25 blocks | Clears 1 block with margin |
| Gravity | 32 blocks/sec² | Minecraft-like |
| Step Height | 0.6 blocks | Auto-step small obstacles |

### Control Modes

#### Walking Mode (Default)
- WASD: Horizontal movement (projected onto XZ plane)
- Space: Jump (when grounded)
- Mouse: Always controls camera rotation
- Shift: Sprint
- Ctrl: Crouch (future)

#### Flying Mode (Creative)
- WASD: Move in look direction
- Space: Ascend
- Ctrl: Descend
- Mouse: Camera rotation
- Shift: Fast fly

### Camera Behavior
- Mouse captured by default (FPS-style)
- ESC: Release cursor, show menu
- Click viewport: Recapture mouse
- Smooth rotation (slight filtering to prevent jitter)
- No smoothing on position when stepping (snappy auto-step)

---

## Technical Architecture

See [ARCHITECTURE_PLAN.md](./ARCHITECTURE_PLAN.md) for detailed actor/entity system design.

### Module Structure
```
src/
├── main.rs           # App entry, plugin registration
├── actors/           # Entity/actor component system
│   ├── mod.rs        # Actor, Player, Npc markers, CapsuleCollider
│   └── player.rs     # PlayerBundle, spawn helpers
├── engine/           # Core systems (camera, input)
│   └── mod.rs        # CameraController, cursor capture
├── world/            # Chunks, blocks, meshing
│   ├── mod.rs        # Chunk data, WorldPlugin, block queries
│   └── meshing.rs    # Mesh generation
├── generation/       # Procedural generation
│   └── mod.rs        # Terrain, caves, biomes
├── physics/          # Collision, movement
│   └── mod.rs        # Gravity, jumping, collision
└── editor/           # UI panels, debug tools
    └── mod.rs        # egui-based editor UI
```

### Key Systems
1. **Chunk Streaming**: Load/unload based on player position
2. **Terrain Generation**: Simplex noise with fractal octaves
3. **Mesh Generation**: Naive culled-face (greedy meshing planned)
4. **Collision**: Heightmap-based (block-level planned)

---

## Lessons Learned

### Terrain Collision
- Terrain height returns Y of surface BLOCK, not top of block
- Player feet should be at `terrain_height + 1.0` to stand ON the block
- Always enforce terrain height AFTER camera interpolation to prevent clipping

### Movement Feel
- Lerp-based smoothing feels floaty for vertical movement
- Auto-step should be snappy/discrete, not smooth
- Horizontal smoothing is good, vertical should be immediate when stepping

### Camera Controls
- FPS games capture mouse by default
- Right-click-to-look feels like an editor, not a game
- ESC to release cursor is standard UX

---

## Future Roadmap

### Phase 1: Core Engine (Current)
- [x] Basic terrain generation
- [x] Chunk streaming
- [x] Player movement with collision
- [ ] Block placement/destruction
- [ ] Proper FPS camera controls

### Phase 2: World Systems
- [ ] Biome system
- [ ] Cave generation improvements
- [ ] Water physics
- [ ] Day/night cycle
- [ ] Weather

### Phase 3: Gameplay
- [ ] Inventory system
- [ ] Crafting
- [ ] NPCs and AI
- [ ] Combat system
- [ ] Quest/narrative system

### Phase 4: Polish
- [ ] Greedy meshing optimization
- [ ] Async chunk generation
- [ ] LOD terrain rendering
- [ ] Audio system
- [ ] Particle effects

---

## Development Guidelines

### Code Quality
- Write unit tests for all game logic (same-file `#[cfg(test)]` modules)
- Use Bevy's ECS patterns consistently
- Document public APIs with `///` comments
- Keep systems focused and composable

### Self-Improvement
- Continuously refine based on playtesting
- Question assumptions about what "feels right"
- Research real game implementations (GDC talks, open source)
- Iterate rapidly on game feel

### Collaboration
- Document decisions and rationale
- Keep this file updated as the project evolves
- Use clear naming that explains intent
- Comment "why" not "what"
