//! PS4 input emulation (stub for Phase 1).
//!
//! Will handle DualShock 4 controller emulation, mapping host
//! gamepad input to PS4 controller state.

/// DualShock 4 button state.
#[derive(Debug, Clone, Default)]
pub struct PadState {
    pub cross: bool,
    pub circle: bool,
    pub square: bool,
    pub triangle: bool,
    pub l1: bool,
    pub r1: bool,
    pub l2_trigger: f32,
    pub r2_trigger: f32,
    pub left_stick_x: f32,
    pub left_stick_y: f32,
    pub right_stick_x: f32,
    pub right_stick_y: f32,
    pub dpad_up: bool,
    pub dpad_down: bool,
    pub dpad_left: bool,
    pub dpad_right: bool,
    pub options: bool,
    pub share: bool,
    pub l3: bool,
    pub r3: bool,
    pub touchpad: bool,
    pub ps_button: bool,
}

/// Controller manager.
pub struct ControllerManager {
    pub pad: PadState,
}

impl ControllerManager {
    pub fn new() -> Self {
        tracing::info!("Controller manager stub initialized (will be implemented in Phase 7)");
        ControllerManager {
            pad: PadState::default(),
        }
    }
}

impl Default for ControllerManager {
    fn default() -> Self {
        Self::new()
    }
}
