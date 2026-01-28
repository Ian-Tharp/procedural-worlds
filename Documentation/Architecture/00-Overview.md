# Procedural Worlds Engine - Architecture Overview

> **Status**: Draft - Establishing foundational design
> **Last Updated**: January 2026

## Vision Statement

Procedural Worlds is an AI-driven voxel game engine designed for collaborative world-building between developers, players, and AI. The engine prioritizes emergent gameplay, moddability, and unique experiences through procedural generation and LLM integration.

## Technology Stack

| Layer | Technology | Purpose |
|-------|------------|---------|
| Language | Rust | Performance, safety, modern tooling |
| Framework | Bevy 0.15+ | ECS architecture, rendering, windowing |
| Graphics | wgpu | Cross-platform GPU (Vulkan/Metal/DX12/WebGPU) |
| UI | egui | Immediate-mode editor interface |
| Scripting | Python (PyO3) | Modding, AI integration, rapid iteration |
| Build | Cargo | Dependency management, compilation |

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Application Layer                         │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐  │
│  │   Editor    │  │    Game     │  │   Python Scripting      │  │
│  │    Mode     │  │    Mode     │  │   (PyO3 Bridge)         │  │
│  └─────────────┘  └─────────────┘  └─────────────────────────┘  │
├─────────────────────────────────────────────────────────────────┤
│                        Engine Systems                            │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────────────┐   │
│  │  World   │ │ Rendering│ │  Input   │ │    AI/LLM        │   │
│  │ Manager  │ │ Pipeline │ │  System  │ │   Integration    │   │
│  └──────────┘ └──────────┘ └──────────┘ └──────────────────┘   │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────────────┐   │
│  │  Chunk   │ │  Entity  │ │  Audio   │ │    Networking    │   │
│  │  System  │ │  System  │ │  System  │ │    (Future)      │   │
│  └──────────┘ └──────────┘ └──────────┘ └──────────────────┘   │
├─────────────────────────────────────────────────────────────────┤
│                         Bevy ECS Core                            │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │  Entities  │  Components  │  Systems  │  Resources       │   │
│  └──────────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────┤
│                        Platform Layer                            │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐  │
│  │    wgpu     │  │   winit     │  │   Platform APIs         │  │
│  │  (Graphics) │  │ (Windowing) │  │   (OS, Input, etc.)     │  │
│  └─────────────┘  └─────────────┘  └─────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

## Core Subsystems

### 1. World System
- **Chunk Management**: Loading, unloading, streaming of 16³ block chunks
- **Block Registry**: Type definitions, properties, behaviors
- **World State**: Persistence, serialization, world coordinates

### 2. Rendering System
- **Voxel Meshing**: Greedy meshing for chunk geometry
- **Materials**: Block textures, PBR-lite shading
- **Lighting**: Sun, ambient, block lighting (future: dynamic)
- **Post-Processing**: Shadows, AO, fog, color grading

### 3. Generation System
- **Terrain**: Noise-based heightmaps, biomes, features
- **Structures**: Procedural buildings, dungeons, villages
- **Vegetation**: Trees, plants, ecosystems
- **Python API**: Scriptable generation rules

### 4. Editor System
- **Viewport**: 3D scene view with camera controls
- **Inspector**: Entity/component property editing
- **World Tools**: Terrain painting, block placement
- **Asset Browser**: Textures, models, scripts

### 5. Entity System (Bevy ECS)
- **Characters**: Players, NPCs, creatures
- **Items**: Inventory, equipment, drops
- **Interactables**: Doors, chests, mechanisms

### 6. AI/LLM Integration (Future)
- **NPC Behavior**: Goal-driven AI with LLM reasoning
- **Narrative Generation**: Quests, dialogue, lore
- **World Events**: Emergent storylines
- **Mod Assistance**: Natural language to game content

## Data Flow

```
Input → Systems → World State → Meshing → Rendering → Display
          ↑                        ↓
       Events ←───── ECS ←───── Updates
```

## Plugin Architecture

Each major system is implemented as a Bevy Plugin:

```rust
// Example plugin structure
pub struct ChunkPlugin;
impl Plugin for ChunkPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChunkManager>()
           .add_systems(Update, (
               chunk_loading_system,
               chunk_meshing_system,
               chunk_unloading_system,
           ));
    }
}
```

## Module Organization

```
src/
├── main.rs              # Entry point
├── lib.rs               # Library exports (for testing)
├── engine/              # Core engine utilities
│   ├── mod.rs
│   ├── camera.rs
│   └── input.rs
├── world/               # Voxel world systems
│   ├── mod.rs
│   ├── chunk.rs
│   ├── block.rs
│   ├── meshing.rs
│   └── coordinates.rs
├── generation/          # Procedural generation
│   ├── mod.rs
│   ├── terrain.rs
│   ├── biomes.rs
│   └── structures.rs
├── rendering/           # Custom rendering (if needed)
│   ├── mod.rs
│   └── voxel_material.rs
├── editor/              # Editor UI and tools
│   ├── mod.rs
│   ├── viewport.rs
│   ├── inspector.rs
│   └── tools.rs
├── entities/            # Game entities
│   ├── mod.rs
│   ├── player.rs
│   └── npc.rs
└── scripting/           # Python integration
    ├── mod.rs
    └── api.rs
```

## Design Principles

1. **Modularity**: Systems are decoupled, communicate via ECS
2. **Data-Driven**: Behavior defined in data, not hardcoded
3. **Scriptable**: Key systems exposed to Python
4. **Performant**: Hot paths in Rust, scripting for logic
5. **Moddable**: Clear extension points for user content

## Open Questions

> These need to be resolved through discussion:

- [ ] Single-player only or multiplayer support?
- [ ] Scope of initial "Kingdoms" demo?
- [ ] When/how to integrate LLM features?
- [ ] Save format and world persistence?
- [ ] Target platforms (PC only? WebGL? Console?)

---

*This document will evolve as design decisions are made.*
