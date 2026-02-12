//! Shader hot-reload system for development.
//!
//! Watches `.wgsl` files in the `assets/shaders/` directory and reloads them
//! at runtime when changes are detected. This enables rapid iteration on
//! visual effects without restarting the editor.
//!
//! The watcher only runs in **debug builds** (`cfg(debug_assertions)`).
//! Release builds embed shaders at compile time and skip file watching entirely.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex};

use bevy::prelude::*;
use bevy::render::render_resource::Shader;

#[cfg(debug_assertions)]
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// Directory containing shader source files (relative to the working directory).
const SHADER_DIR: &str = "assets/shaders";

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that enables shader hot-reload in debug/editor builds.
///
/// In release builds this is a no-op — shaders are baked at compile time.
pub struct ShaderHotReloadPlugin;

impl Plugin for ShaderHotReloadPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<ShaderReloadedEvent>()
            .add_event::<ShaderErrorEvent>();

        #[cfg(debug_assertions)]
        {
            app.add_systems(Startup, setup_shader_watcher)
                .add_systems(Update, poll_shader_changes);
        }
    }
}

// ============================================================================
// EVENTS
// ============================================================================

/// Emitted when a shader file is successfully reloaded.
#[derive(Event, Debug, Clone)]
pub struct ShaderReloadedEvent {
    /// Path of the reloaded shader file (relative to working directory).
    pub path: PathBuf,
}

/// Emitted when a shader file fails validation on reload.
#[derive(Event, Debug, Clone)]
pub struct ShaderErrorEvent {
    /// Path of the shader file that failed.
    pub path: PathBuf,
    /// Human-readable error description.
    pub error: String,
}

// ============================================================================
// SHADER REGISTRY
// ============================================================================

/// Maps shader file paths to their Bevy asset handles.
///
/// When a shader file changes, this registry is consulted to find the
/// corresponding `Handle<Shader>` so we can replace the asset in-place.
#[derive(Resource, Default)]
pub struct ShaderRegistry {
    /// Map from canonical file path → weak shader handle.
    entries: HashMap<PathBuf, Handle<Shader>>,
}

impl ShaderRegistry {
    /// Register a shader file path with its Bevy handle.
    ///
    /// Call this at startup for each shader you want to participate in
    /// hot-reload. The handle should be the same weak handle used when
    /// the shader was initially inserted into `Assets<Shader>`.
    pub fn register(&mut self, path: impl Into<PathBuf>, handle: Handle<Shader>) {
        let path = path.into();
        // Canonicalize for consistent matching with notify events
        let canonical = std::fs::canonicalize(&path).unwrap_or(path);
        self.entries.insert(canonical, handle);
    }

    /// Look up the handle for a given path.
    pub fn get(&self, path: &Path) -> Option<&Handle<Shader>> {
        let canonical = std::fs::canonicalize(path).ok()?;
        self.entries.get(&canonical)
    }

    /// Iterate all registered entries.
    pub fn iter(&self) -> impl Iterator<Item = (&PathBuf, &Handle<Shader>)> {
        self.entries.iter()
    }
}

// ============================================================================
// FILE WATCHER (debug only)
// ============================================================================

/// Resource holding the file watcher and its event receiver.
///
/// Follows the same pattern as `ConfigWatcher` in `crate::config`.
#[cfg(debug_assertions)]
#[derive(Resource)]
pub struct ShaderWatcher {
    receiver: Mutex<mpsc::Receiver<Result<Event, notify::Error>>>,
    _watcher: RecommendedWatcher,
}

#[cfg(debug_assertions)]
impl ShaderWatcher {
    /// Create a new watcher monitoring the `assets/shaders/` directory.
    ///
    /// Returns `None` if the watcher cannot be initialized.
    pub fn new() -> Option<Self> {
        let (tx, rx) = mpsc::channel();

        let mut watcher = match RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                let _ = tx.send(res);
            },
            notify::Config::default(),
        ) {
            Ok(w) => w,
            Err(e) => {
                warn!(
                    "Failed to create shader file watcher: {}. Shader hot-reload disabled.",
                    e
                );
                return None;
            }
        };

        let shader_dir = PathBuf::from(SHADER_DIR);
        if !shader_dir.exists() {
            warn!(
                "Shader directory '{}' not found. Shader hot-reload disabled.",
                SHADER_DIR
            );
            return None;
        }

        let watch_path = std::fs::canonicalize(&shader_dir).unwrap_or(shader_dir);

        if let Err(e) = watcher.watch(&watch_path, RecursiveMode::Recursive) {
            warn!(
                "Failed to watch {}: {}. Shader hot-reload disabled.",
                SHADER_DIR, e
            );
            return None;
        }

        info!("Shader hot-reload enabled — watching {}/", SHADER_DIR);
        Some(Self {
            receiver: Mutex::new(rx),
            _watcher: watcher,
        })
    }
}

// ============================================================================
// SYSTEMS (debug only)
// ============================================================================

/// Startup system: initializes the shader watcher and registry.
#[cfg(debug_assertions)]
fn setup_shader_watcher(mut commands: Commands) {
    // Insert the registry (other plugins register their handles into it)
    commands.init_resource::<ShaderRegistry>();

    if let Some(watcher) = ShaderWatcher::new() {
        commands.insert_resource(watcher);
    }
}

/// Per-frame system: polls the watcher for shader file changes and reloads.
///
/// Only runs when `ShaderWatcher` is present (debug builds where the watcher
/// initialized successfully).
#[cfg(debug_assertions)]
fn poll_shader_changes(
    watcher: Res<ShaderWatcher>,
    registry: Res<ShaderRegistry>,
    mut shaders: ResMut<Assets<Shader>>,
    mut reloaded_events: EventWriter<ShaderReloadedEvent>,
    mut error_events: EventWriter<ShaderErrorEvent>,
) {
    let mut changed_paths: Vec<PathBuf> = Vec::new();

    // Drain all pending events
    {
        let receiver = watcher.receiver.lock().unwrap();
        while let Ok(event_result) = receiver.try_recv() {
            match event_result {
                Ok(event) => {
                    if matches!(
                        event.kind,
                        EventKind::Modify(_) | EventKind::Create(_)
                    ) {
                        for path in event.paths {
                            if path.extension().and_then(|e| e.to_str()) == Some("wgsl") {
                                changed_paths.push(path);
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("Shader watcher error: {}", e);
                }
            }
        }
    }

    // Deduplicate (multiple events can fire for the same save)
    changed_paths.sort();
    changed_paths.dedup();

    for path in changed_paths {
        reload_shader(&path, &registry, &mut shaders, &mut reloaded_events, &mut error_events);
    }
}

/// Attempt to reload a single shader file.
#[cfg(debug_assertions)]
fn reload_shader(
    path: &Path,
    registry: &ShaderRegistry,
    shaders: &mut Assets<Shader>,
    reloaded_events: &mut EventWriter<ShaderReloadedEvent>,
    error_events: &mut EventWriter<ShaderErrorEvent>,
) {
    // Read the file
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("Failed to read shader file {}: {}", path.display(), e);
            warn!("{}", msg);
            error_events.send(ShaderErrorEvent {
                path: path.to_path_buf(),
                error: msg,
            });
            return;
        }
    };

    // Validate WGSL before inserting (don't crash on bad shaders)
    if let Err(msg) = validate_wgsl(&source) {
        let err_msg = format!(
            "Shader validation failed for {}:\n{}",
            path.display(),
            msg
        );
        warn!("{}", err_msg);
        error_events.send(ShaderErrorEvent {
            path: path.to_path_buf(),
            error: err_msg,
        });
        return;
    }

    // Find the registered handle for this path
    if let Some(handle) = registry.get(path) {
        let shader_path_str = format!("hot-reload://{}", path.display());
        shaders.insert(handle, Shader::from_wgsl(source, shader_path_str));

        info!("Shader reloaded: {}", path.display());
        reloaded_events.send(ShaderReloadedEvent {
            path: path.to_path_buf(),
        });
    } else {
        debug!(
            "Changed shader {} is not registered for hot-reload, skipping.",
            path.display()
        );
    }
}

// ============================================================================
// WGSL VALIDATION
// ============================================================================

/// Validate WGSL source using naga.
///
/// Returns `Ok(())` if the shader parses successfully, or `Err(message)` with
/// a human-readable error description.
///
/// Note: This validates standalone WGSL only. Shaders that rely on Bevy's
/// `#import` preprocessor directives will fail naga parsing but may still be
/// valid at runtime. We check for `#import` and skip validation in that case.
pub fn validate_wgsl(source: &str) -> Result<(), String> {
    // Bevy's shader preprocessor uses `#import` directives that naga doesn't
    // understand. If the shader uses imports, skip standalone validation.
    if source.contains("#import") || source.contains("#ifdef") || source.contains("#ifndef") {
        return Ok(());
    }

    match naga::front::wgsl::parse_str(source) {
        Ok(_module) => Ok(()),
        Err(e) => Err(format!("{}", e)),
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_wgsl_valid() {
        let source = r#"
@vertex
fn vs_main(@builtin(vertex_index) in_vertex_index: u32) -> @builtin(position) vec4<f32> {
    let x = f32(1 - i32(in_vertex_index)) * 0.5;
    let y = f32(i32(in_vertex_index & 1u) * 2 - 1) * 0.5;
    return vec4<f32>(x, y, 0.0, 1.0);
}
"#;
        assert!(validate_wgsl(source).is_ok());
    }

    #[test]
    fn test_validate_wgsl_invalid() {
        let source = "this is not valid wgsl at all {};";
        assert!(validate_wgsl(source).is_err());
    }

    #[test]
    fn test_validate_wgsl_skips_imports() {
        // Shaders with Bevy preprocessor directives should pass validation
        let source = "#import bevy_pbr::mesh_view_bindings\n@vertex fn vs() {}";
        assert!(validate_wgsl(source).is_ok());
    }

    #[test]
    fn test_shader_registry_register_and_get() {
        let mut registry = ShaderRegistry::default();
        let handle = Handle::weak_from_u128(0x1234);

        // Use an absolute path that exists to test canonicalization
        let path = PathBuf::from(SHADER_DIR);
        if path.exists() {
            registry.register(&path, handle.clone());
            assert!(registry.get(&path).is_some());
        }
    }

    #[test]
    fn test_shader_registry_missing_path() {
        let registry = ShaderRegistry::default();
        assert!(registry.get(Path::new("nonexistent/shader.wgsl")).is_none());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn test_shader_watcher_creation() {
        // ShaderWatcher::new() should succeed when assets/shaders/ exists
        // and return None otherwise (no panic either way).
        let _result = ShaderWatcher::new();
    }
}
