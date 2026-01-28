# Visual Style Guide - Procedural Worlds

## Art Direction

### Core Aesthetic: Low-Poly Stylized Voxels

The visual style aims to bridge the gap between Minecraft's blockiness and more realistic game aesthetics, drawing heavy inspiration from Hytale's approach while adding our own distinctive low-poly twist.

### Key Characteristics

1. **Stylized Realism**
   - Low-poly models within voxel framework
   - Smooth/beveled edges on blocks where appropriate
   - Not purely cubic like Minecraft
   - Not hyper-realistic

2. **Hytale Influence**
   - Clean, readable visuals
   - Modern voxel aesthetic
   - Cinematic potential
   - Strong silhouettes

3. **Low-Poly Elements**
   - Adds subtle realism
   - Creates unique identity
   - Distinct from pure Minecraft clone
   - Allows for more organic shapes

4. **Voxel Scale**
   - 1 meter blocks (Minecraft/Hytale standard)
   - NOT sub-meter granular voxels
   - Maintains familiar gameplay feel

### Visual Goals

| Aspect | Description |
|--------|-------------|
| Blocks | Cube-based with potential for beveled edges or vertex smoothing |
| Terrain | Procedural with natural-looking formations |
| Lighting | Stylized with strong ambient and directional |
| Colors | Saturated but not garish, painterly feel |
| Atmosphere | Fantasy/mystical vibe, immersive world |

### Technical Approach

1. **Block Rendering**
   - Standard cube mesh as base
   - Potential for ambient occlusion
   - Per-vertex colors or texture atlases
   - Consider smooth lighting between blocks

2. **Materials**
   - PBR-lite (physically based but simplified)
   - Stylized roughness values
   - Avoid pure realism
   - Consistent art direction

3. **Post-Processing (Future)**
   - Subtle bloom
   - Color grading for mood
   - Ambient occlusion
   - Fog for atmosphere

### Inspiration Reference

**Hytale**
- Clean, modern voxel look
- Strong art direction
- Readable at distance
- Cinematic quality

**Lay of the Land**
- Organic terrain shapes
- Atmospheric lighting
- Mystical world feel

**Minecraft (Foundation)**
- Cube-based simplicity
- Familiar voxel scale
- Clear block identity

### NOT These Styles

- Photorealistic (too serious)
- Purely cubic Minecraft clone (too derivative)
- Hyper-granular voxels (too complex, performance issues)
- Generic low-poly (needs voxel identity)

---

## Implementation Notes

### Phase 1: Foundation
- Use basic cube blocks
- Simple solid colors
- Establish rendering pipeline

### Phase 2: Visual Enhancement
- Add texture atlas
- Implement ambient occlusion
- Add smooth lighting

### Phase 3: Polish
- Block edge beveling (optional)
- Post-processing effects
- Atmospheric rendering

---

*This document captures the visual direction for Procedural Worlds. Update as the art style evolves.*
