# Ars Nouveau Research

## Overview

Ars Nouveau is a Minecraft magic mod that features a unique spell crafting system where players combine "glyphs" to create custom spells. The mod is known for its depth, visual polish, and integration with the game world.

**Key Facts:**
- Platform: Minecraft (Forge/Fabric)
- Style: Spell crafting through glyph combination
- Core Philosophy: Player creativity in magic system design

## Spell Crafting System

### Glyph Categories

The spell system uses three types of glyphs that combine to form complete spells:

#### 1. Forms (How the spell is cast)
Forms determine the delivery method of the spell:
- **Projectile**: Shoots a spell bolt that triggers on impact
- **Touch**: Cast at close range through direct contact
- **Self**: Cast on yourself
- **AOE (Area of Effect)**: Affects an area around a point
- **Beam**: Continuous stream of magic

#### 2. Effects (What the spell does)
Effects are the actual magical actions:
- **Break**: Destroys blocks
- **Harm**: Deals damage to entities
- **Heal**: Restores health
- **Launch**: Propels entities upward
- **Light**: Creates light sources
- **Grow**: Accelerates plant growth
- **Harvest**: Collects crops
- **Ignite**: Sets targets on fire
- **Freeze**: Applies frost/slowing effects
- **Summon**: Creates temporary creatures
- Many more...

#### 3. Augments (Modifiers)
Augments modify how forms and effects work:
- **Amplify**: Increases effect strength
- **Dampen**: Decreases effect (reduces mana cost)
- **Extend Duration**: Makes effects last longer
- **Pierce**: Allows projectiles to pass through targets
- **Split**: Divides spell into multiple smaller effects
- **Sensitive**: Makes spells trigger on entities instead of blocks
- **AOE Expand**: Increases area of effect size

### Spell Construction

Spells are built by chaining glyphs together:
```
[Form] -> [Effect] -> [Augment(s)]

Example: Projectile -> Break -> Amplify -> Amplify
Creates a projectile that breaks blocks with enhanced strength
```

**Key Rules:**
- Multiple effects can chain together
- Augments apply to the preceding effect
- Forms can be combined (Projectile -> Touch creates a projectile that triggers a touch spell on impact)
- Mana cost scales with complexity and augments

### Spellbook System

Spells are stored and cast from tiered spellbooks:
1. **Novice Spellbook**: Limited glyph slots, lower mana pool
2. **Apprentice Spellbook**: More slots, moderate mana
3. **Archmage Spellbook**: Maximum slots, large mana pool

Players can store multiple spells and switch between them.

## Mana System

### Mana Pool
- Each player has a mana pool that regenerates over time
- Pool size increases with progression
- Mana can be boosted by:
  - Wearing magical armor
  - Standing near mana sources
  - Consuming mana-restoring items

### Mana Cost Calculation
- Base cost per glyph type
- Augments multiply costs
- More complex spells = higher costs
- Dampen augment reduces costs

## Progression System

### Learning Glyphs
Glyphs are unlocked through:
1. **Glyph Recipes**: Crafted at special stations
2. **Exploration**: Found in world structures
3. **Research**: Unlocked through a knowledge book

### Tiered Unlocks
- Early game: Basic forms and simple effects
- Mid game: More powerful effects, first augments
- Late game: Rare effects, powerful augments

## Familiar System

Ars Nouveau includes magical companions:
- **Starbuncle**: Collects items
- **Whirlisprig**: Helps with farming
- **Drygmy**: Combat assistance
- **Wixie**: Potion brewing automation

## World Integration

### Source Gems
- Magical crystals found in the world
- Required for crafting and rituals
- Different tiers: Amethyst -> Source Gem -> Arcane Crystal

### Ritual System
- Large-scale magical effects
- Requires setup of ritual circles
- Consumes significant resources
- Examples: Weather control, summoning bosses, area enchantment

### Magical Flora
- Sourcebery bushes (generate mana)
- Archwood trees (magical wood)
- Magebloom (crafting component)

## Key Takeaways for Procedural Worlds

### 1. Glyph-Based Spell Construction
The Form + Effect + Augment system is elegant and highly extensible:
- **Forms**: Could map to casting methods (touch, projectile, area, ritual)
- **Effects**: The actual magical actions (damage, heal, transform, summon)
- **Augments**: Modifiers that scale or alter effects

### 2. Mana as Resource Management
- Mana pool creates strategic decision-making
- Regeneration rate affects pacing
- Equipment/progression increases capacity

### 3. Discovery-Based Learning
- Not all spells available immediately
- Exploration and progression unlock new options
- Creates sense of magical growth

### 4. Visual Feedback
- Each glyph type has distinct visual effects
- Particles, colors, and sounds communicate spell properties
- Important for player understanding

### 5. Integration with World Systems
- Magic interacts with blocks, entities, farming, automation
- Not isolated system - connects to core gameplay loops

## Potential Adaptation

For Procedural Worlds' magic system:

```
Spell = [Casting Form] + [Primary Effect] + [Augments*]

Example Spells:
- "Fireball": Projectile + Ignite + Amplify + AOE
- "Healing Touch": Touch + Heal + Extend Duration
- "Frost Nova": Self + Freeze + AOE Expand + AOE Expand
- "Magic Missile": Projectile + Harm + Split + Pierce
```

### Unique Additions for Our Vision
1. **Memory Glyphs**: Spells tied to NPC memories (AI integration)
2. **Faction Glyphs**: Unique effects locked to faction reputation
3. **Emergent Combinations**: AI discovers new spell combinations
4. **Environmental Glyphs**: Effects that change based on biome/plane

## Sources

- Ars Nouveau Wiki
- Minecraft Mod Community Documentation
- Player-created guides and tutorials
