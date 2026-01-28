# Procedural Worlds: Actor/Entity Architecture Plan

## Executive Summary

This document outlines the architectural redesign needed to implement proper player collision with a capsule collider, establish OOP-like patterns in Bevy's ECS, and create a foundation for future entity types (NPCs, mobs, projectiles).

---

## Current Problems

### 1. Camera-as-Player Anti-pattern
Currently, `CameraController` is attached directly to `Camera3d`. This conflates:
- **View** (where we look from) with **Body** (physical presence in world)
- Makes it impossible to have third-person view
- Camera lerping affects collision position
- No separation of concerns

### 2. Heightmap-Only Collision
Current collision uses `estimate_terrain_height()` which:
- Only checks Y at a single point
- Ignores actual block data (can't detect caves, overhangs)
- No horizontal collision (can walk through walls)
- No head collision (can jump into ceilings)

### 3. Player Height Issues
- Eye height at `1.62` blocks but standing ON block requires `+1.0` offset
- Complex math split between engine/mod.rs and physics/mod.rs
- Player center position unclear (is it feet, center, or eyes?)

---

## Proposed Architecture

### Core Design: Component-Based Actors

Instead of OOP inheritance, Bevy uses **composition over inheritance**. We'll create marker components and bundles that compose behaviors:

```
┌─────────────────────────────────────────────────────────────────┐
│                         Actor System                            │
├─────────────────────────────────────────────────────────────────┤
│  Marker Components:     │  Data Components:       │  Behaviors: │
│  ─────────────────────  │  ───────────────────    │  ────────── │
│  • Actor               │  • Transform           │  Systems    │
│  • Player              │  • Velocity            │  query by   │
│  • NPC                 │  • CapsuleCollider     │  component  │
│  • Mob                 │  • Health              │  combos     │
│  • Projectile          │  • Movement            │             │
└─────────────────────────────────────────────────────────────────┘
```

### Entity Hierarchy: Player

```
Player (Entity)
├── Components:
│   ├── Actor                    # Marker: is a game actor
│   ├── Player                   # Marker: is the player specifically
│   ├── Transform                # Position in world (feet position)
│   ├── Velocity                 # Current velocity (Vec3)
│   ├── CapsuleCollider          # Collision shape
│   ├── Movement                 # Movement capabilities (speeds, jump)
│   ├── Grounded                 # Ground state tracking
│   └── GlobalTransform          # Bevy hierarchy
│
└── Children:
    └── Camera (Entity)
        ├── Camera3d
        ├── Transform            # Offset from player (eye height)
        ├── CameraController     # Mouse look, smoothing
        └── GlobalTransform
```

---

## Implementation Plan

### Phase 1: Actor Component System

**New file: `src/actors/mod.rs`**

```rust
//! Actor system - entities that exist in the game world

use bevy::prelude::*;

/// Marker: Entity is a game actor (has physical presence)
#[derive(Component, Default)]
pub struct Actor;

/// Marker: Entity is the player
#[derive(Component, Default)]
pub struct Player;

/// Marker: Entity is an NPC
#[derive(Component, Default)]
pub struct Npc;

/// Velocity component for physics
#[derive(Component, Default)]
pub struct Velocity {
    pub linear: Vec3,
}

/// Movement capabilities
#[derive(Component)]
pub struct Movement {
    pub walk_speed: f32,      // 4.3 blocks/sec
    pub sprint_speed: f32,    // 5.6 blocks/sec
    pub fly_speed: f32,       // 11.0 blocks/sec
    pub jump_velocity: f32,   // 8.4 blocks/sec
    pub flying: bool,
    pub noclip: bool,
}

/// Ground contact state
#[derive(Component, Default)]
pub struct Grounded {
    pub is_grounded: bool,
    pub ground_normal: Vec3,
    pub time_since_grounded: f32,
}

/// Capsule collider shape
#[derive(Component)]
pub struct CapsuleCollider {
    pub radius: f32,          // 0.3 blocks (player width)
    pub height: f32,          // 1.8 blocks (full height)
    pub eye_offset: f32,      // 1.62 blocks (eye height from feet)
}

impl Default for CapsuleCollider {
    fn default() -> Self {
        Self {
            radius: 0.3,
            height: 1.8,
            eye_offset: 1.62,
        }
    }
}
```

**Player Bundle:**

```rust
/// Bundle for spawning a player entity
#[derive(Bundle)]
pub struct PlayerBundle {
    pub actor: Actor,
    pub player: Player,
    pub transform: Transform,
    pub global_transform: GlobalTransform,
    pub velocity: Velocity,
    pub movement: Movement,
    pub grounded: Grounded,
    pub collider: CapsuleCollider,
    pub visibility: Visibility,
    pub inherited_visibility: InheritedVisibility,
    pub view_visibility: ViewVisibility,
}

impl Default for PlayerBundle {
    fn default() -> Self {
        Self {
            actor: Actor,
            player: Player,
            transform: Transform::from_xyz(32.0, 50.0, 32.0),
            global_transform: GlobalTransform::default(),
            velocity: Velocity::default(),
            movement: Movement {
                walk_speed: 4.3,
                sprint_speed: 5.6,
                fly_speed: 11.0,
                jump_velocity: 8.4,
                flying: false,
                noclip: false,
            },
            grounded: Grounded::default(),
            collider: CapsuleCollider::default(),
            visibility: Visibility::default(),
            inherited_visibility: InheritedVisibility::default(),
            view_visibility: ViewVisibility::default(),
        }
    }
}
```

---

### Phase 2: Capsule Collision System

**New file: `src/physics/collision.rs`**

The capsule collider needs to check collision against actual block data:

```rust
/// Result of a collision sweep
pub struct CollisionResult {
    pub hit: bool,
    pub penetration: Vec3,      // How far we're inside solid
    pub contact_normal: Vec3,   // Surface normal at contact
    pub grounded: bool,         // Standing on something
}

/// Check capsule collision against world blocks
pub fn check_capsule_collision(
    position: Vec3,             // Feet position
    collider: &CapsuleCollider,
    chunk_manager: &ChunkManager,
    chunks: &Query<&Chunk>,
) -> CollisionResult {
    // Capsule represented as:
    // - Bottom sphere at position + (0, radius, 0)
    // - Top sphere at position + (0, height - radius, 0)
    // - Cylinder connecting them

    // Check blocks in AABB around capsule
    let min_block = IVec3::new(
        (position.x - collider.radius).floor() as i32,
        position.y.floor() as i32,
        (position.z - collider.radius).floor() as i32,
    );
    let max_block = IVec3::new(
        (position.x + collider.radius).ceil() as i32,
        (position.y + collider.height).ceil() as i32,
        (position.z + collider.radius).ceil() as i32,
    );

    // Test each block for collision...
}

/// Resolve collision by pushing capsule out of solid blocks
pub fn resolve_collision(
    position: &mut Vec3,
    velocity: &mut Vec3,
    collider: &CapsuleCollider,
    chunk_manager: &ChunkManager,
    chunks: &Query<&Chunk>,
) -> bool {
    // Iterative resolution (max 4 iterations)
    for _ in 0..4 {
        let result = check_capsule_collision(*position, collider, chunk_manager, chunks);
        if !result.hit {
            return result.grounded;
        }

        // Push out of solid
        *position += result.penetration;

        // Cancel velocity into surface
        let vel_into_surface = velocity.dot(result.contact_normal);
        if vel_into_surface < 0.0 {
            *velocity -= result.contact_normal * vel_into_surface;
        }
    }
    false
}
```

**Block-Level Collision Helper:**

```rust
/// Get block at world position, querying the correct chunk
pub fn get_block_at(
    world_pos: IVec3,
    chunk_manager: &ChunkManager,
    chunks: &Query<&Chunk>,
) -> BlockType {
    let chunk_pos = IVec3::new(
        world_pos.x.div_euclid(CHUNK_SIZE as i32),
        world_pos.y.div_euclid(CHUNK_SIZE as i32),
        world_pos.z.div_euclid(CHUNK_SIZE as i32),
    );

    let Some(&entity) = chunk_manager.chunks.get(&chunk_pos) else {
        return BlockType::Air; // Unloaded chunk = air
    };

    let Ok(chunk) = chunks.get(entity) else {
        return BlockType::Air;
    };

    let local = UVec3::new(
        world_pos.x.rem_euclid(CHUNK_SIZE as i32) as u32,
        world_pos.y.rem_euclid(CHUNK_SIZE as i32) as u32,
        world_pos.z.rem_euclid(CHUNK_SIZE as i32) as u32,
    );

    chunk.get_block(local.x as usize, local.y as usize, local.z as usize)
}
```

---

### Phase 3: Decouple Camera from Player

**Changes to `src/engine/mod.rs`:**

The camera becomes a child entity of the player, offset by eye height:

```rust
/// Camera controller - now only handles rotation, not position
#[derive(Component)]
pub struct CameraController {
    pub sensitivity: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub smoothing: f32,
}

/// System: Apply camera rotation from mouse input
fn camera_rotation_system(
    mut mouse_motion: EventReader<MouseMotion>,
    cursor_state: Res<CursorState>,
    mut camera_query: Query<&mut CameraController>,
    time: Res<Time>,
) {
    // Only handles rotation - position comes from parent (player)
}

/// System: Sync camera transform to follow player
fn camera_follow_system(
    player_query: Query<(&Transform, &CapsuleCollider), With<Player>>,
    mut camera_query: Query<(&mut Transform, &CameraController), Without<Player>>,
) {
    // Camera's local transform is just the eye offset
    // GlobalTransform automatically combines with parent
}
```

**Spawn hierarchy in `main.rs`:**

```rust
fn setup_scene(mut commands: Commands) {
    // Spawn player with camera as child
    commands.spawn(PlayerBundle::default())
        .with_children(|parent| {
            parent.spawn((
                Camera3d::default(),
                Transform::from_xyz(0.0, 1.62, 0.0), // Eye offset
                CameraController::default(),
            ));
        });
}
```

---

### Phase 4: Unified Physics Pipeline

**System ordering:**

```
CameraSet (input handling)
    ├── cursor_grab_system
    └── camera_rotation_system

MovementSet (player intent)
    ├── movement_input_system      # WASD -> velocity intent
    └── jump_system               # Space -> jump

PhysicsSet (simulation)
    ├── apply_gravity             # Add gravity to velocity
    ├── apply_movement            # Apply velocity to position
    ├── collision_resolution      # Push out of solids
    └── sync_grounded_state       # Update grounded component

LateUpdate
    └── camera_follow_system      # Camera follows player
```

---

## File Structure After Refactor

```
src/
├── main.rs                 # App entry, scene setup
├── actors/
│   ├── mod.rs              # Actor, Player, Npc markers
│   ├── player.rs           # PlayerBundle, player-specific logic
│   └── movement.rs         # Movement component and systems
├── engine/
│   ├── mod.rs              # CameraPlugin
│   ├── camera.rs           # CameraController, rotation
│   └── input.rs            # CursorState, input handling
├── physics/
│   ├── mod.rs              # PhysicsPlugin, PhysicsSet
│   ├── collision.rs        # CapsuleCollider, collision detection
│   ├── gravity.rs          # Gravity system
│   └── grounded.rs         # Ground detection
├── world/
│   ├── mod.rs              # WorldPlugin, ChunkManager
│   ├── chunk.rs            # Chunk, BlockType
│   ├── meshing.rs          # Mesh generation
│   └── query.rs            # get_block_at, world queries
├── generation/
│   └── mod.rs              # Terrain, caves
└── editor/
    └── mod.rs              # UI panels
```

---

## Migration Strategy

### Step 1: Create actors module (non-breaking)
- Add `src/actors/mod.rs` with components
- Add `PlayerBundle`
- Don't use it yet

### Step 2: Add block query helper (non-breaking)
- Add `get_block_at()` to world module
- Test with unit tests

### Step 3: Implement capsule collision (parallel)
- Add `src/physics/collision.rs`
- Test collision detection standalone
- Keep old heightmap system working

### Step 4: Spawn player entity (breaking change)
- Modify `main.rs` to spawn `PlayerBundle` with camera child
- Update physics systems to query `Player` instead of `Camera3d`
- Update editor to read from `Player` position

### Step 5: Switch to capsule collision
- Replace heightmap collision with capsule
- Remove old terrain height caching
- Test thoroughly

### Step 6: Cleanup
- Remove deprecated code
- Update documentation
- Run full test suite

---

## Constants Reference

| Constant | Value | Description |
|----------|-------|-------------|
| `PLAYER_HEIGHT` | 1.8 | Full player height in blocks |
| `PLAYER_WIDTH` | 0.6 | Player diameter (radius * 2) |
| `EYE_HEIGHT` | 1.62 | Eye position from feet |
| `STEP_HEIGHT` | 0.6 | Max auto-step height |
| `WALK_SPEED` | 4.3 | Walking speed (blocks/sec) |
| `SPRINT_SPEED` | 5.6 | Sprinting speed |
| `FLY_SPEED` | 11.0 | Flying speed |
| `GRAVITY` | 32.0 | Gravity (blocks/sec²) |
| `JUMP_VELOCITY` | 8.4 | Initial jump velocity |
| `TERMINAL_VELOCITY` | 78.0 | Max fall speed |

---

## Benefits of This Architecture

### 1. Separation of Concerns
- Camera only handles view
- Player handles physics body
- Clean system boundaries

### 2. Extensibility
- NPCs can reuse `Actor`, `CapsuleCollider`, `Movement`
- Just add `Npc` marker instead of `Player`
- AI systems query `Npc` marker

### 3. Third-Person Ready
- Camera is already a child entity
- Just change offset for third-person view
- Add camera collision later

### 4. Testable
- Collision detection is pure function
- Can unit test without full ECS
- Systems are focused and small

### 5. Performance
- Query by marker component = fast
- Capsule-AABB test is O(1) per block
- Block queries use HashMap lookup

---

## Testing Strategy

### Unit Tests
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_capsule_vs_block_collision() {
        // Test capsule touching block from each direction
    }

    #[test]
    fn test_capsule_inside_block_resolution() {
        // Test that penetration is correctly calculated
    }

    #[test]
    fn test_grounded_detection() {
        // Test standing on block returns grounded=true
    }

    #[test]
    fn test_head_collision() {
        // Test jumping into ceiling cancels upward velocity
    }
}
```

### Integration Tests
- Walk into wall → stopped
- Jump under ceiling → bonk
- Walk up 0.5 block step → auto-step
- Walk into 1.0 block wall → blocked
- Fall through cave opening → correct collision

---

## Timeline Estimate

| Phase | Scope | Status |
|-------|-------|--------|
| Phase 1 | Actor component system | ✅ COMPLETE |
| Phase 2 | Capsule collision | ✅ COMPLETE |
| Phase 3 | Camera decoupling | ✅ COMPLETE |
| Phase 4 | Physics pipeline cleanup | ⏳ NEXT |
| Testing | Integration, edge cases | Ongoing |

### Phase 1 Deliverables (Complete)
- `src/actors/mod.rs` - Actor, Player, Npc, Mob markers
- `src/actors/mod.rs` - Velocity, Movement, Grounded, CapsuleCollider components
- `src/actors/player.rs` - PlayerBundle, PlayerCameraBundle, spawn helpers
- `src/world/mod.rs` - get_block_at(), is_solid_at(), get_solid_blocks_in_aabb()
- 9 new unit tests for actor components

### Phase 2 Deliverables (Complete)
- `src/physics/collision.rs` - Full capsule-vs-block collision system
  - check_capsule_world_collision() - AABB sweep for capsule
  - check_ground() - Ground detection under capsule
  - check_ceiling() - Head collision detection
  - check_step() - Auto-step detection
  - resolve_collision() - Iterative collision resolution
  - capsule_aabb_penetration() - MTV calculation
- `src/physics/mod.rs` - Refactored physics pipeline
  - Uses block-level collision by default
  - Heightmap fallback for unloaded chunks
  - Centralized constants (EYE_HEIGHT, GRAVITY, etc.)
  - Ceiling collision prevents jumping into blocks
- 6 new unit tests for collision system
- Total: 47 tests passing

### Phase 3 Deliverables (Complete)
- **Player Entity Architecture**
  - `main.rs` - Now spawns `PlayerBundle` with camera as child
  - Player entity has: Actor, Player, Transform, Velocity, CapsuleCollider, Grounded, Movement
  - Camera is CHILD entity with just Camera3d + CameraController + eye offset Transform

- **Refactored `src/engine/mod.rs`**
  - Split into `CameraInputSet` (before physics) and `CameraSyncSet` (after physics)
  - `camera_rotation_system` - Mouse look only, rotation state
  - `player_movement_input_system` - WASD sets player Velocity component
  - `camera_sync_system` - Smooth rotation, position from parent hierarchy
  - CameraController simplified: only rotation, no position/speed fields

- **Refactored `src/physics/mod.rs`**
  - All systems now query `Player` entity instead of Camera3d
  - `apply_movement` - New system applies Velocity to Transform
  - Physics operates on Player's Transform directly
  - Legacy `PlayerPhysics` resource synced for compatibility

- **Benefits Achieved**
  - Clean separation: Player has physics, Camera has view
  - Position comes from Bevy's parent-child hierarchy
  - Third-person camera now possible (just change child offset)
  - No more jitter from camera fighting physics
  - Total: 47 tests passing, 0 warnings

---

## Enhancements Identified

### Performance Optimizations
1. **Chunk query caching** - Cache recent block lookups in collision (same blocks queried multiple times per frame)
2. **Spatial hashing** - For entities, use spatial hash instead of iterating all
3. **Collision early-out** - Skip collision check entirely when in air and moving horizontally only

### Gameplay Improvements
1. **Slope sliding** - When hitting angled surfaces, slide along them instead of stopping
2. **Wall sliding** - Smooth movement along walls when walking into them at an angle
3. **Crouch/sneak** - Reduce collision height, prevent falling off edges
4. **Swimming** - Different physics when in water blocks
5. **Ladder climbing** - Special movement when touching ladder blocks

### Code Quality
1. **Consolidate constants** - Move all physics constants to a single `constants.rs`
2. **Configuration file** - Load physics parameters from TOML/JSON for easy tuning
3. **Debug visualization** - Draw collision capsule and contact points in debug mode

### Future Entity Types
1. **NPCs** - Use same actor components with AI controller instead of player input
2. **Projectiles** - Simplified collision (ray cast or small sphere)
3. **Vehicles** - Larger collision shapes, different movement physics

---

## Questions to Resolve

1. **Floating point precision**: Should player position be stored as feet center or body center?
   - **Recommendation**: Feet position (simpler ground detection)

2. **Collision iteration count**: How many iterations for resolution?
   - **Recommendation**: 4 iterations max (diminishing returns)

3. **Step smoothing**: Should auto-step be instant or slightly smoothed?
   - **Recommendation**: Instant Y snap (Minecraft feel)

4. **Block query caching**: Cache recent block lookups?
   - **Recommendation**: Start without, profile later

---

## Next Immediate Steps (Phase 3)

1. ✅ Create `src/actors/mod.rs` with basic components
2. ✅ Add `get_block_at()` helper to world module
3. ✅ Write unit tests for block queries
4. ✅ Implement `check_capsule_collision()`
5. ✅ Test collision standalone before integration

### Phase 3 Tasks (Camera Decoupling)

1. **Modify main.rs setup_scene()**
   - Replace direct Camera3d spawn with `spawn_player()`
   - Camera becomes CHILD of player entity
   - Remove old CameraController spawn

2. **Update physics systems**
   - Query `Player` component instead of `Camera3d`
   - Update player's Transform, not camera's
   - Camera follows automatically via parent hierarchy

3. **Update engine systems**
   - CameraController only handles rotation
   - Movement input updates player entity, not camera
   - Remove position-related code from camera controller

4. **Update editor systems**
   - Read player position from Player entity
   - Display correct feet position vs eye position

5. **Test thoroughly**
   - Walk, jump, fly modes still work
   - Collision detects blocks correctly
   - No visual glitches during movement
