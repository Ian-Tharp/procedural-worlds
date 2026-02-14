//! Chunk lifecycle events — pub/sub for chunk state changes
//!
//! Provides Bevy [`Event`]s emitted when chunks transition through lifecycle
//! stages (queued, loaded, unloaded, compressed). Other systems (audio, physics,
//! entities) subscribe via standard `EventReader<ChunkLifecycleEvent>` to react
//! to chunk changes without coupling to the chunk management internals.
//!
//! # Event Flow
//!
//! ```text
//! chunk_streaming_system  ──→  ChunkQueued(pos)
//! poll_pending_chunks     ──→  ChunkLoaded(pos)
//! chunk_unloading_system  ──→  ChunkUnloaded(pos)
//! save_chunk_fmt (cbin)   ──→  ChunkCompressed(pos)
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! fn my_listener(mut events: EventReader<ChunkLifecycleEvent>) {
//!     for event in events.read() {
//!         match event {
//!             ChunkLifecycleEvent::Loaded(pos) => { /* spawn entities */ }
//!             ChunkLifecycleEvent::Unloaded(pos) => { /* despawn entities */ }
//!             _ => {}
//!         }
//!     }
//! }
//! ```

use bevy::prelude::*;

/// Lifecycle events emitted by the chunk management systems.
///
/// Consumers subscribe via `EventReader<ChunkLifecycleEvent>` in any Bevy
/// system. Events are buffered for two frames (Bevy default), so readers
/// don't need to run in the same frame as the emitter.
#[derive(Event, Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChunkLifecycleEvent {
    /// A chunk generation/load task was queued (background task spawned).
    Queued(IVec3),
    /// A chunk finished loading and its data is now available as a `Chunk` component.
    Loaded(IVec3),
    /// A chunk was unloaded and its entity despawned.
    Unloaded(IVec3),
    /// A chunk was saved in compressed format (palette-based `.cbin`).
    Compressed(IVec3),
}

impl ChunkLifecycleEvent {
    /// Returns the chunk coordinate associated with this event.
    pub fn position(&self) -> IVec3 {
        match self {
            Self::Queued(pos) | Self::Loaded(pos) | Self::Unloaded(pos) | Self::Compressed(pos) => {
                *pos
            }
        }
    }
}

impl std::fmt::Display for ChunkLifecycleEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Queued(pos) => write!(f, "ChunkQueued({}, {}, {})", pos.x, pos.y, pos.z),
            Self::Loaded(pos) => write!(f, "ChunkLoaded({}, {}, {})", pos.x, pos.y, pos.z),
            Self::Unloaded(pos) => write!(f, "ChunkUnloaded({}, {}, {})", pos.x, pos.y, pos.z),
            Self::Compressed(pos) => write!(f, "ChunkCompressed({}, {}, {})", pos.x, pos.y, pos.z),
        }
    }
}

/// Example subscriber: logs chunk lifecycle events for debugging.
///
/// Add this system to your app to verify events are flowing correctly.
/// In production, replace with real subsystem logic (audio zones, entity
/// spawning, physics boundary updates, etc.).
pub fn chunk_event_logger(mut events: EventReader<ChunkLifecycleEvent>) {
    for event in events.read() {
        debug!("{}", event);
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::app::App;

    /// Helper: create a minimal Bevy app with chunk events registered.
    fn test_app() -> App {
        let mut app = App::new();
        app.add_event::<ChunkLifecycleEvent>();
        app
    }

    #[test]
    fn test_event_position() {
        let pos = IVec3::new(1, 2, 3);
        assert_eq!(ChunkLifecycleEvent::Queued(pos).position(), pos);
        assert_eq!(ChunkLifecycleEvent::Loaded(pos).position(), pos);
        assert_eq!(ChunkLifecycleEvent::Unloaded(pos).position(), pos);
        assert_eq!(ChunkLifecycleEvent::Compressed(pos).position(), pos);
    }

    #[test]
    fn test_event_display() {
        let pos = IVec3::new(5, -1, 3);
        assert_eq!(
            format!("{}", ChunkLifecycleEvent::Loaded(pos)),
            "ChunkLoaded(5, -1, 3)"
        );
        assert_eq!(
            format!("{}", ChunkLifecycleEvent::Unloaded(pos)),
            "ChunkUnloaded(5, -1, 3)"
        );
    }

    #[test]
    fn test_event_equality() {
        let a = ChunkLifecycleEvent::Loaded(IVec3::ZERO);
        let b = ChunkLifecycleEvent::Loaded(IVec3::ZERO);
        let c = ChunkLifecycleEvent::Unloaded(IVec3::ZERO);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_event_emission_and_reading() {
        let mut app = test_app();

        // Add a system that emits events
        app.add_systems(Update, |mut writer: EventWriter<ChunkLifecycleEvent>| {
            writer.send(ChunkLifecycleEvent::Loaded(IVec3::new(1, 0, 0)));
            writer.send(ChunkLifecycleEvent::Unloaded(IVec3::new(2, 0, 0)));
        });

        // Run one frame to emit
        app.update();

        // Read events from the world
        let events = app.world().resource::<Events<ChunkLifecycleEvent>>();
        let mut reader = events.get_cursor();
        let collected: Vec<_> = reader.read(events).cloned().collect();

        assert_eq!(collected.len(), 2);
        assert_eq!(
            collected[0],
            ChunkLifecycleEvent::Loaded(IVec3::new(1, 0, 0))
        );
        assert_eq!(
            collected[1],
            ChunkLifecycleEvent::Unloaded(IVec3::new(2, 0, 0))
        );
    }

    #[test]
    fn test_multiple_subscribers_independent() {
        let mut app = test_app();

        // Emitter
        app.add_systems(Update, |mut writer: EventWriter<ChunkLifecycleEvent>| {
            writer.send(ChunkLifecycleEvent::Queued(IVec3::new(3, 3, 3)));
        });

        app.update();

        // Two independent readers should both see the event
        let events = app.world().resource::<Events<ChunkLifecycleEvent>>();

        let mut reader_a = events.get_cursor();
        let mut reader_b = events.get_cursor();

        let a: Vec<_> = reader_a.read(events).collect();
        let b: Vec<_> = reader_b.read(events).collect();

        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_eq!(a[0], b[0]);
    }

    #[test]
    fn test_logger_does_not_panic() {
        let mut app = test_app();
        app.add_systems(Update, chunk_event_logger);

        // Emit some events and run — should not panic
        app.world_mut()
            .resource_mut::<Events<ChunkLifecycleEvent>>()
            .send(ChunkLifecycleEvent::Compressed(IVec3::new(10, 20, 30)));

        app.update();
    }
}
