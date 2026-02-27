//! Input mapping and action system
//!
//! Abstracts raw input (keyboard, mouse, gamepad) into game actions.
//! Supports configurable key bindings.
//!
//! # Architecture
//!
//! ```text
//! Raw Input (KeyCode, GamepadButton)
//!     ↓
//! InputMap (configurable bindings)
//!     ↓
//! InputAction events
//!     ↓
//! Game systems (movement, abilities)
//! ```

use bevy::prelude::*;
use std::collections::HashMap;

// ============================================================================
// INPUT ACTIONS
// ============================================================================

/// All possible player input actions
///
/// These are abstract actions, decoupled from specific keys/buttons.
/// The InputMap determines which inputs trigger which actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputAction {
    // Movement
    MoveForward,
    MoveBackward,
    MoveLeft,
    MoveRight,
    
    // Vertical movement
    Jump,
    Crouch,
    
    // Modifiers
    Sprint,
    
    // Mode toggles
    ToggleFly,
    ToggleNoclip,
    
    // Camera/UI
    ReleaseCursor,
    #[allow(dead_code)]
    GrabCursor,
}

/// State of an input action
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActionState {
    #[default]
    Released,
    JustPressed,
    Pressed,
    JustReleased,
}

impl ActionState {
    /// Is the action currently active (pressed or just pressed)?
    pub fn is_active(&self) -> bool {
        matches!(self, ActionState::JustPressed | ActionState::Pressed)
    }
    
    /// Was the action just triggered this frame?
    pub fn just_pressed(&self) -> bool {
        matches!(self, ActionState::JustPressed)
    }
    
    /// Was the action just released this frame?
    #[allow(dead_code)]
    pub fn just_released(&self) -> bool {
        matches!(self, ActionState::JustReleased)
    }
}

// ============================================================================
// INPUT BINDING
// ============================================================================

/// A binding that can trigger an action
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InputBinding {
    /// A keyboard key
    Key(KeyCode),
    /// A mouse button
    MouseButton(MouseButton),
    // Future: gamepad support
    // GamepadButton(GamepadButtonType),
    // GamepadAxis(GamepadAxisType, AxisDirection),
}

impl From<KeyCode> for InputBinding {
    fn from(key: KeyCode) -> Self {
        InputBinding::Key(key)
    }
}

impl From<MouseButton> for InputBinding {
    fn from(button: MouseButton) -> Self {
        InputBinding::MouseButton(button)
    }
}

// ============================================================================
// INPUT MAP RESOURCE
// ============================================================================

/// Configurable input mapping
///
/// Maps InputBindings to InputActions. Multiple bindings can trigger
/// the same action (e.g., both W and Up Arrow for MoveForward).
#[derive(Resource)]
pub struct InputMap {
    /// Bindings for each action (action -> list of bindings)
    bindings: HashMap<InputAction, Vec<InputBinding>>,
    /// Reverse lookup (binding -> action) for efficiency
    reverse_map: HashMap<InputBinding, InputAction>,
}

impl Default for InputMap {
    fn default() -> Self {
        let mut map = Self {
            bindings: HashMap::new(),
            reverse_map: HashMap::new(),
        };
        
        // Default bindings (Minecraft-style)
        map.bind(InputAction::MoveForward, KeyCode::KeyW);
        map.bind(InputAction::MoveBackward, KeyCode::KeyS);
        map.bind(InputAction::MoveLeft, KeyCode::KeyA);
        map.bind(InputAction::MoveRight, KeyCode::KeyD);
        
        map.bind(InputAction::Jump, KeyCode::Space);
        map.bind(InputAction::Crouch, KeyCode::ControlLeft);
        map.bind(InputAction::Sprint, KeyCode::ShiftLeft);
        
        map.bind(InputAction::ToggleFly, KeyCode::KeyF);
        map.bind(InputAction::ToggleNoclip, KeyCode::KeyN);
        
        map.bind(InputAction::ReleaseCursor, KeyCode::Escape);
        // Mouse click to grab is handled separately (requires special logic)
        
        map
    }
}

impl InputMap {
    /// Bind an input to an action
    pub fn bind(&mut self, action: InputAction, binding: impl Into<InputBinding>) {
        let binding = binding.into();
        
        // Add to forward map
        self.bindings
            .entry(action)
            .or_default()
            .push(binding.clone());
        
        // Add to reverse map
        self.reverse_map.insert(binding, action);
    }
    
    /// Unbind all inputs from an action
    #[allow(dead_code)]
    pub fn unbind_action(&mut self, action: InputAction) {
        if let Some(bindings) = self.bindings.remove(&action) {
            for binding in bindings {
                self.reverse_map.remove(&binding);
            }
        }
    }
    
    /// Clear all bindings (used by config system to rebuild from scratch)
    pub fn clear(&mut self) {
        self.bindings.clear();
        self.reverse_map.clear();
    }

    /// Unbind a specific input
    #[allow(dead_code)]
    pub fn unbind(&mut self, binding: impl Into<InputBinding>) {
        let binding = binding.into();
        if let Some(action) = self.reverse_map.remove(&binding) {
            if let Some(bindings) = self.bindings.get_mut(&action) {
                bindings.retain(|b| b != &binding);
            }
        }
    }
    
    /// Get the action for a binding (if any)
    pub fn get_action(&self, binding: &InputBinding) -> Option<InputAction> {
        self.reverse_map.get(binding).copied()
    }
    
    /// Get all bindings for an action
    #[allow(dead_code)]
    pub fn get_bindings(&self, action: InputAction) -> &[InputBinding] {
        self.bindings.get(&action).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

// ============================================================================
// ACTION STATE RESOURCE
// ============================================================================

/// Current state of all input actions
///
/// Updated each frame by the input processing system.
#[derive(Resource, Default)]
pub struct ActionStates {
    states: HashMap<InputAction, ActionState>,
}

impl ActionStates {
    /// Get the state of an action
    pub fn get(&self, action: InputAction) -> ActionState {
        self.states.get(&action).copied().unwrap_or_default()
    }
    
    /// Check if an action is currently active (pressed)
    pub fn is_active(&self, action: InputAction) -> bool {
        self.get(action).is_active()
    }
    
    /// Check if an action was just pressed this frame
    pub fn just_pressed(&self, action: InputAction) -> bool {
        self.get(action).just_pressed()
    }
    
    /// Check if an action was just released this frame
    #[allow(dead_code)]
    pub fn just_released(&self, action: InputAction) -> bool {
        self.get(action).just_released()
    }
    
    /// Update the state of an action
    pub fn set(&mut self, action: InputAction, state: ActionState) {
        self.states.insert(action, state);
    }
    
    /// Transition states between frames (JustPressed -> Pressed, JustReleased -> Released)
    pub fn tick(&mut self) {
        for state in self.states.values_mut() {
            *state = match *state {
                ActionState::JustPressed => ActionState::Pressed,
                ActionState::JustReleased => ActionState::Released,
                other => other,
            };
        }
    }
}

// ============================================================================
// INPUT PLUGIN
// ============================================================================

/// Plugin for the input action system
pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InputMap>()
            .init_resource::<ActionStates>()
            .add_systems(PreUpdate, process_input_system);
    }
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// Process raw input into action states
///
/// Runs in PreUpdate so action states are available for all game systems.
/// Skips keyboard input when egui has focus (e.g., typing in text fields).
fn process_input_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    input_map: Res<InputMap>,
    mut action_states: ResMut<ActionStates>,
    mut egui_contexts: bevy_egui::EguiContexts,
) {
    // Transition existing states
    action_states.tick();
    
    // Check if egui wants keyboard input (user is typing in a text field)
    let ctx = egui_contexts.ctx_mut();
    let egui_wants_keyboard = ctx.wants_keyboard_input();
    
    // When egui takes keyboard focus, clear all pressed keyboard states
    // to prevent "stuck keys" when user releases while typing
    if egui_wants_keyboard {
        // Release all currently pressed keyboard-bound actions
        for key in keyboard.get_pressed() {
            if let Some(action) = input_map.get_action(&InputBinding::Key(*key)) {
                if action_states.is_active(action) {
                    action_states.set(action, ActionState::JustReleased);
                }
            }
        }
        // Don't process any new keyboard input while egui has focus
        // (but still process mouse below)
    } else {
        // Normal keyboard processing when egui doesn't want input
        for key in keyboard.get_just_pressed() {
            if let Some(action) = input_map.get_action(&InputBinding::Key(*key)) {
                action_states.set(action, ActionState::JustPressed);
            }
        }
        
        for key in keyboard.get_just_released() {
            if let Some(action) = input_map.get_action(&InputBinding::Key(*key)) {
                action_states.set(action, ActionState::JustReleased);
            }
        }
    }
    
    // Process mouse buttons (always, even when typing)
    for button in mouse_buttons.get_just_pressed() {
        if let Some(action) = input_map.get_action(&InputBinding::MouseButton(*button)) {
            action_states.set(action, ActionState::JustPressed);
        }
    }
    
    for button in mouse_buttons.get_just_released() {
        if let Some(action) = input_map.get_action(&InputBinding::MouseButton(*button)) {
            action_states.set(action, ActionState::JustReleased);
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_action_state_active() {
        assert!(!ActionState::Released.is_active());
        assert!(ActionState::JustPressed.is_active());
        assert!(ActionState::Pressed.is_active());
        assert!(!ActionState::JustReleased.is_active());
    }
    
    #[test]
    fn test_input_map_default_bindings() {
        let map = InputMap::default();
        
        assert_eq!(
            map.get_action(&InputBinding::Key(KeyCode::KeyW)),
            Some(InputAction::MoveForward)
        );
        assert_eq!(
            map.get_action(&InputBinding::Key(KeyCode::Space)),
            Some(InputAction::Jump)
        );
        assert_eq!(
            map.get_action(&InputBinding::Key(KeyCode::KeyF)),
            Some(InputAction::ToggleFly)
        );
    }
    
    #[test]
    fn test_input_map_custom_binding() {
        let mut map = InputMap::default();
        
        // Add arrow key bindings
        map.bind(InputAction::MoveForward, KeyCode::ArrowUp);
        
        assert_eq!(
            map.get_action(&InputBinding::Key(KeyCode::ArrowUp)),
            Some(InputAction::MoveForward)
        );
        // Original binding still works
        assert_eq!(
            map.get_action(&InputBinding::Key(KeyCode::KeyW)),
            Some(InputAction::MoveForward)
        );
    }
    
    #[test]
    fn test_action_states_tick() {
        let mut states = ActionStates::default();
        
        states.set(InputAction::Jump, ActionState::JustPressed);
        assert!(states.just_pressed(InputAction::Jump));
        
        states.tick();
        assert!(!states.just_pressed(InputAction::Jump));
        assert!(states.is_active(InputAction::Jump));
        assert_eq!(states.get(InputAction::Jump), ActionState::Pressed);
    }
}
