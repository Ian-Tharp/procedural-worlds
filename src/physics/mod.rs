//! Physics and collision systems
//!
//! This module handles:
//! - Gravity and vertical movement
//! - Capsule collision with world blocks
//! - Ground detection and jumping
//! - Flying/noclip modes
//!
//! # System Order (Phase 3)
//!
//! Physics runs AFTER movement input, operates on Player entity:
//! 1. `buffer_jump_input` - Capture jump presses for buffering
//! 2. `handle_jump` - Execute jump if grounded and buffered
//! 3. `apply_gravity` - Apply gravity to Player's Velocity
//! 4. `apply_movement` - Apply Velocity to Player's Transform
//! 5. `apply_collision` - Resolve collisions with world
//! 6. `sync_grounded_state` - Final ground state verification

use bevy::prelude::*;

use crate::actors::{CapsuleCollider, Grounded, Movement, Player, Velocity};
use crate::generation::TerrainConfig;
use crate::world::{Chunk, ChunkManager, CHUNK_SIZE};

pub mod collision;
pub mod swimming;

pub use collision::{
    check_ceiling, check_ground, resolve_collision, CollisionParams,
};

// ============================================================================
// CONSTANTS
// ============================================================================

/// Eye height above feet (Minecraft standard is 1.62)
/// Note: Now using CapsuleCollider::EYE_HEIGHT in actors module
#[allow(dead_code)]
pub const EYE_HEIGHT: f32 = 1.62;

/// Player total height (for Phase 3)
#[allow(dead_code)]
pub const PLAYER_HEIGHT: f32 = 1.8;

/// Player collision radius (half width, for Phase 3)
#[allow(dead_code)]
pub const PLAYER_RADIUS: f32 = 0.3;

/// Gravity acceleration (blocks per second squared)
pub const GRAVITY: f32 = 32.0;

/// Terminal velocity (blocks per second)
pub const TERMINAL_VELOCITY: f32 = 78.0;

/// Jump velocity (blocks per second) - gives ~1.25 block jump height
pub const JUMP_VELOCITY: f32 = 8.4;

/// Maximum step height for auto-stepping
pub const STEP_HEIGHT: f32 = 0.6;

/// Jump input buffer duration (seconds)
const JUMP_BUFFER_DURATION: f32 = 0.1;

/// Coyote time - can still jump briefly after leaving ground
const COYOTE_TIME: f32 = 0.1;

// ============================================================================
// PLUGIN
// ============================================================================

/// System set for physics ordering
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PhysicsSet;

/// Plugin for physics and collision systems
pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PhysicsConfig>()
            .init_resource::<PlayerPhysics>()
            .init_resource::<CollisionParams>()
            .init_resource::<JumpBuffer>()
            .add_systems(
                Update,
                (
                    sync_movement_to_physics,
                    buffer_jump_input,
                    handle_jump,
                    apply_gravity,
                    apply_movement,
                    apply_collision,
                    sync_grounded_state,
                )
                    .chain()
                    .in_set(PhysicsSet),
            );
    }
}

// ============================================================================
// RESOURCES
// ============================================================================

/// Physics configuration
#[derive(Resource)]
pub struct PhysicsConfig {
    pub gravity: f32,
    pub terminal_velocity: f32,
    pub jump_velocity: f32,
    /// Step height (for Phase 3 auto-stepping)
    #[allow(dead_code)]
    pub step_height: f32,
    /// Use block-level collision (true) or heightmap fallback (false)
    pub use_block_collision: bool,
}

impl Default for PhysicsConfig {
    fn default() -> Self {
        Self {
            gravity: GRAVITY,
            terminal_velocity: TERMINAL_VELOCITY,
            jump_velocity: JUMP_VELOCITY,
            step_height: STEP_HEIGHT,
            use_block_collision: true,
        }
    }
}

/// Player physics state (temporary - will be replaced by actor components)
#[derive(Resource)]
pub struct PlayerPhysics {
    /// Current vertical velocity
    pub velocity_y: f32,
    /// Whether player is on ground
    pub grounded: bool,
    /// Time since last grounded (for coyote time)
    pub time_since_grounded: f32,
    /// Whether flying mode is enabled (creative mode)
    pub flying: bool,
    /// Whether noclip is enabled (ignores collision)
    pub noclip: bool,
}

impl Default for PlayerPhysics {
    fn default() -> Self {
        Self {
            velocity_y: 0.0,
            grounded: false,
            time_since_grounded: 0.0,
            flying: false,
            noclip: false,
        }
    }
}

/// Buffer for jump input (allows pressing jump slightly before landing)
#[derive(Resource, Default)]
struct JumpBuffer {
    buffer_time: f32,
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// Sync Movement component state to PlayerPhysics resource
///
/// This keeps the legacy PlayerPhysics resource in sync with the
/// authoritative Movement component on the Player entity.
fn sync_movement_to_physics(
    mut physics: ResMut<PlayerPhysics>,
    player_query: Query<&Movement, With<Player>>,
) {
    if let Ok(movement) = player_query.get_single() {
        physics.flying = movement.flying;
        physics.noclip = movement.noclip;
    }
}

/// Buffer jump input for responsive feel
fn buffer_jump_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut buffer: ResMut<JumpBuffer>,
    time: Res<Time>,
) {
    // Decrement buffer time
    buffer.buffer_time = (buffer.buffer_time - time.delta_secs()).max(0.0);

    // If jump pressed, start/refresh buffer
    if keyboard.just_pressed(KeyCode::Space) {
        buffer.buffer_time = JUMP_BUFFER_DURATION;
    }
}

/// Handle jumping with input buffering and coyote time
fn handle_jump(
    mut physics: ResMut<PlayerPhysics>,
    mut buffer: ResMut<JumpBuffer>,
    config: Res<PhysicsConfig>,
    chunk_manager: Option<Res<ChunkManager>>,
    chunks: Query<&Chunk>,
    mut player_query: Query<(&Transform, &CapsuleCollider, &Movement, &mut Velocity, &mut Grounded), With<Player>>,
) {
    for (transform, collider, movement, mut velocity, mut grounded) in &mut player_query {
        // Only jump if not flying/noclip
        if movement.flying || movement.noclip {
            buffer.buffer_time = 0.0;
            continue;
        }

        // Check if can jump (grounded or within coyote time)
        let can_jump = grounded.is_grounded || grounded.time_since_grounded < COYOTE_TIME;

        // Jump if can jump AND has buffered input
        if can_jump && buffer.buffer_time > 0.0 {
            let feet_pos = transform.translation;

            let ceiling_blocked = if let Some(ref cm) = chunk_manager {
                check_ceiling(feet_pos, collider, cm, &chunks)
            } else {
                false
            };

            if !ceiling_blocked {
                velocity.linear.y = config.jump_velocity;
                grounded.is_grounded = false;
                grounded.time_since_grounded = COYOTE_TIME + 0.1;
                buffer.buffer_time = 0.0;

                // Sync to legacy resource
                physics.velocity_y = config.jump_velocity;
                physics.grounded = false;
                physics.time_since_grounded = COYOTE_TIME + 0.1;
            }
        }
    }
}

/// Apply gravity acceleration to Player's velocity
fn apply_gravity(
    time: Res<Time>,
    mut physics: ResMut<PlayerPhysics>,
    config: Res<PhysicsConfig>,
    mut player_query: Query<(&Movement, &mut Velocity, &mut Grounded), With<Player>>,
) {
    let dt = time.delta_secs();

    for (movement, mut velocity, mut grounded) in &mut player_query {
        // Update coyote time
        if grounded.is_grounded {
            grounded.time_since_grounded = 0.0;
        } else {
            grounded.time_since_grounded += dt;
        }

        // Skip gravity if flying or noclip
        if movement.flying || movement.noclip {
            // In flying mode, Y velocity is controlled by input
            continue;
        }

        // Apply gravity acceleration when in air
        if !grounded.is_grounded {
            velocity.linear.y -= config.gravity * dt;
            velocity.linear.y = velocity.linear.y.max(-config.terminal_velocity);
        }

        // Sync to legacy resource
        physics.velocity_y = velocity.linear.y;
        physics.grounded = grounded.is_grounded;
        physics.time_since_grounded = grounded.time_since_grounded;
    }
}

/// Apply velocity to Player's transform
fn apply_movement(
    time: Res<Time>,
    mut player_query: Query<(&mut Transform, &Velocity, &Movement), With<Player>>,
) {
    let dt = time.delta_secs();

    for (mut transform, velocity, movement) in &mut player_query {
        if movement.noclip {
            // Noclip: apply full velocity directly, no collision
            transform.translation += velocity.linear * dt;
        } else if movement.flying {
            // Flying: apply velocity directly (collision will resolve)
            transform.translation += velocity.linear * dt;
        } else {
            // Walking: apply velocity (collision will resolve)
            transform.translation += velocity.linear * dt;
        }
    }
}

/// Apply collision resolution using block-level collision
fn apply_collision(
    mut physics: ResMut<PlayerPhysics>,
    config: Res<PhysicsConfig>,
    collision_params: Res<CollisionParams>,
    chunk_manager: Option<Res<ChunkManager>>,
    chunks: Query<&Chunk>,
    terrain_config: Option<Res<TerrainConfig>>,
    mut player_query: Query<(&mut Transform, &CapsuleCollider, &Movement, &mut Velocity, &mut Grounded), With<Player>>,
) {
    for (mut transform, collider, movement, mut velocity, mut grounded) in &mut player_query {
        // Skip if noclip (no collision at all)
        if movement.noclip {
            continue;
        }

        // Skip collision for flying mode (allows passing through blocks)
        // Comment this out if you want flying to still collide
        if movement.flying {
            continue;
        }

        let mut feet_pos = transform.translation;

        // Try block-level collision first
        if config.use_block_collision {
            if let Some(ref cm) = chunk_manager {
                let mut vel = velocity.linear;

                let (new_feet_pos, result) = resolve_collision(
                    feet_pos,
                    &mut vel,
                    collider,
                    cm,
                    &chunks,
                    collision_params.as_ref(),
                );

                feet_pos = new_feet_pos;
                velocity.linear = vel;

                if result.grounded {
                    grounded.is_grounded = true;
                    if velocity.linear.y < 0.0 {
                        velocity.linear.y = 0.0;
                    }
                }

                if result.head_hit && velocity.linear.y > 0.0 {
                    velocity.linear.y = 0.0;
                }

                transform.translation = feet_pos;

                // Sync to legacy resource
                physics.velocity_y = velocity.linear.y;
                physics.grounded = grounded.is_grounded;
                continue;
            }
        }

        // Fallback: heightmap-based collision for unloaded areas
        if let Some(ref tc) = terrain_config {
            let terrain_height = estimate_terrain_height(feet_pos.x, feet_pos.z, tc);
            let ground_y = terrain_height + 1.0; // Stand ON TOP of block

            if feet_pos.y < ground_y {
                transform.translation.y = ground_y;
                grounded.is_grounded = true;
                if velocity.linear.y < 0.0 {
                    velocity.linear.y = 0.0;
                }

                // Sync to legacy resource
                physics.velocity_y = 0.0;
                physics.grounded = true;
            }
        }
    }
}

/// Synchronize grounded state - prevents rapid toggling
fn sync_grounded_state(
    mut physics: ResMut<PlayerPhysics>,
    collision_params: Res<CollisionParams>,
    chunk_manager: Option<Res<ChunkManager>>,
    chunks: Query<&Chunk>,
    terrain_config: Option<Res<TerrainConfig>>,
    mut player_query: Query<(&Transform, &CapsuleCollider, &Movement, &mut Velocity, &mut Grounded), With<Player>>,
) {
    for (transform, collider, movement, mut velocity, mut grounded) in &mut player_query {
        if movement.flying || movement.noclip {
            continue;
        }

        let feet_pos = transform.translation;

        // Try block-level ground check
        if let Some(ref cm) = chunk_manager {
            let (is_grounded, ground_y) = check_ground(
                feet_pos,
                collider,
                cm,
                &chunks,
                collision_params.as_ref(),
            );

            if is_grounded {
                let distance = feet_pos.y - ground_y;
                if distance.abs() < collision_params.ground_tolerance {
                    if !grounded.is_grounded && velocity.linear.y <= 0.0 {
                        grounded.is_grounded = true;
                        velocity.linear.y = 0.0;

                        // Sync to legacy resource
                        physics.grounded = true;
                        physics.velocity_y = 0.0;
                    }
                    continue;
                }
            }
        }

        // Fallback: heightmap check
        if let Some(ref tc) = terrain_config {
            let terrain_height = estimate_terrain_height(feet_pos.x, feet_pos.z, tc);
            let ground_y = terrain_height + 1.0;
            let distance = feet_pos.y - ground_y;

            if distance.abs() < 0.1 {
                if !grounded.is_grounded && velocity.linear.y <= 0.0 {
                    grounded.is_grounded = true;
                    velocity.linear.y = 0.0;

                    // Sync to legacy resource
                    physics.grounded = true;
                    physics.velocity_y = 0.0;
                }
            } else if distance > 0.1 {
                grounded.is_grounded = false;
                physics.grounded = false;
            }
        }
    }
}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/// Estimate terrain height using noise (fallback when chunks not loaded)
///
/// Returns the Y coordinate of the TOP surface block (integer, matching actual terrain)
pub fn estimate_terrain_height(world_x: f32, world_z: f32, config: &TerrainConfig) -> f32 {
    use noise::{NoiseFn, Simplex};

    let noise = Simplex::new(config.seed);

    let mut height = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = config.frequency;

    for _ in 0..config.octaves {
        height += noise.get([world_x as f64 * frequency, world_z as f64 * frequency]) * amplitude;
        amplitude *= 0.5;
        frequency *= 2.0;
    }

    // Cast to i32 first to match terrain generation (blocky terrain)
    let block_height = (config.base_height + height * config.height_scale) as i32;
    block_height as f32
}

/// Convert world position to chunk coordinates (used in tests)
#[allow(dead_code)]
pub fn world_to_chunk_pos(world_pos: Vec3) -> IVec3 {
    IVec3::new(
        (world_pos.x / CHUNK_SIZE as f32).floor() as i32,
        (world_pos.y / CHUNK_SIZE as f32).floor() as i32,
        (world_pos.z / CHUNK_SIZE as f32).floor() as i32,
    )
}

/// Convert world position to local block coordinates within a chunk (used in tests)
#[allow(dead_code)]
pub fn world_to_local_block(world_pos: Vec3) -> (IVec3, UVec3) {
    let chunk_pos = world_to_chunk_pos(world_pos);
    let local = UVec3::new(
        ((world_pos.x.floor() as i32).rem_euclid(CHUNK_SIZE as i32)) as u32,
        ((world_pos.y.floor() as i32).rem_euclid(CHUNK_SIZE as i32)) as u32,
        ((world_pos.z.floor() as i32).rem_euclid(CHUNK_SIZE as i32)) as u32,
    );
    (chunk_pos, local)
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_world_to_chunk_pos() {
        assert_eq!(world_to_chunk_pos(Vec3::new(0.0, 0.0, 0.0)), IVec3::ZERO);
        assert_eq!(world_to_chunk_pos(Vec3::new(15.9, 15.9, 15.9)), IVec3::ZERO);
        assert_eq!(world_to_chunk_pos(Vec3::new(16.0, 16.0, 16.0)), IVec3::ONE);
        assert_eq!(
            world_to_chunk_pos(Vec3::new(32.5, 48.0, 64.0)),
            IVec3::new(2, 3, 4)
        );

        // Negative coordinates
        assert_eq!(
            world_to_chunk_pos(Vec3::new(-1.0, 0.0, 0.0)),
            IVec3::new(-1, 0, 0)
        );
        assert_eq!(
            world_to_chunk_pos(Vec3::new(-16.0, -16.0, -16.0)),
            IVec3::new(-1, -1, -1)
        );
    }

    #[test]
    fn test_world_to_local_block() {
        let (chunk, local) = world_to_local_block(Vec3::new(5.5, 10.2, 3.8));
        assert_eq!(chunk, IVec3::ZERO);
        assert_eq!(local, UVec3::new(5, 10, 3));

        let (chunk, local) = world_to_local_block(Vec3::new(20.0, 5.0, 35.0));
        assert_eq!(chunk, IVec3::new(1, 0, 2));
        assert_eq!(local, UVec3::new(4, 5, 3));

        // Negative world coordinates
        let (chunk, local) = world_to_local_block(Vec3::new(-5.0, 10.0, -20.0));
        assert_eq!(chunk, IVec3::new(-1, 0, -2));
        assert_eq!(local, UVec3::new(11, 10, 12));
    }

    #[test]
    fn test_terrain_height_estimation() {
        let config = TerrainConfig::default();

        let height = estimate_terrain_height(0.0, 0.0, &config);
        assert!(
            height > 16.0 && height < 48.0,
            "Height {} out of expected range",
            height
        );

        // Same position should give same height (deterministic)
        let height2 = estimate_terrain_height(0.0, 0.0, &config);
        assert_eq!(height, height2);
    }

    #[test]
    fn test_physics_config_default() {
        let config = PhysicsConfig::default();
        assert_eq!(config.gravity, GRAVITY);
        assert_eq!(config.terminal_velocity, TERMINAL_VELOCITY);
        assert_eq!(config.jump_velocity, JUMP_VELOCITY);
        assert!(config.use_block_collision);
    }

    #[test]
    fn test_player_physics_default() {
        let physics = PlayerPhysics::default();
        assert_eq!(physics.velocity_y, 0.0);
        assert!(!physics.grounded);
        assert!(!physics.flying);
        assert!(!physics.noclip);
    }

    #[test]
    fn test_constants() {
        assert_eq!(EYE_HEIGHT, 1.62);
        assert_eq!(PLAYER_HEIGHT, 1.8);
        assert_eq!(PLAYER_RADIUS, 0.3);
        assert_eq!(GRAVITY, 32.0);
        assert_eq!(TERMINAL_VELOCITY, 78.0);
        assert_eq!(JUMP_VELOCITY, 8.4);
        assert_eq!(STEP_HEIGHT, 0.6);
    }
}
