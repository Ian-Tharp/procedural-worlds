# Procedural Worlds: Vision Statement

> **Status**: Defined
> **Last Updated**: January 2026

## Elevator Pitch

**Procedural Worlds** is an open-world voxel sandbox that combines crafting, building, RPG progression, survival mechanics, and AI-driven emergent storytelling. Unlike scripted games, the world's NPCs and factions are powered by LLMs with persistent memory, creating unique narratives and consequences that emerge organically from player actions.

## The Dream

> *"I want this game to be that epic fantasy - what I feel like should exist, blending the aspects of the fantasy I grew up with in World of Warcraft, but with the endless possibilities of Minecraft."*

This is the game that captures the wonder of standing in Stormwind for the first time, the freedom of building anything in Minecraft, the tactical depth of Fire Emblem, and the survival tension of Rust - all in a world where NPCs remember you, factions rise and fall, and your story is truly your own.

## Core Pillars

### 1. Open-World Sandbox
- Procedurally generated voxel world
- Freedom to explore, build, craft, survive
- No forced linear progression
- Player-driven goals and emergent gameplay

### 2. RPG & Magic Systems
- Character progression and skills
- **Glyph-based spell crafting** (Form + Effect + Augment, inspired by Ars Nouveau)
- Equipment, crafting, and itemization
- Combat with tactical/strategic elements
- Magic has rules, costs, and consequences (Magic as System)

### 3. Survival Crafting
- Resource gathering and processing
- Building and construction
- Environmental challenges
- Progression through crafting tiers

### 4. Living World with Factions
- NPCs with goals, relationships, personalities
- Factions that rise, fall, ally, and war
- World that evolves with or without player
- Consequences that persist and ripple

### 5. AI-Driven Emergent Narrative
- LLM-powered NPC behavior and dialogue
- Persistent memory for actors (NPCs remember)
- Dynamic events shaped by AI reasoning
- No scripted quests - stories emerge organically
- True immersion through intelligent world actors

## The Unique Value Proposition

> "An open sandbox no longer has barriers to creating a fully immersive and dynamic world with consequence."

Traditional sandbox games either:
- Have scripted content that runs out
- Have emergent systems but dumb NPCs
- Rely on players to create all meaning

Procedural Worlds bridges this gap by giving AI actors:
- **Memory**: They remember what happened
- **Goals**: They pursue their own objectives
- **Reasoning**: They make decisions that make sense
- **Consequence**: Their actions shape the world

The result: A world that tells infinite unique stories.

## Multiplayer Architecture

### Tier 1: Single-Player Offline (Priority)
- Full game experience offline
- Local world saves
- Foundation for all other modes

### Tier 2: LAN / Peer-to-Peer
- Play with friends locally
- Host from your machine
- Minecraft-style "Open to LAN"

### Tier 3: Dedicated Servers
- Self-hosted persistent worlds
- Support for hundreds of players
- Community-run servers

### Future Consideration: MMO Scale
- Not in initial scope
- Architecture should not preclude it
- Evaluate after core game works

## Inspirations & What We Take From Each

| Game | Inspiration |
|------|-------------|
| **Minecraft** | Voxel building, crafting depth, modding ecosystem, survival loop |
| **Hytale** | Visual style, creative tools, modern polish, zone-based world |
| **World of Warcraft** | Living world feel, factions, RPG progression, sense of place |
| **Fire Emblem** | Tactical/strategic depth, meaningful choices, character relationships |
| **Rust** | Survival tension, player interaction, base building, persistence |
| **Lay of the Land** | Physics simulation, atmospheric world, procedural terrain |

## Core Gameplay Loop

```
┌─────────────────────────────────────────────────────────────┐
│                                                             │
│  EXPLORE ──→ DISCOVER ──→ GATHER ──→ CRAFT ──→ BUILD      │
│     ↑                                              │        │
│     │         ┌──────────────────────────┐        ↓        │
│     │         │   EMERGENT NARRATIVE     │    SURVIVE      │
│     │         │   (AI-driven events,     │        │        │
│     │         │    faction dynamics,     │        │        │
│     │         │    NPC relationships)    │        │        │
│     │         └──────────────────────────┘        │        │
│     │                    ↓                        ↓        │
│     └────── PROGRESS ←── QUEST ←── ENCOUNTER ←───┘        │
│              (skills, gear, reputation, territory)         │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

Players can engage with any part of this loop at any time. The AI-driven narrative layer intersects all activities, creating meaningful context and consequences.

## What This Is NOT

- ❌ A Minecraft clone with better graphics
- ❌ A scripted RPG with a set story
- ❌ A pure survival game focused on grind
- ❌ A creative mode building toy
- ❌ An AI tech demo without solid gameplay

## Success Criteria

The game succeeds when players:
1. Lose track of time exploring and building
2. Tell stories about what happened to them (not what the game showed them)
3. Feel that NPCs are "real" with their own lives
4. Return to see how the world evolved
5. Share unique experiences that no other player had

## The "Wow Moment"

The defining experience of Procedural Worlds:

> *Player stands on a hill at the edge of the starting area. The world stretches to the horizon - mountains, forests, distant kingdoms visible in the haze. A village smokes in the valley, a tower glints on a far peak.*
>
> *"I can go anywhere I see."*
>
> *They begin walking toward the distant mountain. Hours later, they climb it. The view from the top reveals even more world. NPCs in the village remember the stranger who passed through yesterday.*

This requires:
- **Vast draw distances** (LOD system inspired by Distant Horizons)
- **Seamless world streaming**
- **Persistent NPC memory**
- **Procedural content that holds up at every scale**

## World Structure

### The Material Plane Split

The world consists of two interconnected realms (yin and yang):

| Realm | Description | Gameplay Style |
|-------|-------------|----------------|
| **Aloryith** | Ordered regions with established civilizations | Themepark - crafted zones, structured content |
| **The Wilderness** | Untamed, procedurally generated territories | Sandbox - player kingdoms, emergent gameplay |

Events in one affect the other. The AI narrative engine weaves stories across both.

### Multiple Planes of Existence

Beyond the Material Plane:
- **Spirit Plane**: Where memory shapes existence, Spirit Guides bind shattered souls
- **Shadow Plane**: Realm of darkness and twisted magic
- **Celestial Plane**: Divine realm, source of humanity's origin

## The Spirit System

A unique mechanic where **memory shapes existence**:

- When something dies, its spirit shatters into pieces
- Spirit Guides can rebind these pieces using Spirit Magic
- **If no one remembers an entity, its spirit is lost forever**
- Players and NPCs who remember someone keep their spirit alive

**Gameplay Implication**: Social interaction matters. NPCs you befriend remember you, and you remember them. This creates real stakes for relationships.

## Narrative Framework

### Epic Fantasy Principles

Stories follow the Epic Fantasy Narrative Framework:

1. **Grandeur without Simplicity** - Large scale with nuanced depth
2. **Moral Complexity without Cynicism** - Meaningful consequences, not nihilism
3. **Magic as System** - Magic has rules and costs
4. **Heroic Agency** - Player actions shape the world
5. **Living History** - The past matters to the present
6. **Meaningful Relationships** - NPCs remember and change
7. **Stakes and Sacrifice** - Victories have cost

### Not Black and White

The Tri-Faction conflict (Aloryian Accord, Elidinar Union, Garinai Pact) isn't about good vs evil - it's about control, freedom, survival, and differing visions of the future.

---

## Related Documentation

- [Kingdoms of Aloryith Lore](../Research/Kingdoms-of-Aloryith-Lore.md) - Source worldbuilding
- [Epic Fantasy Narrative Framework](../Research/Epic-Fantasy-Narrative-Framework.md) - Storytelling principles
- [Ars Nouveau Research](../Research/Ars-Nouveau-Research.md) - Magic system inspiration
- [Distant Horizons Research](../Research/Distant-Horizons-Research.md) - LOD/draw distance tech

---

*This vision guides all design and implementation decisions.*
