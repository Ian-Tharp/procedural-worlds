//! Biome-aware weather system with rain and snow particles.
//!
//! Provides dynamic weather that changes based on the player's current biome,
//! with smooth transitions between weather states and particle effects.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use noise::Simplex;

use crate::actors::Player;
use crate::engine::lighting::DayNightCycle;
use crate::generation::biome::{biome_at, BiomeType};
use crate::generation::TerrainConfig;

// ============================================================================
// WEATHER TYPE
// ============================================================================

/// The possible weather conditions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum WeatherType {
    /// Clear skies, no precipitation.
    #[default]
    Clear,
    /// Rainfall.
    Rain,
    /// Snowfall.
    Snow,
    /// Storm — heavy rain with reduced visibility.
    Storm,
}

impl WeatherType {
    /// Human-readable display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Clear => "Clear",
            Self::Rain => "Rain",
            Self::Snow => "Snow",
            Self::Storm => "Storm",
        }
    }

    /// Icon text for HUD display.
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Clear => "[sun]",
            Self::Rain => "[rain]",
            Self::Snow => "[snow]",
            Self::Storm => "[storm]",
        }
    }
}

// ============================================================================
// WEATHER STATE
// ============================================================================

/// Resource tracking the current weather conditions.
#[derive(Resource)]
pub struct WeatherState {
    /// Current weather type.
    pub current: WeatherType,
    /// Weather we're transitioning toward (if different from current).
    pub target: WeatherType,
    /// Current intensity (0.0-1.0). Ramps up/down during transitions.
    pub intensity: f32,
    /// Timer until the next weather roll.
    pub transition_timer: Timer,
    /// Whether we're currently fading out the old weather before switching.
    pub fading_out: bool,
}

impl Default for WeatherState {
    fn default() -> Self {
        Self {
            current: WeatherType::Clear,
            target: WeatherType::Clear,
            intensity: 0.0,
            transition_timer: Timer::from_seconds(60.0, TimerMode::Repeating),
            fading_out: false,
        }
    }
}

// ============================================================================
// BIOME WEATHER PROBABILITIES
// ============================================================================

/// Weather probability distribution for a biome.
/// Values should sum to approximately 1.0.
#[derive(Clone, Debug)]
pub struct WeatherProbabilities {
    pub clear: f32,
    pub rain: f32,
    pub snow: f32,
    pub storm: f32,
}

impl WeatherProbabilities {
    /// Select a weather type given a random value in [0.0, 1.0).
    pub fn select(&self, roll: f32) -> WeatherType {
        let r = roll.clamp(0.0, 0.9999);
        if r < self.clear {
            WeatherType::Clear
        } else if r < self.clear + self.rain {
            WeatherType::Rain
        } else if r < self.clear + self.rain + self.snow {
            WeatherType::Snow
        } else {
            WeatherType::Storm
        }
    }
}

/// Returns weather probabilities for a given biome.
pub fn biome_weather_probabilities(biome: BiomeType) -> WeatherProbabilities {
    match biome {
        BiomeType::Plains => WeatherProbabilities {
            clear: 0.50, rain: 0.30, snow: 0.05, storm: 0.15,
        },
        BiomeType::Desert => WeatherProbabilities {
            clear: 0.85, rain: 0.05, snow: 0.0, storm: 0.10,
        },
        BiomeType::Forest => WeatherProbabilities {
            clear: 0.40, rain: 0.35, snow: 0.05, storm: 0.20,
        },
        BiomeType::Mountains => WeatherProbabilities {
            clear: 0.35, rain: 0.20, snow: 0.30, storm: 0.15,
        },
        BiomeType::Tundra => WeatherProbabilities {
            clear: 0.30, rain: 0.05, snow: 0.55, storm: 0.10,
        },
        BiomeType::Volcanic => WeatherProbabilities {
            clear: 0.60, rain: 0.10, snow: 0.0, storm: 0.30,
        },
        BiomeType::Swamp => WeatherProbabilities {
            clear: 0.25, rain: 0.45, snow: 0.0, storm: 0.30,
        },
        BiomeType::Savanna => WeatherProbabilities {
            clear: 0.65, rain: 0.20, snow: 0.0, storm: 0.15,
        },
        BiomeType::Taiga => WeatherProbabilities {
            clear: 0.30, rain: 0.10, snow: 0.50, storm: 0.10,
        },
        BiomeType::Jungle => WeatherProbabilities {
            clear: 0.30, rain: 0.40, snow: 0.0, storm: 0.30,
        },
        BiomeType::Badlands => WeatherProbabilities {
            clear: 0.70, rain: 0.10, snow: 0.0, storm: 0.20,
        },
        BiomeType::Mushroom => WeatherProbabilities {
            clear: 0.50, rain: 0.30, snow: 0.05, storm: 0.15,
        },
    }
}

// ============================================================================
// COMPONENTS
// ============================================================================

/// Marker component for weather particle entities.
#[derive(Component)]
pub struct WeatherParticle {
    /// Velocity of this particle (blocks/sec).
    pub velocity: Vec3,
    /// Lifetime remaining in seconds.
    pub lifetime: f32,
}

// ============================================================================
// CONSTANTS
// ============================================================================

const INTENSITY_RAMP_SPEED: f32 = 0.5;
const MAX_PARTICLES_PER_FRAME: usize = 20;
const SPAWN_RADIUS: f32 = 30.0;
const SPAWN_HEIGHT: f32 = 25.0;
const DESPAWN_Y: f32 = 0.0;
const RAIN_SPEED: f32 = 20.0;
const SNOW_SPEED: f32 = 4.0;
const SNOW_DRIFT: f32 = 1.5;
const PARTICLE_LIFETIME: f32 = 5.0;

// ============================================================================
// SYSTEMS
// ============================================================================

/// Periodically rolls new weather based on the player's biome.
pub fn weather_transition_system(
    time: Res<Time>,
    mut weather: ResMut<WeatherState>,
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
    terrain_config: Res<TerrainConfig>,
) {
    let dt = time.delta_secs();

    if weather.fading_out {
        weather.intensity = (weather.intensity - INTENSITY_RAMP_SPEED * dt).max(0.0);
        if weather.intensity <= 0.0 {
            weather.current = weather.target;
            weather.fading_out = false;
        }
    } else if weather.current != WeatherType::Clear {
        weather.intensity = (weather.intensity + INTENSITY_RAMP_SPEED * dt).min(1.0);
    }

    weather.transition_timer.tick(time.delta());
    if !weather.transition_timer.just_finished() {
        return;
    }

    let Ok(camera_global) = camera_query.get_single() else {
        return;
    };
    let pos = camera_global.translation();
    let world_x = pos.x.floor() as i32;
    let world_z = pos.z.floor() as i32;
    let biome_noise = Simplex::new(
        terrain_config.seed.wrapping_add(terrain_config.biome_seed_offset),
    );
    let current_biome = biome_at(world_x, world_z, &biome_noise, terrain_config.biome_scale);

    let probs = biome_weather_probabilities(current_biome);
    let roll = ((time.elapsed_secs() * 1000.0) % 1000.0) / 1000.0;
    let new_weather = probs.select(roll);

    if new_weather != weather.current && !weather.fading_out {
        weather.target = new_weather;
        if weather.current == WeatherType::Clear {
            weather.current = new_weather;
            weather.intensity = 0.0;
        } else {
            weather.fading_out = true;
        }
    }
}

/// Spawns rain/snow particle entities around the player.
pub fn spawn_weather_particles_system(
    mut commands: Commands,
    weather: Res<WeatherState>,
    time: Res<Time>,
    player_query: Query<&GlobalTransform, With<Player>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if weather.intensity <= 0.0 {
        return;
    }

    let weather_type = weather.current;
    if weather_type == WeatherType::Clear {
        return;
    }

    let Ok(player_transform) = player_query.get_single() else {
        return;
    };
    let player_pos = player_transform.translation();

    let storm_multiplier = if weather_type == WeatherType::Storm { 2.0 } else { 1.0 };
    let count = ((MAX_PARTICLES_PER_FRAME as f32) * weather.intensity * storm_multiplier) as usize;
    let count = count.min(MAX_PARTICLES_PER_FRAME * 2);

    let t = time.elapsed_secs();

    for i in 0..count {
        let seed = t * 1000.0 + i as f32;
        let angle = (seed * 2.3283) % std::f32::consts::TAU;
        let dist = (seed * 0.7235) % 1.0 * SPAWN_RADIUS;

        let x = player_pos.x + angle.cos() * dist;
        let z = player_pos.z + angle.sin() * dist;
        let y = player_pos.y + SPAWN_HEIGHT;

        let (velocity, color, scale) = match weather_type {
            WeatherType::Rain | WeatherType::Storm => {
                let speed = if weather_type == WeatherType::Storm {
                    RAIN_SPEED * 1.5
                } else {
                    RAIN_SPEED
                };
                (
                    Vec3::new(0.0, -speed, 0.0),
                    Color::srgba(0.6, 0.7, 0.9, 0.5),
                    Vec3::new(0.02, 0.3, 0.02),
                )
            }
            WeatherType::Snow => {
                let drift_x = ((seed * 1.337) % 2.0 - 1.0) * SNOW_DRIFT;
                let drift_z = ((seed * 0.931) % 2.0 - 1.0) * SNOW_DRIFT;
                (
                    Vec3::new(drift_x, -SNOW_SPEED, drift_z),
                    Color::srgba(0.95, 0.95, 1.0, 0.8),
                    Vec3::splat(0.08),
                )
            }
            WeatherType::Clear => unreachable!(),
        };

        let mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
        let material = materials.add(StandardMaterial {
            base_color: color,
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        });

        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(Vec3::new(x, y, z)).with_scale(scale),
            WeatherParticle {
                velocity,
                lifetime: PARTICLE_LIFETIME,
            },
        ));
    }
}

/// Moves weather particles and despawns them when they expire or hit ground.
pub fn update_weather_particles_system(
    mut commands: Commands,
    time: Res<Time>,
    mut particles: Query<(Entity, &mut Transform, &mut WeatherParticle)>,
) {
    let dt = time.delta_secs();

    for (entity, mut transform, mut particle) in &mut particles {
        transform.translation += particle.velocity * dt;
        particle.lifetime -= dt;

        if particle.lifetime <= 0.0 || transform.translation.y < DESPAWN_Y {
            commands.entity(entity).despawn();
        }
    }
}

/// Egui HUD showing current weather and estimated temperature.
pub fn weather_hud_system(
    mut contexts: EguiContexts,
    weather: Res<WeatherState>,
    day_night: Option<Res<DayNightCycle>>,
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
    terrain_config: Option<Res<TerrainConfig>>,
) {
    let biome = if let (Ok(cam), Some(tc)) = (camera_query.get_single(), terrain_config.as_ref()) {
        let pos = cam.translation();
        let biome_noise = Simplex::new(tc.seed.wrapping_add(tc.biome_seed_offset));
        Some(biome_at(pos.x.floor() as i32, pos.z.floor() as i32, &biome_noise, tc.biome_scale))
    } else {
        None
    };

    let base_temp = biome.map(|b| biome_base_temperature(b)).unwrap_or(20.0);

    let time_offset = day_night
        .as_ref()
        .map(|dn| {
            let noon_dist = (dn.time_of_day - 0.5).abs();
            -noon_dist * 10.0
        })
        .unwrap_or(0.0);

    let temperature = base_temp + time_offset;

    let weather_text = format!(
        "{} {} {:.0} C",
        weather.current.icon(),
        weather.current.display_name(),
        temperature,
    );

    egui::Area::new(egui::Id::new("weather_hud"))
        .fixed_pos(egui::pos2(10.0, 10.0))
        .order(egui::Order::Foreground)
        .show(contexts.ctx_mut(), |ui| {
            egui::Frame::none()
                .fill(egui::Color32::from_rgba_premultiplied(0, 0, 0, 140))
                .rounding(egui::Rounding::same(4.0))
                .inner_margin(egui::Margin::symmetric(6.0, 6.0))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(&weather_text)
                            .color(egui::Color32::WHITE)
                            .size(14.0),
                    );
                    if weather.intensity > 0.0 && weather.current != WeatherType::Clear {
                        let bar_width = 60.0 * weather.intensity;
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(60.0, 4.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(rect.min, egui::vec2(bar_width, 4.0)),
                            egui::Rounding::same(2.0),
                            egui::Color32::from_rgb(100, 160, 255),
                        );
                    }
                });
        });
}

/// Base temperature (Celsius) for each biome.
pub fn biome_base_temperature(biome: BiomeType) -> f32 {
    match biome {
        BiomeType::Plains => 18.0,
        BiomeType::Desert => 38.0,
        BiomeType::Forest => 16.0,
        BiomeType::Mountains => 5.0,
        BiomeType::Tundra => -10.0,
        BiomeType::Volcanic => 45.0,
        BiomeType::Swamp => 22.0,
        BiomeType::Savanna => 30.0,
        BiomeType::Taiga => -5.0,
        BiomeType::Jungle => 28.0,
        BiomeType::Badlands => 35.0,
        BiomeType::Mushroom => 15.0,
    }
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that adds the biome-aware weather system.
pub struct WeatherPlugin;

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WeatherState>()
            .add_systems(
                Update,
                (
                    weather_transition_system,
                    spawn_weather_particles_system,
                    update_weather_particles_system,
                    weather_hud_system,
                )
                    .chain(),
            );
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_weather_type_default_is_clear() {
        assert_eq!(WeatherType::default(), WeatherType::Clear);
    }

    #[test]
    fn test_weather_probabilities_select() {
        let probs = WeatherProbabilities {
            clear: 0.5, rain: 0.3, snow: 0.1, storm: 0.1,
        };
        assert_eq!(probs.select(0.0), WeatherType::Clear);
        assert_eq!(probs.select(0.49), WeatherType::Clear);
        assert_eq!(probs.select(0.5), WeatherType::Rain);
        assert_eq!(probs.select(0.79), WeatherType::Rain);
        assert_eq!(probs.select(0.80), WeatherType::Snow);
        assert_eq!(probs.select(0.85), WeatherType::Snow);
        assert_eq!(probs.select(0.95), WeatherType::Storm);
        assert_eq!(probs.select(0.99), WeatherType::Storm);
    }

    #[test]
    fn test_biome_weather_desert_rarely_rains() {
        let probs = biome_weather_probabilities(BiomeType::Desert);
        assert!(probs.clear > 0.7, "Desert should be mostly clear");
        assert!(probs.rain < 0.1, "Desert should rarely rain");
        assert_eq!(probs.snow, 0.0, "Desert should never snow");
    }

    #[test]
    fn test_biome_weather_taiga_often_snows() {
        let probs = biome_weather_probabilities(BiomeType::Taiga);
        assert!(probs.snow >= 0.4, "Taiga should snow often, got {}", probs.snow);
    }

    #[test]
    fn test_biome_weather_swamp_often_rains() {
        let probs = biome_weather_probabilities(BiomeType::Swamp);
        assert!(probs.rain >= 0.4, "Swamp should rain often, got {}", probs.rain);
    }

    #[test]
    fn test_all_biomes_probabilities_sum_to_one() {
        for biome in BiomeType::all() {
            let p = biome_weather_probabilities(*biome);
            let sum = p.clear + p.rain + p.snow + p.storm;
            assert!(
                (sum - 1.0).abs() < 0.01,
                "{:?} probabilities sum to {}, expected ~1.0",
                biome, sum
            );
        }
    }

    #[test]
    fn test_biome_base_temperatures() {
        assert!(biome_base_temperature(BiomeType::Desert) > 30.0);
        assert!(biome_base_temperature(BiomeType::Tundra) < 0.0);
        assert!(biome_base_temperature(BiomeType::Taiga) < 0.0);
        assert!(biome_base_temperature(BiomeType::Jungle) > 20.0);
    }

    #[test]
    fn test_weather_state_default() {
        let state = WeatherState::default();
        assert_eq!(state.current, WeatherType::Clear);
        assert_eq!(state.intensity, 0.0);
        assert!(!state.fading_out);
    }

    #[test]
    fn test_weather_type_display_names() {
        assert_eq!(WeatherType::Clear.display_name(), "Clear");
        assert_eq!(WeatherType::Rain.display_name(), "Rain");
        assert_eq!(WeatherType::Snow.display_name(), "Snow");
        assert_eq!(WeatherType::Storm.display_name(), "Storm");
    }

    #[test]
    fn test_weather_plugin_builds() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_plugins(WeatherPlugin);
    }
}
