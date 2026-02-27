//! Integration tests for the particle effects system.

use bevy::prelude::*;
use procedural_worlds::particles::*;
use procedural_worlds::particles::emitter::EmitterBuilder;
use procedural_worlds::particles::effects::*;
use procedural_worlds::weather::WeatherType;

// ============================================================================
// EMITTER LIFECYCLE TESTS
// ============================================================================

#[test]
fn test_emitter_lifecycle_finite_emitter() {
    let emitter = EmitterBuilder::new(ParticleEffect::Dust)
        .emitter_lifetime(Some(5.0))
        .build();
    assert_eq!(emitter.age, 0.0);
    assert!(emitter.enabled);
    assert_eq!(emitter.emitter_lifetime, Some(5.0));
}

#[test]
fn test_emitter_lifecycle_infinite_emitter() {
    let emitter = EmitterBuilder::new(ParticleEffect::Light)
        .build();
    assert!(emitter.emitter_lifetime.is_none());
    assert!(emitter.enabled);
}

// ============================================================================
// EFFECT CONFIGURATION TESTS
// ============================================================================

#[test]
fn test_water_splash_scales_with_speed() {
    let slow = WaterSplashEffect::from_speed(1.0);
    let fast = WaterSplashEffect::from_speed(10.0);

    let slow_rate = match slow.mode {
        EmissionMode::Continuous { rate } => rate,
        _ => panic!("Expected continuous"),
    };
    let fast_rate = match fast.mode {
        EmissionMode::Continuous { rate } => rate,
        _ => panic!("Expected continuous"),
    };

    assert!(fast_rate > slow_rate, "Faster movement = more particles");
    assert!(fast.particle_speed > slow.particle_speed, "Faster movement = faster particles");
}

#[test]
fn test_light_emission_scales_with_level() {
    let dim = LightEmissionEffect::from_light_level(3).unwrap();
    let bright = LightEmissionEffect::from_light_level(15).unwrap();

    let dim_rate = match dim.mode {
        EmissionMode::Continuous { rate } => rate,
        _ => panic!("Expected continuous"),
    };
    let bright_rate = match bright.mode {
        EmissionMode::Continuous { rate } => rate,
        _ => panic!("Expected continuous"),
    };

    assert!(bright_rate > dim_rate, "Brighter light = more particles");
    assert!(bright.max_particles > dim.max_particles);
}

#[test]
fn test_dust_hardness_scales_particle_count() {
    let soft = DustParticleEffect::block_break([0.5, 0.5, 0.5, 1.0], 1.0);
    let hard = DustParticleEffect::block_break([0.5, 0.5, 0.5, 1.0], 5.0);

    let soft_count = match soft.mode {
        EmissionMode::Burst { count, .. } => count,
        _ => panic!("Expected burst"),
    };
    let hard_count = match hard.mode {
        EmissionMode::Burst { count, .. } => count,
        _ => panic!("Expected burst"),
    };

    assert!(hard_count >= soft_count, "Harder blocks = more debris");
}

#[test]
fn test_all_weather_types_produce_effects() {
    for weather in [WeatherType::Rain, WeatherType::Snow, WeatherType::Storm] {
        let emitter = WeatherParticleEffect::from_weather(weather, 0.5);
        assert!(emitter.is_some(), "{:?} should produce particles", weather);
    }
    assert!(WeatherParticleEffect::from_weather(WeatherType::Clear, 1.0).is_none());
}

// ============================================================================
// PARTICLE COUNT & LIFETIME TESTS
// ============================================================================

#[test]
fn test_particle_lifetime_positive() {
    let effects = [
        WaterSplashEffect::from_speed(5.0),
        LightEmissionEffect::torch(),
        DustParticleEffect::block_break([0.5; 4], 2.0),
        WeatherParticleEffect::from_weather(WeatherType::Rain, 0.5).unwrap(),
    ];

    for emitter in &effects {
        assert!(emitter.particle_lifetime > 0.0, "Lifetime must be positive for {:?}", emitter.effect);
        assert!(emitter.max_particles > 0, "Max particles must be > 0 for {:?}", emitter.effect);
    }
}

#[test]
fn test_particle_config_defaults_reasonable() {
    let config = ParticleConfig::default();
    assert!(config.max_total_particles >= 5000, "Should support 5K+ particles");
    assert!(config.gravity > 0.0);
    assert!(config.enabled);
}

// ============================================================================
// BUILDER PATTERN TESTS
// ============================================================================

#[test]
fn test_builder_chaining() {
    let emitter = EmitterBuilder::new(ParticleEffect::Water)
        .continuous(30.0)
        .lifetime(2.0)
        .speed(10.0)
        .color([1.0, 0.0, 0.0, 1.0])
        .size(0.15)
        .max_particles(500)
        .enabled(false)
        .emitter_lifetime(Some(10.0))
        .build();

    assert_eq!(emitter.effect, ParticleEffect::Water);
    assert!(!emitter.enabled);
    assert_eq!(emitter.particle_lifetime, 2.0);
    assert_eq!(emitter.particle_speed, 10.0);
    assert_eq!(emitter.max_particles, 500);
    assert_eq!(emitter.emitter_lifetime, Some(10.0));
}

// ============================================================================
// BEVY APP INTEGRATION TESTS
// ============================================================================

#[test]
fn test_particle_plugin_initializes_resources() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(ParticlePlugin);
    app.update();

    assert!(app.world().get_resource::<ParticleConfig>().is_some());
    assert!(app.world().get_resource::<ParticleStats>().is_some());
}

#[test]
fn test_particle_system_runs_without_emitters() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(ParticlePlugin);

    // Should not panic with no emitters
    for _ in 0..10 {
        app.update();
    }

    let stats = app.world().get_resource::<ParticleStats>().unwrap();
    assert_eq!(stats.active_count, 0);
}

// ============================================================================
// STRESS / PERFORMANCE TESTS
// ============================================================================

#[test]
fn test_high_particle_count_configuration() {
    // Verify that creating 10K particle configurations doesn't blow up
    let mut emitters = Vec::with_capacity(10_000);
    for i in 0..10_000 {
        let effect = match i % 4 {
            0 => ParticleEffect::Water,
            1 => ParticleEffect::Light,
            2 => ParticleEffect::Dust,
            _ => ParticleEffect::Weather,
        };
        emitters.push(EmitterBuilder::new(effect).max_particles(1).build());
    }
    assert_eq!(emitters.len(), 10_000);
}

#[test]
fn test_particle_aging_logic() {
    // Simulate particle aging
    let lifetime = 2.0;
    let mut age = 0.0;
    let dt = 0.016; // ~60fps

    let mut frames = 0;
    while age < lifetime {
        age += dt;
        frames += 1;
    }

    assert!(frames > 100, "2s lifetime at 60fps should be ~125 frames, got {frames}");
    assert!(frames < 150, "Should not take more than 150 frames");
}

#[test]
fn test_fade_at_end_of_life() {
    // Particles at >70% lifetime should start fading
    let lifetime = 2.0;
    let age_70 = lifetime * 0.7;
    let age_85 = lifetime * 0.85;
    let age_100 = lifetime;

    let fade_at_70 = 1.0 - ((age_70 / lifetime - 0.7) / 0.3_f32).clamp(0.0, 1.0);
    let fade_at_85 = 1.0 - ((age_85 / lifetime - 0.7) / 0.3_f32).clamp(0.0, 1.0);
    let fade_at_100 = 1.0 - ((age_100 / lifetime - 0.7) / 0.3_f32).clamp(0.0, 1.0);

    assert!((fade_at_70 - 1.0).abs() < 0.01, "No fade at 70%");
    assert!((fade_at_85 - 0.5).abs() < 0.01, "Half fade at 85%");
    assert!(fade_at_100.abs() < 0.01, "Fully faded at 100%");
}
