//! Health and Hunger system
//!
//! Provides Health, Hunger components, damage/death events, fall damage,
//! starvation, regeneration, and a HUD overlay.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use crate::actors::{Grounded, Player, Velocity};

// ============================================================================
// COMPONENTS
// ============================================================================

/// Health component for any living entity.
#[derive(Component, Debug, Clone)]
pub struct Health {
    pub current: f32,
    pub max: f32,
    pub regeneration_rate: f32,
    pub invulnerable_timer: f32,
}

impl Health {
    pub fn new(max: f32) -> Self {
        Self {
            current: max,
            max,
            regeneration_rate: 0.5,
            invulnerable_timer: 0.0,
        }
    }

    /// Apply damage. Returns true if the entity died.
    pub fn damage(&mut self, amount: f32) -> bool {
        if self.invulnerable_timer > 0.0 {
            return false;
        }
        self.current = (self.current - amount).max(0.0);
        self.invulnerable_timer = 0.5;
        self.is_dead()
    }

    pub fn heal(&mut self, amount: f32) {
        self.current = (self.current + amount).min(self.max);
    }

    pub fn is_dead(&self) -> bool {
        self.current <= 0.0
    }

    pub fn percentage(&self) -> f32 {
        if self.max <= 0.0 {
            0.0
        } else {
            self.current / self.max
        }
    }
}

/// Hunger component for any living entity.
#[derive(Component, Debug, Clone)]
pub struct Hunger {
    pub current: f32,
    pub max: f32,
    pub depletion_rate: f32,
}

impl Hunger {
    pub fn new(max: f32) -> Self {
        Self {
            current: max,
            max,
            depletion_rate: 0.1,
        }
    }

    pub fn consume(&mut self, amount: f32) {
        self.current = (self.current + amount).min(self.max);
    }

    pub fn deplete(&mut self, amount: f32) {
        self.current = (self.current - amount).max(0.0);
    }

    pub fn is_starving(&self) -> bool {
        self.current <= 0.0
    }

    pub fn percentage(&self) -> f32 {
        if self.max <= 0.0 {
            0.0
        } else {
            self.current / self.max
        }
    }
}

// ============================================================================
// EVENTS
// ============================================================================

/// Source of damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageSource {
    Fall,
    Starving,
    Mob,
    Void,
    Environment,
}

/// Fired when an entity should take damage.
#[derive(Event, Debug, Clone)]
pub struct DamageEvent {
    pub target: Entity,
    pub amount: f32,
    pub source: DamageSource,
}

/// Fired when an entity dies.
#[derive(Event, Debug, Clone)]
pub struct DeathEvent {
    pub entity: Entity,
    pub cause: DamageSource,
}

// ============================================================================
// RESOURCES
// ============================================================================

/// Tracks damage flash for the HUD.
#[derive(Resource, Default)]
struct DamageFlash {
    timer: f32,
}

/// Tracks previous vertical velocity for fall damage detection.
#[derive(Component, Debug, Default)]
struct PreviousVelocityY(f32);

// ============================================================================
// SYSTEMS
// ============================================================================

/// Attach Health, Hunger, and PreviousVelocityY to the player entity.
fn attach_health_to_player(
    mut commands: Commands,
    query: Query<Entity, (With<Player>, Without<Health>)>,
) {
    for entity in &query {
        commands.entity(entity).insert((
            Health::new(20.0),
            Hunger::new(20.0),
            PreviousVelocityY(0.0),
        ));
    }
}

fn apply_damage(
    mut events: EventReader<DamageEvent>,
    mut query: Query<&mut Health>,
    mut death_events: EventWriter<DeathEvent>,
    mut flash: ResMut<DamageFlash>,
) {
    for event in events.read() {
        if let Ok(mut health) = query.get_mut(event.target) {
            let died = health.damage(event.amount);
            if event.amount > 0.0 && health.invulnerable_timer > 0.0 {
                flash.timer = 0.3;
            }
            if died {
                death_events.send(DeathEvent {
                    entity: event.target,
                    cause: event.source,
                });
            }
        }
    }
}

fn update_invulnerability(time: Res<Time>, mut query: Query<&mut Health>) {
    let dt = time.delta_secs();
    for mut health in &mut query {
        if health.invulnerable_timer > 0.0 {
            health.invulnerable_timer = (health.invulnerable_timer - dt).max(0.0);
        }
    }
}

fn regenerate_health(time: Res<Time>, mut query: Query<(&mut Health, &Hunger)>) {
    let dt = time.delta_secs();
    for (mut health, hunger) in &mut query {
        if hunger.percentage() > 0.9 && health.current < health.max {
            let regen = health.regeneration_rate * dt;
            health.heal(regen);
        }
    }
}

fn deplete_hunger(time: Res<Time>, mut query: Query<&mut Hunger>) {
    let dt = time.delta_secs();
    for mut hunger in &mut query {
        let amount = hunger.depletion_rate * dt;
        hunger.deplete(amount);
    }
}

fn starvation_damage(
    time: Res<Time>,
    query: Query<(Entity, &Hunger)>,
    mut damage_events: EventWriter<DamageEvent>,
) {
    let dt = time.delta_secs();
    for (entity, hunger) in &query {
        if hunger.is_starving() {
            damage_events.send(DamageEvent {
                target: entity,
                amount: 1.0 * dt,
                source: DamageSource::Starving,
            });
        }
    }
}

fn check_death(
    query: Query<(Entity, &Health), With<Player>>,
    mut death_events: EventWriter<DeathEvent>,
) {
    for (entity, health) in &query {
        if health.is_dead() {
            death_events.send(DeathEvent {
                entity,
                cause: DamageSource::Environment,
            });
        }
    }
}

fn handle_player_death(
    mut events: EventReader<DeathEvent>,
    mut query: Query<(&mut Health, &mut Hunger, &mut Transform), With<Player>>,
) {
    for event in events.read() {
        if let Ok((mut health, mut hunger, mut transform)) = query.get_mut(event.entity) {
            health.current = health.max;
            health.invulnerable_timer = 1.0;
            hunger.current = hunger.max;
            transform.translation = Vec3::new(32.0, 80.0, 32.0);
        }
    }
}

fn fall_damage(
    mut query: Query<(Entity, &Velocity, &Grounded, &mut PreviousVelocityY), With<Player>>,
    mut damage_events: EventWriter<DamageEvent>,
) {
    for (entity, velocity, grounded, mut prev_vel) in &mut query {
        if grounded.is_grounded && prev_vel.0 < -10.0 {
            let fall_speed = prev_vel.0.abs();
            let damage = (fall_speed - 10.0) * 1.5;
            damage_events.send(DamageEvent {
                target: entity,
                amount: damage,
                source: DamageSource::Fall,
            });
        }
        prev_vel.0 = velocity.linear.y;
    }
}

fn update_damage_flash(time: Res<Time>, mut flash: ResMut<DamageFlash>) {
    if flash.timer > 0.0 {
        flash.timer = (flash.timer - time.delta_secs()).max(0.0);
    }
}

// ============================================================================
// HUD
// ============================================================================

fn health_hud_system(
    mut contexts: EguiContexts,
    query: Query<(&Health, &Hunger), With<Player>>,
    flash: Res<DamageFlash>,
) {
    let Ok((health, hunger)) = query.get_single() else {
        return;
    };

    let ctx = contexts.ctx_mut();

    egui::Area::new(egui::Id::new("hud_health_hunger"))
        .fixed_pos(egui::pos2(16.0, 60.0))
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 140))
                .rounding(egui::Rounding::same(6.0))
                .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                .show(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);

                    let health_color = if flash.timer > 0.0 {
                        egui::Color32::from_rgb(255, 80, 80)
                    } else {
                        egui::Color32::from_rgb(220, 50, 50)
                    };

                    ui.label(
                        egui::RichText::new(format!("Γ¥ñ {:.1}/{}", health.current, health.max))
                            .color(health_color)
                            .size(14.0)
                            .strong(),
                    );

                    let health_bar = egui::ProgressBar::new(health.percentage()).fill(health_color);
                    ui.add_sized([160.0, 12.0], health_bar);

                    ui.add_space(4.0);

                    let hunger_color = egui::Color32::from_rgb(200, 150, 50);

                    ui.label(
                        egui::RichText::new(format!("≡ƒìû {:.1}/{}", hunger.current, hunger.max))
                            .color(hunger_color)
                            .size(14.0)
                            .strong(),
                    );

                    let hunger_bar = egui::ProgressBar::new(hunger.percentage()).fill(hunger_color);
                    ui.add_sized([160.0, 12.0], hunger_bar);
                });
        });
}

// ============================================================================
// PLUGIN
// ============================================================================

pub struct HealthPlugin;

impl Plugin for HealthPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<DamageEvent>()
            .add_event::<DeathEvent>()
            .init_resource::<DamageFlash>()
            .add_systems(Update, attach_health_to_player)
            .add_systems(
                Update,
                (
                    update_invulnerability,
                    deplete_hunger,
                    starvation_damage,
                    fall_damage,
                    apply_damage,
                    check_death,
                    handle_player_death,
                    regenerate_health,
                    update_damage_flash,
                    health_hud_system,
                )
                    .chain()
                    .after(attach_health_to_player),
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
    fn test_health_damage() {
        let mut health = Health::new(20.0);
        health.damage(5.0);
        assert!((health.current - 15.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_health_death() {
        let mut health = Health::new(20.0);
        let died = health.damage(25.0);
        assert!(died);
        assert!(health.is_dead());
        assert_eq!(health.current, 0.0);
    }

    #[test]
    fn test_health_heal() {
        let mut health = Health::new(20.0);
        health.damage(10.0);
        health.invulnerable_timer = 0.0;
        health.heal(5.0);
        assert!((health.current - 15.0).abs() < f32::EPSILON);
        health.heal(100.0);
        assert!((health.current - 20.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_health_invulnerability() {
        let mut health = Health::new(20.0);
        health.damage(5.0);
        assert!((health.current - 15.0).abs() < f32::EPSILON);
        let died = health.damage(5.0);
        assert!(!died);
        assert!((health.current - 15.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_hunger_depletion() {
        let mut hunger = Hunger::new(20.0);
        hunger.deplete(5.0);
        assert!((hunger.current - 15.0).abs() < f32::EPSILON);
        hunger.deplete(100.0);
        assert_eq!(hunger.current, 0.0);
    }

    #[test]
    fn test_hunger_starving() {
        let mut hunger = Hunger::new(20.0);
        assert!(!hunger.is_starving());
        hunger.deplete(20.0);
        assert!(hunger.is_starving());
    }

    #[test]
    fn test_fall_damage_calculation() {
        let fall_speed: f32 = 15.0;
        let damage = (fall_speed - 10.0) * 1.5;
        assert!((damage - 7.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_health_percentage() {
        let mut health = Health::new(20.0);
        assert!((health.percentage() - 1.0).abs() < f32::EPSILON);
        health.damage(10.0);
        assert!((health.percentage() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_hunger_consume() {
        let mut hunger = Hunger::new(20.0);
        hunger.deplete(10.0);
        hunger.consume(5.0);
        assert!((hunger.current - 15.0).abs() < f32::EPSILON);
        hunger.consume(100.0);
        assert!((hunger.current - 20.0).abs() < f32::EPSILON);
    }
}
