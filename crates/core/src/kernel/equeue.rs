//! PS4 Event Queue (EQueue) HLE.
//!
//! The PS4 uses event queues (similar to BSD kqueue) for asynchronous
//! event notification. Games use these for VSync, timer events, and
//! general-purpose I/O multiplexing.

use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

/// Event filter types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i16)]
pub enum EventFilter {
    /// User-defined event.
    User = -11,
    /// File descriptor event.
    Read = -1,
    /// Write event.
    Write = -2,
    /// Timer event.
    Timer = -7,
    /// Graphics core event.
    GraphicsCore = -13,
    /// Display event (VSync).
    Display = -14,
    /// Hid (input) event.
    Hid = -15,
}

/// An event that can be triggered or waited on.
#[derive(Debug, Clone)]
pub struct Event {
    /// Event identifier.
    pub ident: u64,
    /// Event filter type.
    pub filter: i16,
    /// Filter flags.
    pub flags: u16,
    /// Filter-specific flags.
    pub fflags: u32,
    /// Data associated with the event.
    pub data: i64,
    /// User data pointer.
    pub udata: u64,
}

/// An event queue (equivalent to a kqueue).
pub struct EventQueue {
    /// Queue name.
    pub name: String,
    /// Pending triggered events.
    events: Mutex<VecDeque<Event>>,
}

impl EventQueue {
    pub fn new(name: &str) -> Self {
        tracing::debug!(name, "Event queue created");
        EventQueue {
            name: name.to_string(),
            events: Mutex::new(VecDeque::new()),
        }
    }

    /// Add/register an event to watch.
    pub fn add_event(&self, event: Event) {
        tracing::trace!(
            name = %self.name,
            ident = event.ident,
            filter = event.filter,
            "Event registered"
        );
        // For now, just store it. Actual triggering will come later.
    }

    /// Trigger an event (push it to the pending queue).
    pub fn trigger_event(&self, event: Event) {
        let mut events = self.events.lock();
        events.push_back(event);
    }

    /// Wait for events with timeout.
    pub fn wait(&self, max_events: usize, timeout: Option<Duration>) -> Vec<Event> {
        // Simple implementation: check if any events are pending
        let mut events = self.events.lock();

        if events.is_empty() {
            if let Some(dur) = timeout {
                // Sleep for the timeout duration (simplified)
                drop(events);
                std::thread::sleep(dur.min(Duration::from_millis(16)));
                events = self.events.lock();
            }
        }

        let count = events.len().min(max_events);
        events.drain(..count).collect()
    }

    /// Number of pending events.
    pub fn pending_count(&self) -> usize {
        self.events.lock().len()
    }
}

/// Manages all event queues.
pub struct EventQueueManager {
    queues: Mutex<Vec<Arc<EventQueue>>>,
}

impl EventQueueManager {
    pub fn new() -> Self {
        EventQueueManager {
            queues: Mutex::new(Vec::new()),
        }
    }

    /// Create a new event queue.
    pub fn create_equeue(&self, name: &str) -> Arc<EventQueue> {
        let eq = Arc::new(EventQueue::new(name));
        self.queues.lock().push(eq.clone());
        eq
    }

    /// Get the number of active event queues.
    pub fn queue_count(&self) -> usize {
        self.queues.lock().len()
    }
}

impl Default for EventQueueManager {
    fn default() -> Self {
        Self::new()
    }
}
