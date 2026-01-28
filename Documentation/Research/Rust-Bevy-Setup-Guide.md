# Rust + Bevy Setup Guide for Procedural Worlds

## Decision Summary

- **Language**: Rust
- **Framework**: Bevy (ECS game engine)
- **Graphics**: wgpu (via Bevy)
- **UI**: egui via bevy_egui
- **Python**: PyO3

---

## Prerequisites

### Install Rust

**Windows** (recommended method):
```powershell
# Install rustup (Rust toolchain manager)
winget install Rustlang.Rustup

# Or download from https://rustup.rs
```

**Verify installation**:
```bash
rustc --version    # Should be 1.83+
cargo --version
```

### VS Code Extensions (Recommended)

- **rust-analyzer**: Language server (essential)
- **Even Better TOML**: Cargo.toml syntax
- **crates**: Dependency version info
- **CodeLLDB**: Debugging

---

## Project Setup

### Initialize New Rust Project

```bash
cd "C:\Users\Owner\Desktop\Projects\Procedural Worlds"

# Create new Rust project (will create src/ directory)
cargo init --name procedural_worlds

# Or if you want a fresh directory:
# cargo new procedural_worlds_rust
```

### Cargo.toml Configuration

```toml
[package]
name = "procedural_worlds"
version = "0.1.0"
edition = "2024"
authors = ["Ian Tharp <praht09ian@gmail.com>"]
description = "AI-driven voxel game engine for collaborative world-building"

[dependencies]
# Bevy game engine
bevy = { version = "0.17", features = ["dynamic_linking"] }

# Editor UI
bevy_egui = "0.32"

# Voxel world (optional - can build custom)
# bevy_voxel_world = "0.6"

# Math and noise
glam = "0.29"
noise = "0.9"

# Parallel processing
rayon = "1.10"

# Python integration (optional initially)
# pyo3 = { version = "0.23", features = ["auto-initialize"] }

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# Logging
log = "0.4"
env_logger = "0.11"

[dev-dependencies]
criterion = "0.5"  # Benchmarking

# Faster compilation during development
[profile.dev]
opt-level = 1

[profile.dev.package."*"]
opt-level = 3

# Enable dynamic linking for faster compile times during development
# Remove for release builds
```

### Project Structure

```
procedural_worlds/
├── Cargo.toml
├── Cargo.lock                 # Auto-generated
├── src/
│   ├── main.rs               # Entry point
│   ├── lib.rs                # Library root (for tests)
│   ├── engine/
│   │   ├── mod.rs
│   │   └── camera.rs
│   ├── world/
│   │   ├── mod.rs
│   │   ├── chunk.rs
│   │   ├── block.rs
│   │   └── meshing.rs
│   ├── editor/
│   │   ├── mod.rs
│   │   └── viewport.rs
│   └── generation/
│       ├── mod.rs
│       └── terrain.rs
├── assets/
│   ├── textures/
│   └── shaders/              # Custom shaders if needed
├── scripts/                   # Python scripts (future)
├── Documentation/
│   └── Research/
├── Engine/                    # Archive of C++ code
└── .cargo/
    └── config.toml           # Cargo configuration
```

---

## Minimal Bevy Example

### src/main.rs

```rust
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin};

fn main() {
    App::new()
        // Default Bevy plugins (window, rendering, input, etc.)
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Procedural Worlds Engine".into(),
                resolution: (1280., 720.).into(),
                ..default()
            }),
            ..default()
        }))
        // Editor UI
        .add_plugins(EguiPlugin)
        // Our systems
        .add_systems(Startup, setup)
        .add_systems(Update, (editor_ui, camera_controller))
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 5.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Light
    commands.spawn((
        DirectionalLight {
            illuminance: 10000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.5, 0.5, 0.0)),
    ));

    // Test cube (placeholder for voxel chunk)
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.3, 0.5, 0.3))),
        Transform::from_xyz(0.0, 0.5, 0.0),
    ));

    // Ground plane
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(20.0, 20.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.2, 0.2, 0.2))),
    ));

    info!("Procedural Worlds Engine initialized");
}

fn editor_ui(mut contexts: EguiContexts) {
    egui::TopBottomPanel::top("menu_bar").show(contexts.ctx_mut(), |ui| {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New World").clicked() {
                    // TODO: Implement
                }
                if ui.button("Exit").clicked() {
                    std::process::exit(0);
                }
            });
            ui.menu_button("View", |ui| {
                if ui.button("Viewport").clicked() {
                    // TODO: Toggle viewport
                }
            });
        });
    });

    egui::SidePanel::left("inspector").show(contexts.ctx_mut(), |ui| {
        ui.heading("Inspector");
        ui.separator();
        ui.label("World Properties");
        // TODO: Add world editing controls
    });
}

fn camera_controller(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut query: Query<&mut Transform, With<Camera3d>>,
    time: Res<Time>,
) {
    let speed = 5.0 * time.delta_secs();

    for mut transform in &mut query {
        let forward = transform.forward();
        let right = transform.right();

        if keyboard.pressed(KeyCode::KeyW) {
            transform.translation += forward * speed;
        }
        if keyboard.pressed(KeyCode::KeyS) {
            transform.translation -= forward * speed;
        }
        if keyboard.pressed(KeyCode::KeyA) {
            transform.translation -= right * speed;
        }
        if keyboard.pressed(KeyCode::KeyD) {
            transform.translation += right * speed;
        }
        if keyboard.pressed(KeyCode::Space) {
            transform.translation.y += speed;
        }
        if keyboard.pressed(KeyCode::ShiftLeft) {
            transform.translation.y -= speed;
        }
    }
}
```

---

## Build and Run

```bash
# Development build (fast compile, slow runtime)
cargo run

# Release build (slow compile, fast runtime)
cargo run --release

# Check for errors without building
cargo check

# Run tests
cargo test

# Generate documentation
cargo doc --open
```

---

## Next Steps After Setup

1. **Verify Bevy window opens** with test cube
2. **Add chunk data structure** (16x16x16 blocks)
3. **Implement basic meshing** (naive first, then greedy)
4. **Add block type registry**
5. **Implement terrain generation** using noise crate
6. **Add texture atlas** for block faces

---

## Useful Bevy Resources

- [Bevy Book](https://bevyengine.org/learn/book/introduction/)
- [Bevy Cheat Book](https://bevy-cheatbook.github.io/)
- [Bevy Examples](https://github.com/bevyengine/bevy/tree/main/examples)
- [bevy_voxel_world](https://github.com/splashdust/bevy_voxel_world)
- [vx_bevy](https://github.com/Game4all/vx_bevy) - Minecraft-style reference

---

## Preserving C++ Work

The existing C++ code should be archived but not deleted:
- Rename `Engine/` to `_archive_cpp_engine/` or similar
- Keep for reference on patterns and structure
- CMakeLists.txt and build scripts can be removed once Rust is confirmed working
