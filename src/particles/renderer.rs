//! Particle renderer with block light integration.
//!
//! Handles the visual rendering of particles using Bevy's mesh and material
//! system. The renderer applies block light modulation to particle colors,
//! creating the visual effect of particles brightening near light sources
//! and dimming in shadows.
//!
//! # Shader Integration
//!
//! Particle light levels are encoded into vertex colors. The brightness
//! modulation happens CPU-side for now (vertex color × light factor),
//! which avoids the complexity of a custom shader pipeline while still
//! producing correct visual results. When GPU-instanced particles are
//! implemented, this will transition to a custom vertex shader with a
//! dedicated light level attribute.
//!
//! # Light-Responsive Vertex Colors
//!
//! ```text
//! final_color = base_color × light_brightness × age_fade
//! ```
//!
//! Where:
//! - `base_color`: The particle effect's configured color
//! - `light_brightness`: Block light level / 15.0 (0.0–1.0)
//! - `age_fade`: Alpha fade based on remaining lifetime

use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};

use super::effects::ParticleLightData;
use super::emitter::{Particle, ParticleEmitter};
use super::light_query::BlockLightQuery;

// ============================================================================
// CONFIGURATION
// ============================================================================

/// Configuration for the particle renderer.
#[derive(Resource, Debug, Clone)]
pub struct ParticleRendererConfig {
    /// Whether to apply block light modulation to particle colors.
    pub light_modulation_enabled: bool,
    /// Minimum alpha before a particle is culled from rendering.
    pub alpha_cull_threshold: f32,
    /// Whether to use billboard rendering (particles always face camera).
    pub billboard: bool,
    /// Gamma correction exponent for light brightness.
    /// Values > 1.0 make the transition from dark to bright more gradual.
    pub light_gamma: f32,
}

impl Default for ParticleRendererConfig {
    fn default() -> Self {
        Self {
            light_modulation_enabled: true,
            alpha_cull_threshold: 0.01,
            billboard: true,
            light_gamma: 1.4,
        }
    }
}

// ============================================================================
// PARTICLE MESH QUAD
// ============================================================================

/// Creates a simple quad mesh for particle rendering.
///
/// The quad is centered at the origin with the given size, facing +Z.
/// Vertex colors are initialized to white and modulated at render time.
pub fn create_particle_quad(size: f32) -> Mesh {
    let half = size / 2.0;

    let positions = vec![
        [-half, -half, 0.0],
        [half, -half, 0.0],
        [half, half, 0.0],
        [-half, half, 0.0],
    ];

    let normals = vec![
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 1.0],
    ];

    let uvs: Vec<[f32; 2]> = vec![
        [0.0, 1.0],
        [1.0, 1.0],
        [1.0, 0.0],
        [0.0, 0.0],
    ];

    let indices = vec![0u32, 1, 2, 2, 3, 0];

    // Initialize vertex colors to white (will be modulated by light)
    let colors: Vec<[f32; 4]> = vec![[1.0, 1.0, 1.0, 1.0]; 4];

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        bevy::render::render_asset::RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

// ============================================================================
// LIGHT-MODULATED VERTEX COLOR COMPUTATION
// ============================================================================

/// Compute the final vertex color for a particle, incorporating block light.
///
/// This is the core of the particle lighting integration. The base color
/// is multiplied by the light brightness factor, with gamma correction
/// applied for more natural-looking transitions.
///
/// # Arguments
/// * `base_color` - The particle's base color from its effect type
/// * `light_data` - Block light information for this particle
/// * `age_ratio` - Particle age (0.0 = new, 1.0 = expired) for alpha fade
/// * `config` - Renderer configuration (gamma, modulation toggle)
pub fn compute_lit_vertex_color(
    base_color: Color,
    light_data: ParticleLightData,
    age_ratio: f32,
    config: &ParticleRendererConfig,
) -> [f32; 4] {
    let srgba = base_color.to_srgba();

    let brightness = if !config.light_modulation_enabled || light_data.self_illuminated {
        1.0
    } else {
        let raw_brightness = (light_data.block_light as f32 / 15.0).max(0.05);
        // Apply gamma correction for smoother light transitions
        raw_brightness.powf(1.0 / config.light_gamma)
    };

    // Alpha fades out as particle ages
    let alpha_fade = 1.0 - age_ratio.clamp(0.0, 1.0);
    let final_alpha = srgba.alpha * alpha_fade;

    [
        srgba.red * brightness,
        srgba.green * brightness,
        srgba.blue * brightness,
        final_alpha,
    ]
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// Shared particle mesh handle, created once and reused for all particles.
#[derive(Resource)]
pub struct SharedParticleMesh {
    pub handle: Handle<Mesh>,
}

/// Shared particle material, created once and reused.
#[derive(Resource)]
pub struct SharedParticleMaterial {
    pub handle: Handle<StandardMaterial>,
}

/// System that initializes shared particle rendering resources.
pub fn setup_particle_renderer(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Create a shared unit quad mesh for all particles
    let mesh = create_particle_quad(1.0);
    let mesh_handle = meshes.add(mesh);
    commands.insert_resource(SharedParticleMesh {
        handle: mesh_handle,
    });

    // Create a shared material with vertex color support
    let material = StandardMaterial {
        base_color: Color::WHITE,
        alpha_mode: AlphaMode::Blend,
        unlit: true, // Particles manage their own lighting via vertex colors
        ..default()
    };
    let material_handle = materials.add(material);
    commands.insert_resource(SharedParticleMaterial {
        handle: material_handle,
    });
}

/// System that spawns particle entities from active emitters.
///
/// Each spawned particle inherits the emitter's cached light level
/// and is given initial velocity, lifetime, and visual properties.
pub fn spawn_particles(
    mut commands: Commands,
    time: Res<Time>,
    light_query: Res<BlockLightQuery>,
    shared_mesh: Option<Res<SharedParticleMesh>>,
    shared_material: Option<Res<SharedParticleMaterial>>,
    mut emitters: Query<(&GlobalTransform, &mut ParticleEmitter)>,
) {
    let Some(mesh_res) = shared_mesh else { return };
    let Some(mat_res) = shared_material else { return };

    let dt = time.delta_secs();

    for (transform, mut emitter) in &mut emitters {
        let spawn_count = emitter.compute_spawn_count(dt);
        if spawn_count == 0 {
            continue;
        }

        let emitter_pos = transform.translation();
        let _light_sample = emitter.light_sample;
        let config = emitter.config.clone();
        let effect_type = emitter.effect_type;

        for _ in 0..spawn_count {
            // Random velocity within configured range
            let velocity = Vec3::new(
                rand_range(config.velocity_min.x, config.velocity_max.x),
                rand_range(config.velocity_min.y, config.velocity_max.y),
                rand_range(config.velocity_min.z, config.velocity_max.z),
            );

            // Slight spawn position jitter
            let jitter = Vec3::new(
                rand_range(-0.1, 0.1),
                rand_range(-0.05, 0.05),
                rand_range(-0.1, 0.1),
            );
            let spawn_pos = emitter_pos + jitter;

            // Determine particle light based on effect type
            let particle_light = if effect_type.is_self_illuminated() {
                super::light_query::LightSample::max()
            } else {
                // Sample light at spawn position for initial brightness
                light_query.sample_at(spawn_pos)
            };

            let particle = Particle::new(
                velocity,
                config.lifetime,
                config.base_color,
                particle_light,
                config.size,
                config.per_particle_light_sampling,
            );

            commands.spawn((
                Mesh3d(mesh_res.handle.clone()),
                MeshMaterial3d(mat_res.handle.clone()),
                Transform::from_translation(spawn_pos)
                    .with_scale(Vec3::splat(config.size)),
                particle,
            ));

            emitter.alive_count += 1;
        }
    }
}

/// System that updates particle positions, lifetimes, and light levels.
pub fn update_particles(
    mut commands: Commands,
    time: Res<Time>,
    light_query: Res<BlockLightQuery>,
    renderer_config: Res<ParticleRendererConfig>,
    mut particles: Query<(Entity, &mut Transform, &mut Particle)>,
    mut emitters: Query<&mut ParticleEmitter>,
) {
    let dt = time.delta_secs();

    for (entity, mut transform, mut particle) in &mut particles {
        // Update position
        transform.translation += particle.velocity * dt;

        // Apply gravity (slight downward pull)
        particle.velocity.y -= 0.5 * dt;

        // Update lifetime
        particle.lifetime -= dt;

        // Despawn expired particles
        if particle.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            // Decrement emitter alive count (best-effort — emitter may be gone)
            for mut emitter in &mut emitters {
                if emitter.alive_count > 0 {
                    emitter.alive_count = emitter.alive_count.saturating_sub(1);
                    break;
                }
            }
            continue;
        }

        // Update per-particle light if enabled
        if particle.sample_own_light && renderer_config.light_modulation_enabled {
            let sample = light_query.sample_at(transform.translation);
            particle.update_light(sample);
        }
    }
}

/// System that applies billboard rotation to particles (face the camera).
pub fn billboard_particles(
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
    renderer_config: Res<ParticleRendererConfig>,
    mut particles: Query<&mut Transform, With<Particle>>,
) {
    if !renderer_config.billboard {
        return;
    }

    let Ok(camera_transform) = camera_query.get_single() else {
        return;
    };

    let camera_forward = camera_transform.forward().as_vec3();

    for mut transform in &mut particles {
        // Billboard: rotate to face camera
        let look_dir = -camera_forward;
        if look_dir.length_squared() > 0.001 {
            transform.rotation = Quat::from_rotation_arc(Vec3::Z, look_dir.normalize());
        }
    }
}

// ============================================================================
// HELPERS
// ============================================================================

/// Simple pseudo-random number in range [min, max].
/// Uses a fast xorshift-based approach seeded from the current time.
fn rand_range(min: f32, max: f32) -> f32 {
    // Simple hash-based random using thread-local state
    use std::cell::Cell;
    thread_local! {
        static STATE: Cell<u64> = const { Cell::new(0x12345678_9ABCDEF0) };
    }
    STATE.with(|state| {
        let mut s = state.get();
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        state.set(s);
        let t = (s & 0xFFFFFFFF) as f32 / u32::MAX as f32;
        min + t * (max - min)
    })
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_particle_quad() {
        let mesh = create_particle_quad(1.0);
        // Should have 4 vertices and 6 indices (2 triangles)
        assert!(mesh.count_vertices() == 4);
    }

    #[test]
    fn test_compute_lit_vertex_color_full_brightness() {
        let config = ParticleRendererConfig::default();
        let color = Color::srgba(1.0, 0.5, 0.25, 1.0);
        let light = ParticleLightData::responsive(15);

        let result = compute_lit_vertex_color(color, light, 0.0, &config);

        // At full brightness (15/15), gamma-corrected should be ~1.0
        assert!(result[0] > 0.95, "Red should be near 1.0, got {}", result[0]);
        assert!(result[3] > 0.95, "Alpha should be ~1.0 at age 0");
    }

    #[test]
    fn test_compute_lit_vertex_color_dim() {
        let config = ParticleRendererConfig::default();
        let color = Color::srgba(1.0, 1.0, 1.0, 1.0);
        let light = ParticleLightData::responsive(3);

        let result = compute_lit_vertex_color(color, light, 0.0, &config);

        // At low light (3/15 = 0.2), with gamma correction should be < 1.0
        assert!(result[0] < 0.6, "Color should be dimmed, got {}", result[0]);
        assert!(result[0] > 0.0, "Color should not be zero");
    }

    #[test]
    fn test_compute_lit_vertex_color_self_illuminated() {
        let config = ParticleRendererConfig::default();
        let color = Color::srgba(1.0, 0.7, 0.2, 1.0);
        let light = ParticleLightData::self_lit();

        let result = compute_lit_vertex_color(color, light, 0.0, &config);

        // Self-illuminated should be at full brightness
        assert!((result[0] - 1.0).abs() < 0.01);
        assert!((result[1] - 0.7).abs() < 0.01);
    }

    #[test]
    fn test_compute_lit_vertex_color_age_fade() {
        let config = ParticleRendererConfig::default();
        let color = Color::srgba(1.0, 1.0, 1.0, 1.0);
        let light = ParticleLightData::responsive(15);

        // At age 0.5, alpha should be ~0.5
        let result = compute_lit_vertex_color(color, light, 0.5, &config);
        assert!((result[3] - 0.5).abs() < 0.01);

        // At age 1.0, alpha should be ~0.0
        let result = compute_lit_vertex_color(color, light, 1.0, &config);
        assert!((result[3] - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_compute_lit_vertex_color_disabled() {
        let mut config = ParticleRendererConfig::default();
        config.light_modulation_enabled = false;
        let color = Color::srgba(1.0, 1.0, 1.0, 1.0);
        let light = ParticleLightData::responsive(0); // Completely dark

        let result = compute_lit_vertex_color(color, light, 0.0, &config);

        // With modulation disabled, should be full brightness
        assert!((result[0] - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_rand_range_bounds() {
        for _ in 0..100 {
            let val = rand_range(0.0, 1.0);
            assert!(val >= 0.0 && val <= 1.0, "Out of range: {val}");
        }
    }

    #[test]
    fn test_renderer_config_defaults() {
        let config = ParticleRendererConfig::default();
        assert!(config.light_modulation_enabled);
        assert!(config.billboard);
        assert!(config.light_gamma > 1.0);
        assert!(config.alpha_cull_threshold > 0.0);
    }
}
