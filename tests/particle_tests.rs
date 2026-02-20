//! Integration tests for the particle effects system with block light integration.
//!
//! Verifies that particles correctly respond to block light levels,
//! including brightening near light sources and dimming in shadows.

use procedural_worlds::particles::effects::{ParticleEffectType, ParticleLightData};
use procedural_worlds::particles::emitter::{Particle, ParticleEmitter};
use procedural_worlds::particles::light_query::{BlockLightQuery, BlockLightQueryConfig};
use procedural_worlds::particles::renderer::{compute_lit_vertex_color, ParticleRendererConfig};

use bevy::prelude::*;

// ============================================================================
// Test: Particles brighten near light sources
// ============================================================================

#[test]
fn test_particles_brighten_near_light_sources() {
    let light_query = BlockLightQuery {
        config: BlockLightQueryConfig {
            minimum_light: 0,
            use_sky_estimation: true,
            sky_light_height: 64.0,
            underground_height: 0.0,
        },
    };

    // Sample at high altitude (near sky) — should be bright
    let bright_pos = Vec3::new(10.0, 60.0, 10.0);
    let bright_sample = light_query.sample_at(bright_pos);

    // Sample underground — should be dim
    let dark_pos = Vec3::new(10.0, 5.0, 10.0);
    let dark_sample = light_query.sample_at(dark_pos);

    assert!(
        bright_sample.brightness > dark_sample.brightness,
        "Particles at height 60 (brightness={}) should be brighter than at height 5 (brightness={})",
        bright_sample.brightness,
        dark_sample.brightness,
    );

    // Create particles at both positions
    let bright_particle = Particle::new(
        Vec3::Y,
        3.0,
        Color::WHITE,
        bright_sample,
        0.1,
        false,
    );
    let dark_particle = Particle::new(
        Vec3::Y,
        3.0,
        Color::WHITE,
        dark_sample,
        0.1,
        false,
    );

    // Verify the lit colors reflect the light difference
    let bright_lit = bright_particle.lit_color().to_srgba();
    let dark_lit = dark_particle.lit_color().to_srgba();

    assert!(
        bright_lit.red > dark_lit.red,
        "Bright particle (r={}) should have higher red than dark particle (r={})",
        bright_lit.red,
        dark_lit.red,
    );
}

// ============================================================================
// Test: Particles dim in shadows
// ============================================================================

#[test]
fn test_particles_dim_in_shadows() {
    let config = ParticleRendererConfig::default();

    // Simulate a well-lit particle (near torch — light level 14)
    let lit_data = ParticleLightData::responsive(14);
    let lit_color = compute_lit_vertex_color(
        Color::srgba(1.0, 1.0, 1.0, 1.0),
        lit_data,
        0.0,
        &config,
    );

    // Simulate a shadowed particle (in cave — light level 2)
    let shadow_data = ParticleLightData::responsive(2);
    let shadow_color = compute_lit_vertex_color(
        Color::srgba(1.0, 1.0, 1.0, 1.0),
        shadow_data,
        0.0,
        &config,
    );

    assert!(
        lit_color[0] > shadow_color[0],
        "Lit particle (r={}) should be brighter than shadowed (r={})",
        lit_color[0],
        shadow_color[0],
    );

    // Shadow should be significantly darker
    assert!(
        shadow_color[0] < 0.5,
        "Shadow particle should be dim (r={}), expected < 0.5",
        shadow_color[0],
    );
}

// ============================================================================
// Test: Self-illuminated particles ignore ambient light
// ============================================================================

#[test]
fn test_self_illuminated_particles_full_brightness() {
    let config = ParticleRendererConfig::default();

    // Self-illuminated (torch flame) in complete darkness
    let self_lit = ParticleLightData::self_lit();
    let torch_color = compute_lit_vertex_color(
        Color::srgba(1.0, 0.7, 0.2, 1.0),
        self_lit,
        0.0,
        &config,
    );

    // Should be at full base color despite "dark" surroundings
    assert!(
        (torch_color[0] - 1.0).abs() < 0.01,
        "Torch red should be ~1.0, got {}",
        torch_color[0],
    );
    assert!(
        (torch_color[1] - 0.7).abs() < 0.01,
        "Torch green should be ~0.7, got {}",
        torch_color[1],
    );
}

// ============================================================================
// Test: Emitter inherits light at spawn position
// ============================================================================

#[test]
fn test_emitter_captures_light_at_position() {
    let light_query = BlockLightQuery {
        config: BlockLightQueryConfig {
            minimum_light: 0,
            use_sky_estimation: true,
            sky_light_height: 64.0,
            underground_height: 0.0,
        },
    };

    let mut emitter = ParticleEmitter::from_effect(ParticleEffectType::Smoke);

    // Update light at a high position (should be bright)
    emitter.update_light(Vec3::new(0.0, 60.0, 0.0), &light_query);
    let bright_level = emitter.light_sample.block_light;

    // Update light at a low position (should be dim)
    emitter.update_light(Vec3::new(0.0, 5.0, 0.0), &light_query);
    let dim_level = emitter.light_sample.block_light;

    assert!(
        bright_level > dim_level,
        "Emitter at y=60 (light={}) should be brighter than at y=5 (light={})",
        bright_level,
        dim_level,
    );
}

// ============================================================================
// Test: Light level interpolation across heights
// ============================================================================

#[test]
fn test_light_level_gradient() {
    let light_query = BlockLightQuery::default();

    let mut prev_brightness = 0.0;
    let heights = [0.0, 16.0, 32.0, 48.0, 64.0];

    for &y in &heights {
        let sample = light_query.sample_at(Vec3::new(0.0, y, 0.0));
        assert!(
            sample.brightness >= prev_brightness,
            "Light should increase with height: at y={y}, brightness={} < prev={}",
            sample.brightness,
            prev_brightness,
        );
        prev_brightness = sample.brightness;
    }

    // Top should be max
    let top = light_query.sample_at(Vec3::new(0.0, 64.0, 0.0));
    assert_eq!(
        top.block_light, 15,
        "At sky_light_height, light should be max (15), got {}",
        top.block_light,
    );
}

// ============================================================================
// Test: Particle light data pack/unpack roundtrip
// ============================================================================

#[test]
fn test_light_data_shader_roundtrip() {
    // Test all light levels round-trip through pack/unpack
    for level in 0..=15u8 {
        let data = ParticleLightData::responsive(level);
        let packed = data.pack_to_f32();
        let unpacked = ParticleLightData::unpack_from_f32(packed);
        assert_eq!(
            unpacked.block_light, level,
            "Level {level} failed roundtrip: packed={packed}, got {}",
            unpacked.block_light,
        );
        assert!(!unpacked.self_illuminated);
    }

    // Test self-illuminated roundtrip
    let self_lit = ParticleLightData::self_lit();
    let packed = self_lit.pack_to_f32();
    let unpacked = ParticleLightData::unpack_from_f32(packed);
    assert!(unpacked.self_illuminated, "Self-illuminated flag lost in roundtrip");
    assert_eq!(unpacked.block_light, 15);
}

// ============================================================================
// Test: Effect type light behavior consistency
// ============================================================================

#[test]
fn test_all_effects_have_consistent_light_config() {
    for effect in ParticleEffectType::all() {
        let config = effect.default_config();

        if effect.is_self_illuminated() {
            // Self-illuminated effects should not inherit emitter light
            assert!(
                !config.inherit_emitter_light,
                "{effect:?}: self-illuminated effects should not inherit emitter light"
            );
            // Should have glow emission > 0
            assert!(
                effect.glow_emission() > 0,
                "{effect:?}: self-illuminated effects should emit glow"
            );
        }

        // All effects should have valid parameters
        assert!(config.max_particles > 0, "{effect:?}: invalid max_particles");
        assert!(config.lifetime > 0.0, "{effect:?}: invalid lifetime");
        assert!(config.size > 0.0, "{effect:?}: invalid size");
    }
}

// ============================================================================
// Test: Light modulation can be disabled
// ============================================================================

#[test]
fn test_light_modulation_disable() {
    let mut config = ParticleRendererConfig::default();
    config.light_modulation_enabled = false;

    let color = Color::srgba(0.8, 0.6, 0.4, 1.0);
    let dark = ParticleLightData::responsive(0);

    let result = compute_lit_vertex_color(color, dark, 0.0, &config);

    // With modulation disabled, even dark light should not dim the color
    assert!(
        (result[0] - 0.8).abs() < 0.01,
        "With modulation disabled, red should be ~0.8, got {}",
        result[0],
    );
}

// ============================================================================
// Test: Minimum light floor prevents invisible particles
// ============================================================================

#[test]
fn test_minimum_light_floor() {
    let light_query = BlockLightQuery {
        config: BlockLightQueryConfig {
            minimum_light: 3,
            use_sky_estimation: true,
            sky_light_height: 64.0,
            underground_height: 0.0,
        },
    };

    // Even deep underground, minimum light should apply
    let sample = light_query.sample_at(Vec3::new(0.0, -100.0, 0.0));
    assert!(
        sample.block_light >= 3,
        "Minimum light floor should be 3, got {}",
        sample.block_light,
    );
    assert!(
        sample.brightness > 0.0,
        "Brightness should never be zero with minimum_light > 0"
    );
}
