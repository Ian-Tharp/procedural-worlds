# Procedural Worlds - Project Understanding

## Document Purpose

This document captures the current state and understanding of the Procedural Worlds project as of January 2026, synthesizing information from the codebase, Obsidian vault documentation, and the user's stated vision.

## Project Vision Summary

**Procedural Worlds** is an innovative AI-driven game engine designed for collaborative world-building between developers, players, and AI. The core philosophy emphasizes:

1. **Player and Developer Empowerment** - Blur the line between developer and player
2. **AI-Native Game Design** - LLMs as first-class citizens, not afterthoughts
3. **Modular, Extensible Architecture** - C++ performance with Python flexibility
4. **Emergent Storytelling** - Unique worlds and narratives every playthrough

## Inspirational Games

### Primary Inspirations
| Game | Key Influence |
|------|---------------|
| **Minecraft** | Voxel scale, modding ecosystem, building mechanics |
| **Hytale** | Creative tools, visual style, zone system, modern engine |
| **Lay of the Land** | Physics simulation, procedural tools, immersive crafting |
| **World of Warcraft** | MMO structure, living world feel, factions |
| **Fire Emblem** | Strategic gameplay elements, character systems |
| **Rust** | Survival mechanics, player interaction, base building |

### Voxel Style Decision
User preference aligns with **Minecraft/Hytale scale** (1m voxels) rather than the more granular approach of Lay of the Land. This provides:
- Familiar gameplay feel
- Better performance characteristics
- Established rendering techniques
- Easier content creation

## Current Implementation Status

### Completed Systems
- SDL2 window management and input handling
- OpenGL 3.3 rendering context (with 2.1 fallback)
- ImGui editor interface with menu bar
- Viewport system with framebuffer rendering
- Python interpreter embedding (pybind11)
- CMake build system with vcpkg integration
- Basic project structure

### Architecture
```
Engine/
├── include/           # Public headers
│   ├── EngineGUI.h   # Editor UI management
│   ├── Viewport.h    # Off-screen rendering
│   └── PythonInterface.h
├── src/              # Implementations
│   ├── main.cpp      # Entry point, main loop
│   ├── EngineGUI.cpp
│   ├── Viewport.cpp
│   └── PythonInterface.cpp
├── scripts/          # Python scripts (empty)
├── assets/           # Game assets (empty)
└── third_party/      # External code
    └── imgui_backends/
```

### Technology Stack
- **C++17** - Core engine
- **OpenGL 3.3** - Graphics rendering
- **SDL2** - Window/input management
- **ImGui** - Editor UI
- **pybind11** - Python integration
- **vcpkg** - C++ dependency management
- **uv** - Python dependency management
- **CMake** - Build system

## Development Roadmap (From Obsidian)

### Phase 1: Core Rendering (CURRENT)
- [ ] Implement basic shader system
- [ ] Add mesh loading (OBJ/FBX)
- [ ] Create camera system
- [ ] Implement basic lighting
- [ ] Add texture loading

### Phase 2: Entity-Component System
- [ ] Design ECS architecture
- [ ] Transform component
- [ ] Mesh renderer component
- [ ] Material system
- [ ] Scene graph

### Phase 3: Procedural Generation Foundation
- [ ] Noise library integration
- [ ] Terrain generation
- [ ] Vegetation placement
- [ ] Procedural meshes
- [ ] Python API for generation

### Phase 4: Editor Tools
- [ ] Object selection/manipulation
- [ ] Property inspector
- [ ] Asset browser
- [ ] Scene hierarchy
- [ ] Terrain painting

### Phase 5: Advanced Features
- [ ] Physics integration
- [ ] Audio system
- [ ] Particle system
- [ ] Post-processing
- [ ] Networking

## Key Design Decisions

### Why C++ Over Rust?
The project already has C++ foundation with:
- Working SDL2/OpenGL setup
- pybind11 integration
- CMake/vcpkg ecosystem in place

Switching to Rust would require complete rewrite with uncertain benefits.

### Why Python for Scripting?
1. Excellent AI/ML library ecosystem
2. Rapid prototyping capability
3. Accessible to non-programmers
4. Hot-reload potential
5. LLM integration ease

### Editor-First Development
Building tools alongside engine ensures:
- Immediate visual feedback
- Dogfooding the API
- Earlier identification of usability issues
- Parallel development of features and workflows

## AI Integration Plans

### LLM Use Cases (From Vision)
1. **Procedural Narrative Engine** - Generate lore, quests, events
2. **Contextual World Generation** - AI-populated settlements and characters
3. **Emergent NPCs** - Goals, relationships, personalities on-the-fly
4. **AI-Assisted Modding** - Natural language to game content
5. **Generational Storytelling** - World states inform future generations

### Python as AI Bridge
Python scripting enables:
- Direct integration with OpenAI, Anthropic APIs
- Local LLM hosting (llama.cpp, etc.)
- ML frameworks (PyTorch, transformers)
- Easy experimentation with AI features

## "Kingdoms" Demo Project

The first showcase game using the engine:
- Emergent world history
- Player-driven factions
- Dynamic kingdoms
- Collaborative storytelling

Not bound to previous "Kingdoms of Aloryith" concepts - fresh creative start.

## Design Decisions (Confirmed January 2026)

### Chunk Size: 16x16x16
- Proven Minecraft-style approach
- Good balance of memory and performance
- Well-understood rendering techniques
- Enables efficient LOD systems

### Physics: Moderate Simulation
- Water flow simulation (priority)
- Basic structural physics
- NOT full Lay of the Land-style simulation
- Performance over simulation depth

### Visual Style: Stylized (Hytale-like)
- Clean, modern voxel aesthetic
- Readable at distance
- Cinematic potential
- Consistent art direction

### AI Integration: Later Phase
- Focus on core engine first
- Design systems to be AI-hookable
- Add LLM features as experimental after core works
- Python scripting provides future bridge

## Next Steps

Immediate priorities based on roadmap:
1. Implement shader system for proper 3D rendering
2. Create basic mesh loading capability
3. Implement camera controls (FPS/orbit)
4. Begin terrain representation system

---

*This document should be updated as the project evolves and design decisions are made.*
