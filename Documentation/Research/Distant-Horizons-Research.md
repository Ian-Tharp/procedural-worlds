# Distant Horizons Research

## Overview

Distant Horizons is a Minecraft mod that dramatically increases render distance by using a Level of Detail (LOD) system. It allows players to see terrain hundreds of chunks away while maintaining playable framerates.

**Key Facts:**
- Platform: Minecraft (Fabric/Forge)
- Purpose: Extreme render distance through LOD
- Achievement: 512+ chunk render distances (vs vanilla ~16-32)

## Core Technology

### Level of Detail (LOD) System

The fundamental concept: render distant terrain at lower detail levels.

#### LOD Levels
Objects/terrain are rendered with progressively less detail based on distance:
- **Full Detail (Near)**: Standard voxel rendering, all blocks visible
- **Medium Detail**: Simplified geometry, reduced block types
- **Low Detail**: Heavily simplified, color-averaged surfaces
- **Distant**: Billboard/impostor rendering

#### Distance Bands
The world is divided into concentric rings:
```
Player -> [Full Detail] -> [LOD 1] -> [LOD 2] -> [LOD 3] -> [Horizon]
           16 chunks       32-64      64-128     128-256     256-512+
```

### Chunk Simplification

#### Block Merging
Distant chunks merge similar blocks:
- Groups of same-type blocks become single larger faces
- Colors are averaged across regions
- Essential shapes preserved, fine details removed

#### Heightmap-Based Rendering
For very distant terrain:
- Only surface heightmap is rendered
- No interior blocks computed
- Dramatic reduction in geometry

### Asynchronous Generation

Critical for performance:
- LOD chunks generate on background threads
- Never blocks main game thread
- Progressive loading (nearest first)
- Caching of generated LOD data

### Culling Optimizations

- **Frustum Culling**: Only render what's in view
- **Occlusion Culling**: Skip terrain hidden behind mountains
- **Distance Culling**: Hard cutoff for extreme distances

## Visual Quality

### Color Preservation
- LOD maintains biome colors
- Water, grass, leaves retain characteristic hues
- Creates visually coherent distant views

### Lighting
- Simplified lighting model for distant terrain
- Ambient occlusion approximation
- Day/night cycle affects LOD terrain

### Fog Integration
- Distance fog blends LOD transitions
- Atmospheric haze masks LOD boundaries
- Configurable fog distance and density

## Performance Characteristics

### Memory Usage
- LOD data is compact (8-16 bytes per LOD chunk vs thousands for full)
- Disk caching reduces memory footprint
- Configurable memory limits

### GPU Impact
- Dramatically fewer triangles for distant terrain
- Single draw calls for merged geometry
- Shader-based detail fading

### CPU Impact
- Background LOD generation
- Chunk merging algorithms
- Caching/loading management

## Configuration Options

### Quality Settings
- LOD detail levels (how much simplification)
- Transition distances
- Update frequency

### Performance Settings
- Maximum render distance
- Memory allocation
- Thread count for generation

### Visual Settings
- Fog density
- LOD blending smoothness
- Vertical render distance

## The "Wow Moment"

Distant Horizons creates the experience of:
- Standing on a mountain and seeing the entire continent
- "That mountain? I can go there"
- Sense of a truly vast, explorable world
- Epic fantasy scale realized in gameplay

## Key Takeaways for Procedural Worlds

### 1. Multi-Level LOD System
Implement at least 3-4 LOD levels:
```rust
enum LodLevel {
    Full,      // All blocks, standard rendering
    Medium,    // 2x2x2 block groups, simplified
    Low,       // 4x4x4 block groups, averaged colors
    Distant,   // Heightmap only, impostor rendering
}
```

### 2. Asynchronous Generation Pipeline
```
Main Thread          Background Threads
     |                      |
     |--Request LOD-------->|
     |                      |--Generate LOD
     |                      |--Simplify mesh
     |<--LOD Ready----------|
     |--Upload to GPU
```

### 3. Chunk Data Structure for LOD
Store multiple detail levels per region:
```rust
struct ChunkRegion {
    full_chunks: Vec<Chunk>,      // 16x16x16 blocks
    lod1_data: LodChunk,          // 32x32x32 averaged
    lod2_data: LodChunk,          // 64x64x64 averaged
    heightmap: Heightmap,          // For distant rendering
}
```

### 4. Smooth Transitions
- Cross-fade between LOD levels
- Avoid "popping" as detail changes
- Fog helps mask transitions

### 5. Prioritized Loading
```
Priority Order:
1. Chunks player is moving toward
2. Chunks in player's view frustum
3. Nearest unloaded chunks
4. Background fill
```

### 6. Memory-Efficient Storage
- Compress LOD data
- Cache to disk
- Stream on demand
- Unload distant LOD when memory constrained

## Implementation Considerations

### For Bevy/Rust
- Use Bevy's async task system for background generation
- Implement custom mesh LOD component
- Consider compute shaders for LOD generation
- Leverage wgpu's instancing for distant objects

### Chunk Streaming Architecture
```
World Manager
    |
    |-- ChunkLoader (full detail, near player)
    |-- LODGenerator (background, progressive)
    |-- LODCache (disk + memory)
    |-- MeshManager (GPU uploads, pooling)
```

### The Vision: Procedural Worlds' Draw Distance

Target experience:
- Player stands on starter hill
- Sees procedurally generated mountains 5km away
- Thinks "I want to climb that"
- Begins walking, world streams in seamlessly
- Arrives at mountain, it matches what they saw from afar

This creates the "wow moment" - the sense of an infinite, explorable world.

## Integration with AI/Emergent Narrative

LOD system enables AI to:
- Reference visible landmarks in dialogue
- Create quests pointing to visible distant locations
- NPCs can "see" the same world player sees
- Faction territories visible on horizon

## Sources

- Distant Horizons Mod Documentation
- Curseforge/Modrinth mod pages
- Player configuration guides
- LOD rendering technique papers
