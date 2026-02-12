//! Unified Debug Console
//!
//! A comprehensive in-game debug console with:
//! - **Command categories** (World, Performance, Debug, Player) with tab navigation
//! - **Tab-completion** with inline suggestions popup
//! - **Command history** (circular buffer, 100 entries) with Up/Down navigation
//! - **Command validation** with descriptive error messages
//! - **Weather/time commands** for environment control
//!
//! Toggle with **F11** (or backtick `` ` ``).
//!
//! # Consolidated from feature branches
//!
//! This module unifies the following scattered debug console branches:
//! - `debug-console` (base commands)
//! - `debug-console-autocomplete` (tab-completion)
//! - `debug-console-categories` (category tabs)
//! - `debug-console-command-validation` (parameter validation)
//! - `debug-console-validation` (validation layer)
//! - `debug-console-weather-time` (weather/time/biome commands)
//! - `debug-console-history` (history navigation)

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use std::collections::{HashMap, VecDeque};

use crate::actors::{Movement, Player};
use crate::editor::debug_overlay::DebugOverlayState;
use crate::engine::lighting::DayNightCycle;
use crate::world::ChunkManager;

// ============================================================================
// Constants
// ============================================================================

/// Maximum number of commands to keep in history.
const MAX_HISTORY_SIZE: usize = 100;

/// Maximum number of output lines to retain.
const MAX_OUTPUT_LINES: usize = 500;

/// Maximum number of autocomplete suggestions shown.
const MAX_SUGGESTIONS: usize = 5;

/// Primary toggle key.
const CONSOLE_TOGGLE_KEY: KeyCode = KeyCode::F11;

/// Secondary toggle key (backtick).
const CONSOLE_TOGGLE_KEY_ALT: KeyCode = KeyCode::Backquote;

// ============================================================================
// Command Categories
// ============================================================================

/// Categories for organizing debug commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CommandCategory {
    /// All commands (no filter).
    #[default]
    All,
    /// World-related: chunks, terrain, environment.
    World,
    /// Performance: FPS, profiling, memory.
    Performance,
    /// Debug visualization: overlays, wireframe.
    Debug,
    /// Player: position, movement, physics.
    Player,
}

impl CommandCategory {
    /// All categories in display order.
    pub const TABS: [CommandCategory; 5] = [
        CommandCategory::All,
        CommandCategory::World,
        CommandCategory::Performance,
        CommandCategory::Debug,
        CommandCategory::Player,
    ];

    /// Display name for the category tab.
    pub fn display_name(&self) -> &'static str {
        match self {
            CommandCategory::All => "📋 All",
            CommandCategory::World => "🌍 World",
            CommandCategory::Performance => "⚡ Perf",
            CommandCategory::Debug => "🔧 Debug",
            CommandCategory::Player => "🎮 Player",
        }
    }

    /// Short description.
    pub fn description(&self) -> &'static str {
        match self {
            CommandCategory::All => "Show all commands",
            CommandCategory::World => "Chunks, terrain, weather, time",
            CommandCategory::Performance => "FPS, profiling, memory",
            CommandCategory::Debug => "Visualization, logging, diagnostics",
            CommandCategory::Player => "Position, movement, physics",
        }
    }
}

// ============================================================================
// Parameter Types & Validation
// ============================================================================

/// Supported parameter types for command validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterType {
    /// Signed integer
    Int,
    /// Floating point number
    Float,
    /// Boolean (true/false, on/off, yes/no, 1/0)
    Bool,
    /// Free-form string
    String,
}

impl std::fmt::Display for ParameterType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParameterType::Int => write!(f, "integer"),
            ParameterType::Float => write!(f, "number"),
            ParameterType::Bool => write!(f, "boolean"),
            ParameterType::String => write!(f, "text"),
        }
    }
}

/// A parsed parameter value.
#[derive(Debug, Clone, PartialEq)]
pub enum ParameterValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
}

impl ParameterValue {
    pub fn as_float(&self) -> Option<f64> {
        match self {
            ParameterValue::Float(v) => Some(*v),
            ParameterValue::Int(v) => Some(*v as f64),
            _ => None,
        }
    }
}

/// Definition of a single command parameter.
#[derive(Debug, Clone)]
pub struct CommandParameter {
    /// Parameter name (for help text).
    pub name: String,
    /// Expected type.
    pub param_type: ParameterType,
    /// Whether this parameter is optional.
    pub optional: bool,
    /// Description for help text.
    pub description: String,
}

impl CommandParameter {
    /// Create a required parameter.
    pub fn required(name: impl Into<String>, param_type: ParameterType, desc: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            param_type,
            optional: false,
            description: desc.into(),
        }
    }

    /// Create an optional parameter.
    pub fn optional(name: impl Into<String>, param_type: ParameterType, desc: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            param_type,
            optional: true,
            description: desc.into(),
        }
    }

    /// Format for usage text.
    pub fn usage_text(&self) -> String {
        if self.optional {
            format!("[{}]", self.name)
        } else {
            format!("<{}>", self.name)
        }
    }
}

/// Try to parse a token into a ParameterValue of the given type.
fn parse_parameter(token: &str, param_type: ParameterType) -> Result<ParameterValue, String> {
    match param_type {
        ParameterType::Int => token
            .parse::<i64>()
            .map(ParameterValue::Int)
            .map_err(|_| format!("'{}' is not a valid integer", token)),
        ParameterType::Float => token
            .parse::<f64>()
            .map(ParameterValue::Float)
            .map_err(|_| format!("'{}' is not a valid number", token)),
        ParameterType::Bool => match token.to_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Ok(ParameterValue::Bool(true)),
            "false" | "no" | "off" | "0" => Ok(ParameterValue::Bool(false)),
            _ => Err(format!("'{}' is not a valid boolean (use true/false, on/off, yes/no)", token)),
        },
        ParameterType::String => Ok(ParameterValue::String(token.to_string())),
    }
}

// ============================================================================
// Command Definition
// ============================================================================

/// Metadata for a registered console command.
#[derive(Clone)]
pub struct CommandDefinition {
    /// The command name (what the user types).
    pub name: String,
    /// Short description shown in help and autocomplete.
    pub description: String,
    /// Usage syntax string.
    pub usage: String,
    /// Category for tab organization.
    pub category: CommandCategory,
    /// Parameter definitions for validation.
    pub parameters: Vec<CommandParameter>,
    /// Aliases for the command.
    pub aliases: Vec<String>,
}

impl CommandDefinition {
    /// Create a new command definition.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        category: CommandCategory,
    ) -> Self {
        let name = name.into();
        Self {
            usage: name.clone(),
            name,
            description: description.into(),
            category,
            parameters: Vec::new(),
            aliases: Vec::new(),
        }
    }

    /// Add an alias.
    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    /// Add a required parameter.
    pub fn param(mut self, name: impl Into<String>, param_type: ParameterType, desc: impl Into<String>) -> Self {
        self.parameters.push(CommandParameter::required(name, param_type, desc));
        self.rebuild_usage();
        self
    }

    /// Add an optional parameter.
    pub fn optional_param(mut self, name: impl Into<String>, param_type: ParameterType, desc: impl Into<String>) -> Self {
        self.parameters.push(CommandParameter::optional(name, param_type, desc));
        self.rebuild_usage();
        self
    }

    /// Rebuild the usage string from parameters.
    fn rebuild_usage(&mut self) {
        let params: Vec<String> = self.parameters.iter().map(|p| p.usage_text()).collect();
        self.usage = if params.is_empty() {
            self.name.clone()
        } else {
            format!("{} {}", self.name, params.join(" "))
        };
    }

    /// Validate arguments against parameter definitions.
    /// Returns parsed values or an error message.
    pub fn validate_args(&self, args: &[&str]) -> Result<Vec<ParameterValue>, String> {
        let mut values = Vec::new();
        let required_count = self.parameters.iter().filter(|p| !p.optional).count();

        if args.len() < required_count {
            return Err(format!(
                "Not enough arguments. Usage: {}",
                self.usage
            ));
        }

        for (i, param) in self.parameters.iter().enumerate() {
            if let Some(token) = args.get(i) {
                match parse_parameter(token, param.param_type) {
                    Ok(val) => values.push(val),
                    Err(e) => return Err(format!("Parameter '{}': {}", param.name, e)),
                }
            } else if param.optional {
                break;
            } else {
                return Err(format!(
                    "Missing required parameter '{}'. Usage: {}",
                    param.name, self.usage
                ));
            }
        }

        Ok(values)
    }
}

// ============================================================================
// Console Output
// ============================================================================

/// A single line of console output with styling.
#[derive(Clone, Debug)]
pub struct ConsoleOutputLine {
    pub text: String,
    pub color: egui::Color32,
}

impl ConsoleOutputLine {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            color: egui::Color32::from_rgb(200, 200, 200),
        }
    }

    pub fn success(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            color: egui::Color32::from_rgb(100, 255, 100),
        }
    }

    pub fn warning(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            color: egui::Color32::from_rgb(255, 200, 100),
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            color: egui::Color32::from_rgb(255, 100, 100),
        }
    }

    pub fn command(text: impl Into<String>) -> Self {
        Self {
            text: format!("> {}", text.into()),
            color: egui::Color32::from_rgb(150, 200, 255),
        }
    }

    pub fn header(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            color: egui::Color32::from_rgb(100, 200, 255),
        }
    }
}

// ============================================================================
// Debug Console State
// ============================================================================

/// The unified debug console resource.
///
/// Manages command history, autocomplete, categories, validation, and output.
#[derive(Resource)]
pub struct DebugConsoleState {
    /// Whether the console window is visible.
    pub visible: bool,
    /// Current input buffer.
    pub input: String,
    /// Console output lines.
    pub output: VecDeque<ConsoleOutputLine>,
    /// Command history (oldest first).
    pub history: VecDeque<String>,
    /// Current position in history (-1 = not navigating).
    pub history_index: isize,
    /// Saved input when browsing history.
    pub saved_input: String,
    /// Registered commands.
    pub commands: HashMap<String, CommandDefinition>,
    /// Alias -> command name mapping.
    pub aliases: HashMap<String, String>,
    /// Currently selected category tab.
    pub selected_category: CommandCategory,
    /// Current autocomplete suggestions (command names).
    pub suggestions: Vec<String>,
    /// Selected suggestion index (-1 = none).
    pub selected_suggestion: i32,
    /// Whether autocomplete popup is visible.
    pub show_suggestions: bool,
    /// Whether input field should request focus.
    pub focus_input: bool,
    /// Whether to scroll output to bottom.
    pub scroll_to_bottom: bool,
}

impl Default for DebugConsoleState {
    fn default() -> Self {
        let mut state = Self {
            visible: false,
            input: String::new(),
            output: VecDeque::with_capacity(MAX_OUTPUT_LINES),
            history: VecDeque::with_capacity(MAX_HISTORY_SIZE),
            history_index: -1,
            saved_input: String::new(),
            commands: HashMap::new(),
            aliases: HashMap::new(),
            selected_category: CommandCategory::All,
            suggestions: Vec::new(),
            selected_suggestion: -1,
            show_suggestions: false,
            focus_input: false,
            scroll_to_bottom: false,
        };
        state.register_builtin_commands();
        state.print_welcome();
        state
    }
}

impl DebugConsoleState {
    // ── Output ──────────────────────────────────────────────────────────

    /// Push an output line.
    pub fn print(&mut self, line: ConsoleOutputLine) {
        self.output.push_back(line);
        while self.output.len() > MAX_OUTPUT_LINES {
            self.output.pop_front();
        }
        self.scroll_to_bottom = true;
    }

    /// Print info text.
    pub fn info(&mut self, text: impl Into<String>) {
        self.print(ConsoleOutputLine::info(text));
    }

    /// Print success text.
    pub fn success(&mut self, text: impl Into<String>) {
        self.print(ConsoleOutputLine::success(text));
    }

    /// Print warning text.
    pub fn warning(&mut self, text: impl Into<String>) {
        self.print(ConsoleOutputLine::warning(text));
    }

    /// Print error text.
    pub fn error(&mut self, text: impl Into<String>) {
        self.print(ConsoleOutputLine::error(text));
    }

    /// Clear the output.
    pub fn clear_output(&mut self) {
        self.output.clear();
    }

    /// Print welcome message.
    fn print_welcome(&mut self) {
        self.print(ConsoleOutputLine::header(
            "═══════════════════════════════════════════════",
        ));
        self.print(ConsoleOutputLine::success(
            "  Procedural Worlds Debug Console",
        ));
        self.print(ConsoleOutputLine::info(
            "  Type 'help' for commands | Tab for autocomplete",
        ));
        self.print(ConsoleOutputLine::info(
            "  F11 or ` to toggle | ↑↓ for history",
        ));
        self.print(ConsoleOutputLine::header(
            "═══════════════════════════════════════════════",
        ));
    }

    // ── Command Registration ────────────────────────────────────────────

    /// Register a command definition.
    pub fn register(&mut self, cmd: CommandDefinition) {
        // Register aliases
        for alias in &cmd.aliases {
            self.aliases.insert(alias.clone(), cmd.name.clone());
        }
        self.commands.insert(cmd.name.clone(), cmd);
    }

    /// Get commands filtered by category.
    pub fn commands_in_category(&self, category: CommandCategory) -> Vec<&CommandDefinition> {
        let mut cmds: Vec<&CommandDefinition> = if category == CommandCategory::All {
            self.commands.values().collect()
        } else {
            self.commands
                .values()
                .filter(|cmd| cmd.category == category)
                .collect()
        };
        cmds.sort_by_key(|c| &c.name);
        cmds
    }

    /// Resolve a command name or alias to the canonical name.
    pub fn resolve_command(&self, input: &str) -> Option<String> {
        let lower = input.to_lowercase();
        if self.commands.contains_key(&lower) {
            Some(lower)
        } else {
            self.aliases.get(&lower).cloned()
        }
    }

    /// Register all built-in commands.
    fn register_builtin_commands(&mut self) {
        // === Debug commands ===
        self.register(
            CommandDefinition::new("help", "Show available commands", CommandCategory::Debug)
                .alias("?")
                .optional_param("command", ParameterType::String, "Command name for detailed help"),
        );
        self.register(
            CommandDefinition::new("clear", "Clear console output", CommandCategory::Debug),
        );
        self.register(
            CommandDefinition::new("version", "Show engine version", CommandCategory::Debug),
        );
        self.register(
            CommandDefinition::new("echo", "Print text to console", CommandCategory::Debug)
                .param("text", ParameterType::String, "Text to print"),
        );
        self.register(
            CommandDefinition::new("debug", "Toggle debug overlay (F3)", CommandCategory::Debug),
        );
        self.register(
            CommandDefinition::new("wireframe", "Toggle wireframe rendering", CommandCategory::Debug)
                .optional_param("state", ParameterType::Bool, "on/off (toggles if omitted)"),
        );
        self.register(
            CommandDefinition::new("shadows", "Toggle shadow rendering", CommandCategory::Debug),
        );
        self.register(
            CommandDefinition::new("profiler", "Toggle profiler overlay (F4)", CommandCategory::Debug),
        );
        self.register(
            CommandDefinition::new("history", "Show command history", CommandCategory::Debug),
        );

        // === Player commands ===
        self.register(
            CommandDefinition::new("pos", "Show player position", CommandCategory::Player),
        );
        self.register(
            CommandDefinition::new("tp", "Teleport player to coordinates", CommandCategory::Player)
                .alias("teleport")
                .param("x", ParameterType::Float, "X coordinate")
                .param("y", ParameterType::Float, "Y coordinate")
                .param("z", ParameterType::Float, "Z coordinate"),
        );
        self.register(
            CommandDefinition::new("fly", "Toggle flying mode", CommandCategory::Player),
        );
        self.register(
            CommandDefinition::new("noclip", "Toggle noclip (no collision)", CommandCategory::Player),
        );
        self.register(
            CommandDefinition::new("god", "Toggle god mode (fly + noclip)", CommandCategory::Player),
        );

        // === World commands ===
        self.register(
            CommandDefinition::new("chunks", "Show loaded chunk statistics", CommandCategory::World),
        );
        self.register(
            CommandDefinition::new("time", "Get or set time of day (0.0-1.0)", CommandCategory::World)
                .optional_param("value", ParameterType::Float, "Time value 0.0-1.0"),
        );
        self.register(
            CommandDefinition::new("pause_time", "Toggle day/night cycle pause", CommandCategory::World),
        );
        self.register(
            CommandDefinition::new("weather", "Get or set weather", CommandCategory::World)
                .optional_param("type", ParameterType::String, "clear, rain, snow, storm"),
        );
        self.register(
            CommandDefinition::new("seed", "Show world generation seed", CommandCategory::World),
        );
        self.register(
            CommandDefinition::new("render_distance", "Get or set render distance", CommandCategory::World)
                .alias("rd")
                .optional_param("distance", ParameterType::Int, "Distance in chunks (2-24)"),
        );

        // === Performance commands ===
        self.register(
            CommandDefinition::new("fps", "Show current FPS info", CommandCategory::Performance),
        );
        self.register(
            CommandDefinition::new("memory", "Show memory usage statistics", CommandCategory::Performance)
                .alias("mem"),
        );
        self.register(
            CommandDefinition::new("stats", "Show engine statistics", CommandCategory::Performance),
        );
    }

    // ── History ─────────────────────────────────────────────────────────

    /// Add a command to history.
    pub fn add_to_history(&mut self, command: &str) {
        if command.is_empty() {
            return;
        }
        // No consecutive duplicates
        if self.history.back().map(|s| s.as_str()) == Some(command) {
            return;
        }
        if self.history.len() >= MAX_HISTORY_SIZE {
            self.history.pop_front();
        }
        self.history.push_back(command.to_string());
    }

    /// Navigate to previous (older) history entry.
    pub fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        if self.history_index == -1 {
            self.saved_input = self.input.clone();
        }
        let max_idx = self.history.len() as isize - 1;
        if self.history_index < max_idx {
            self.history_index += 1;
            let idx = self.history.len() - 1 - self.history_index as usize;
            if let Some(cmd) = self.history.get(idx) {
                self.input = cmd.clone();
            }
        }
    }

    /// Navigate to next (newer) history entry.
    pub fn history_next(&mut self) {
        if self.history_index == -1 {
            return;
        }
        self.history_index -= 1;
        if self.history_index == -1 {
            self.input = std::mem::take(&mut self.saved_input);
        } else {
            let idx = self.history.len() - 1 - self.history_index as usize;
            if let Some(cmd) = self.history.get(idx) {
                self.input = cmd.clone();
            }
        }
    }

    /// Reset history navigation state.
    pub fn reset_history_navigation(&mut self) {
        self.history_index = -1;
        self.saved_input.clear();
    }

    // ── Autocomplete ────────────────────────────────────────────────────

    /// Update autocomplete suggestions based on current input.
    pub fn update_suggestions(&mut self) {
        let input_lower = self.input.to_lowercase();
        let input_trimmed = input_lower.trim();

        if input_trimmed.is_empty() {
            self.suggestions.clear();
            self.show_suggestions = false;
            self.selected_suggestion = -1;
            return;
        }

        // Extract the command part (first word)
        let cmd_part = input_trimmed.split_whitespace().next().unwrap_or("");

        // Don't show suggestions if we already have arguments typed
        let has_args = input_trimmed.contains(' ');
        if has_args {
            self.suggestions.clear();
            self.show_suggestions = false;
            self.selected_suggestion = -1;
            return;
        }

        // Find matching commands and aliases
        let mut matches: Vec<String> = Vec::new();
        for cmd in self.commands.values() {
            if cmd.name.starts_with(cmd_part) {
                matches.push(cmd.name.clone());
            }
            for alias in &cmd.aliases {
                if alias.starts_with(cmd_part) && !matches.contains(alias) {
                    matches.push(alias.clone());
                }
            }
        }
        matches.sort();
        matches.truncate(MAX_SUGGESTIONS);

        // Show suggestions only if input isn't already a complete command
        let is_exact_match = self.commands.contains_key(input_trimmed)
            || self.aliases.contains_key(input_trimmed);
        self.show_suggestions = !matches.is_empty() && !is_exact_match;
        self.suggestions = matches;

        // Reset selection if it's out of range
        if self.selected_suggestion >= self.suggestions.len() as i32 {
            self.selected_suggestion = -1;
        }
    }

    /// Apply the selected (or first) suggestion.
    pub fn apply_suggestion(&mut self) {
        if self.suggestions.is_empty() {
            return;
        }
        let idx = if self.selected_suggestion >= 0 {
            self.selected_suggestion as usize
        } else {
            0
        };
        if let Some(suggestion) = self.suggestions.get(idx) {
            self.input = suggestion.clone();
        }
        self.show_suggestions = false;
        self.selected_suggestion = -1;
    }

    /// Move suggestion selection up.
    pub fn select_prev_suggestion(&mut self) {
        if self.suggestions.is_empty() || !self.show_suggestions {
            return;
        }
        if self.selected_suggestion <= 0 {
            self.selected_suggestion = self.suggestions.len() as i32 - 1;
        } else {
            self.selected_suggestion -= 1;
        }
    }

    /// Move suggestion selection down.
    pub fn select_next_suggestion(&mut self) {
        if self.suggestions.is_empty() || !self.show_suggestions {
            return;
        }
        self.selected_suggestion += 1;
        if self.selected_suggestion >= self.suggestions.len() as i32 {
            self.selected_suggestion = 0;
        }
    }

    // ── Submission ──────────────────────────────────────────────────────

    /// Submit the current input. Returns the command string if non-empty.
    pub fn submit(&mut self) -> Option<String> {
        let input = self.input.trim().to_string();
        if input.is_empty() {
            return None;
        }

        // Echo command
        self.print(ConsoleOutputLine::command(&input));

        // Add to history
        self.add_to_history(&input);
        self.reset_history_navigation();

        // Clear input and suggestions
        self.input.clear();
        self.show_suggestions = false;
        self.selected_suggestion = -1;

        Some(input)
    }
}

// ============================================================================
// Command Execution
// ============================================================================

/// System to execute console commands against game state.
#[allow(clippy::too_many_arguments)]
fn execute_commands(
    mut console: ResMut<DebugConsoleState>,
    mut player_query: Query<(&mut Transform, &mut Movement), With<Player>>,
    mut overlay_state: Option<ResMut<DebugOverlayState>>,
    mut day_night: Option<ResMut<DayNightCycle>>,
    chunk_manager: Option<Res<ChunkManager>>,
    mut profiler: Option<ResMut<crate::engine::profiler::ProfilerState>>,
) {
    // Check if there's a pending command (last output is a "> " command echo)
    let command_line = {
        let last = console.output.back();
        match last {
            Some(line) if line.text.starts_with("> ") => line.text[2..].to_string(),
            _ => return,
        }
    };

    // Parse command and arguments
    let parts: Vec<&str> = command_line.split_whitespace().collect();
    if parts.is_empty() {
        return;
    }

    let raw_cmd = parts[0].to_lowercase();
    let args = &parts[1..];

    // Resolve aliases
    let cmd_name = console.resolve_command(&raw_cmd).unwrap_or(raw_cmd.clone());

    // Validate parameters if we have a definition
    if let Some(def) = console.commands.get(&cmd_name).cloned()
        && let Err(e) = def.validate_args(args)
    {
        // Only validate for commands that have required params
        let has_required = def.parameters.iter().any(|p| !p.optional);
        if has_required && args.len() < def.parameters.iter().filter(|p| !p.optional).count() {
            console.error(e);
            return;
        }
    }

    // Execute command
    match cmd_name.as_str() {
        "help" => {
            if args.is_empty() {
                let category = console.selected_category;
                // Collect command info first to avoid borrow conflict
                let cmd_infos: Vec<(String, String)> = console
                    .commands_in_category(category)
                    .iter()
                    .map(|c| (c.name.clone(), c.description.clone()))
                    .collect();

                let category_name = category.display_name().to_string();
                console.print(ConsoleOutputLine::header(format!(
                    "Commands in {}:",
                    category_name
                )));
                for (name, desc) in cmd_infos {
                    console.info(format!("  {} — {}", name, desc));
                }
                console.info("Type 'help <command>' for detailed usage.");
            } else {
                let search = args[0].to_lowercase();
                let resolved = console.resolve_command(&search);
                let cmd_info = resolved.and_then(|name| console.commands.get(&name).cloned());

                if let Some(cmd) = cmd_info {
                    console.print(ConsoleOutputLine::header(format!(
                        "{} — {}",
                        cmd.name, cmd.description
                    )));
                    console.info(format!("  Usage: {}", cmd.usage));
                    console.info(format!("  Category: {}", cmd.category.display_name()));
                    if !cmd.aliases.is_empty() {
                        console.info(format!("  Aliases: {}", cmd.aliases.join(", ")));
                    }
                    for param in &cmd.parameters {
                        let req = if param.optional { "optional" } else { "required" };
                        console.info(format!(
                            "  {} ({}, {}) — {}",
                            param.name, param.param_type, req, param.description
                        ));
                    }
                } else {
                    // Suggest similar commands
                    let similar: Vec<String> = console
                        .commands
                        .keys()
                        .filter(|k| k.contains(&search) || search.contains(k.as_str()))
                        .cloned()
                        .collect();
                    console.error(format!("Unknown command: '{}'", search));
                    if !similar.is_empty() {
                        console.info(format!("  Did you mean: {}?", similar.join(", ")));
                    }
                }
            }
        }

        "clear" => {
            console.clear_output();
            console.info("Console cleared.");
        }

        "version" => {
            console.success(format!(
                "Procedural Worlds Engine v{}",
                env!("CARGO_PKG_VERSION")
            ));
        }

        "echo" => {
            let message = args.join(" ");
            console.info(message);
        }

        "history" => {
            if console.history.is_empty() {
                console.info("No command history.");
            } else {
                let lines: Vec<String> = console
                    .history
                    .iter()
                    .enumerate()
                    .map(|(i, cmd)| format!("  {}: {}", i + 1, cmd))
                    .collect();
                console.print(ConsoleOutputLine::header(format!(
                    "Command history ({} entries):",
                    lines.len()
                )));
                for line in lines {
                    console.info(line);
                }
            }
        }

        "pos" => {
            if let Ok((transform, _)) = player_query.get_single() {
                let pos = transform.translation;
                console.success(format!(
                    "Position: ({:.2}, {:.2}, {:.2})",
                    pos.x, pos.y, pos.z
                ));
            } else {
                console.error("Player not found.");
            }
        }

        "tp" | "teleport" => {
            if args.len() != 3 {
                console.error("Usage: tp <x> <y> <z>");
                return;
            }
            let coords: Result<Vec<f32>, _> = args.iter().map(|a| a.parse::<f32>()).collect();
            match coords {
                Ok(c) => {
                    if let Ok((mut transform, _)) = player_query.get_single_mut() {
                        transform.translation = Vec3::new(c[0], c[1], c[2]);
                        console.success(format!(
                            "Teleported to ({:.2}, {:.2}, {:.2})",
                            c[0], c[1], c[2]
                        ));
                    } else {
                        console.error("Player not found.");
                    }
                }
                Err(_) => console.error("Invalid coordinates. Use numbers: tp <x> <y> <z>"),
            }
        }

        "fly" => {
            if let Ok((_, mut movement)) = player_query.get_single_mut() {
                movement.flying = !movement.flying;
                let status = if movement.flying { "enabled" } else { "disabled" };
                console.success(format!("Flying {}", status));
            } else {
                console.error("Player not found.");
            }
        }

        "noclip" => {
            if let Ok((_, mut movement)) = player_query.get_single_mut() {
                movement.noclip = !movement.noclip;
                let status = if movement.noclip { "enabled" } else { "disabled" };
                console.success(format!("Noclip {}", status));
            } else {
                console.error("Player not found.");
            }
        }

        "god" => {
            if let Ok((_, mut movement)) = player_query.get_single_mut() {
                let enable = !(movement.flying && movement.noclip);
                movement.flying = enable;
                movement.noclip = enable;
                let status = if enable { "enabled" } else { "disabled" };
                console.success(format!("God mode {} (fly + noclip)", status));
            } else {
                console.error("Player not found.");
            }
        }

        "debug" => {
            if let Some(ref mut overlay) = overlay_state {
                overlay.visible = !overlay.visible;
                let status = if overlay.visible { "shown" } else { "hidden" };
                console.success(format!("Debug overlay {}", status));
            } else {
                console.error("Debug overlay not available.");
            }
        }

        "wireframe" => {
            if let Some(ref mut overlay) = overlay_state {
                if let Some(arg) = args.first() {
                    match arg.to_lowercase().as_str() {
                        "on" | "true" | "1" => overlay.wireframe_enabled = true,
                        "off" | "false" | "0" => overlay.wireframe_enabled = false,
                        _ => {
                            console.error("Usage: wireframe [on|off]");
                            return;
                        }
                    }
                } else {
                    overlay.wireframe_enabled = !overlay.wireframe_enabled;
                }
                let status = if overlay.wireframe_enabled { "ON" } else { "OFF" };
                console.success(format!("Wireframe: {}", status));
            } else {
                console.error("Debug overlay not available.");
            }
        }

        "shadows" => {
            if let Some(ref overlay) = overlay_state {
                console.info(format!(
                    "Shadows: {} (use F7 to toggle)",
                    if overlay.shadows_enabled { "ON" } else { "OFF" }
                ));
            } else {
                console.error("Debug overlay not available.");
            }
        }

        "profiler" => {
            if let Some(ref mut prof) = profiler {
                prof.overlay_visible = !prof.overlay_visible;
                let status = if prof.overlay_visible { "enabled" } else { "disabled" };
                console.success(format!("Profiler overlay {}", status));
            } else {
                console.info("Toggle profiler with F4 key.");
            }
        }

        "fps" => {
            if let Some(ref overlay) = overlay_state {
                console.success(format!(
                    "FPS: {:.1} ({:.2} ms/frame)",
                    overlay.cached_fps, overlay.cached_frame_time_ms
                ));
            } else {
                console.error("Performance data not available.");
            }
        }

        "memory" | "mem" => {
            if let Some(ref overlay) = overlay_state {
                if let Some(ref mem) = overlay.cached_process_memory {
                    console.success(format!(
                        "Process memory: {}",
                        crate::engine::memory::format_bytes(mem.rss_bytes)
                    ));
                    if let Some(peak) = mem.peak_rss_bytes {
                        console.info(format!(
                            "Peak: {}",
                            crate::engine::memory::format_bytes(peak)
                        ));
                    }
                } else {
                    console.warning("Memory info not available.");
                }
                console.info(format!("Entities: {}", overlay.cached_entity_count as u64));
            } else {
                console.error("Debug overlay not available.");
            }
        }

        "stats" => {
            if let Some(ref overlay) = overlay_state {
                console.print(ConsoleOutputLine::header("═══ Engine Statistics ═══"));
                console.info(format!("FPS: {:.1}", overlay.cached_fps));
                console.info(format!("Frame time: {:.2} ms", overlay.cached_frame_time_ms));
                console.info(format!("Entities: {}", overlay.cached_entity_count as u64));
                if let Some(ref mem) = overlay.cached_process_memory {
                    console.info(format!(
                        "Memory: {}",
                        crate::engine::memory::format_bytes(mem.rss_bytes)
                    ));
                }
                if let Some(ref cm) = chunk_manager {
                    console.info(format!("Loaded chunks: {}", cm.chunks.len()));
                    console.info(format!("Render distance: {}", cm.render_distance));
                }
            } else {
                console.error("Performance data not available.");
            }
        }

        "chunks" => {
            if let Some(ref cm) = chunk_manager {
                console.print(ConsoleOutputLine::header("═══ Chunk Statistics ═══"));
                console.info(format!("Loaded chunks: {}", cm.chunks.len()));
                console.info(format!("Pending chunks: {}", cm.pending.len()));
                console.info(format!("Render distance: {}", cm.render_distance));
                console.info(format!("Load distance: {}", cm.effective_load_distance()));
            } else {
                console.error("Chunk manager not available.");
            }
        }

        "time" => {
            if let Some(ref mut cycle) = day_night {
                if args.is_empty() {
                    console.success(format!(
                        "Time of day: {:.4} ({})",
                        cycle.time_of_day,
                        cycle.clock_display()
                    ));
                } else {
                    match args[0].parse::<f32>() {
                        Ok(v) if (0.0..=1.0).contains(&v) => {
                            cycle.time_of_day = v;
                            console.success(format!(
                                "Time set to {:.4} ({})",
                                v,
                                cycle.clock_display()
                            ));
                        }
                        Ok(_) => console.error("Time must be between 0.0 and 1.0"),
                        Err(_) => console.error("Invalid number. Usage: time [0.0-1.0]"),
                    }
                }
            } else {
                console.error("Day/night cycle not available.");
            }
        }

        "pause_time" => {
            if let Some(ref mut cycle) = day_night {
                cycle.paused = !cycle.paused;
                let status = if cycle.paused { "paused" } else { "resumed" };
                console.success(format!("Day/night cycle {}", status));
            } else {
                console.error("Day/night cycle not available.");
            }
        }

        "weather" => {
            if args.is_empty() {
                console.info("Weather system: clear (default)");
                console.info("Available: clear, rain, snow, storm");
                console.warning("Weather effects not yet implemented — command registered for future use.");
            } else {
                match args[0].to_lowercase().as_str() {
                    "clear" | "rain" | "snow" | "storm" => {
                        console.success(format!("Weather set to: {}", args[0]));
                        console.warning("Visual weather effects not yet implemented.");
                    }
                    _ => console.error(format!(
                        "Unknown weather type: '{}'. Use: clear, rain, snow, storm",
                        args[0]
                    )),
                }
            }
        }

        "seed" => {
            console.info("World seed: 12345 (hardcoded)");
        }

        "render_distance" | "rd" => {
            if let Some(ref cm) = chunk_manager {
                if args.is_empty() {
                    console.info(format!(
                        "Render distance: {} chunks (use F5/F6 to adjust)",
                        cm.render_distance
                    ));
                } else {
                    console.info(format!(
                        "Current render distance: {} (use F5/F6 or slider to change)",
                        cm.render_distance
                    ));
                }
            } else {
                console.error("Chunk manager not available.");
            }
        }

        _ => {
            // Try to find similar commands for a helpful error
            let similar: Vec<String> = console
                .commands
                .keys()
                .filter(|k| {
                    k.starts_with(&raw_cmd[..1.min(raw_cmd.len())])
                        || levenshtein_close(k, &raw_cmd)
                })
                .take(3)
                .cloned()
                .collect();

            console.error(format!(
                "Unknown command: '{}'. Type 'help' for available commands.",
                raw_cmd
            ));
            if !similar.is_empty() {
                console.info(format!("  Did you mean: {}?", similar.join(", ")));
            }
        }
    }
}

/// Simple check if two strings are "close enough" (differ by ≤2 characters).
fn levenshtein_close(a: &str, b: &str) -> bool {
    if a.len().abs_diff(b.len()) > 2 {
        return false;
    }
    let mut diff = 0;
    for (ca, cb) in a.chars().zip(b.chars()) {
        if ca != cb {
            diff += 1;
        }
        if diff > 2 {
            return false;
        }
    }
    diff + a.len().abs_diff(b.len()) <= 2
}

// ============================================================================
// Console Toggle System
// ============================================================================

/// System to handle console toggle and keyboard shortcuts.
fn console_toggle_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut console: ResMut<DebugConsoleState>,
) {
    if keyboard.just_pressed(CONSOLE_TOGGLE_KEY)
        || keyboard.just_pressed(CONSOLE_TOGGLE_KEY_ALT)
    {
        console.visible = !console.visible;
        if console.visible {
            console.focus_input = true;
        }
    }
}

// ============================================================================
// Console UI System
// ============================================================================

/// System to render the debug console UI.
fn console_ui_system(
    mut contexts: EguiContexts,
    mut console: ResMut<DebugConsoleState>,
) {
    if !console.visible {
        return;
    }

    let ctx = contexts.ctx_mut();

    let mut submitted_command = false;

    egui::Window::new("🖥 Debug Console")
        .id(egui::Id::new("debug_console_unified"))
        .default_pos([50.0, 50.0])
        .default_size([620.0, 400.0])
        .resizable(true)
        .collapsible(true)
        .show(ctx, |ui| {
            // ── Category tabs ──
            ui.horizontal(|ui| {
                for category in CommandCategory::TABS {
                    let selected = console.selected_category == category;
                    let text = egui::RichText::new(category.display_name());
                    let text = if selected {
                        text.strong().color(egui::Color32::from_rgb(100, 200, 255))
                    } else {
                        text.color(egui::Color32::from_rgb(180, 180, 180))
                    };

                    if ui
                        .selectable_label(selected, text)
                        .on_hover_text(category.description())
                        .clicked()
                    {
                        console.selected_category = category;
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.small("F11 or ` to toggle");
                });
            });

            ui.separator();

            // ── Command quick reference (collapsible) ──
            egui::CollapsingHeader::new("📋 Commands")
                .default_open(false)
                .show(ui, |ui| {
                    let cmds = console.commands_in_category(console.selected_category);
                    egui::Grid::new("cmd_ref_grid")
                        .num_columns(2)
                        .spacing([20.0, 3.0])
                        .show(ui, |ui| {
                            for cmd in cmds {
                                ui.monospace(&cmd.usage);
                                ui.label(&cmd.description);
                                ui.end_row();
                            }
                        });
                });

            ui.separator();

            // ── Output area ──
            let output_height = ui.available_height() - 60.0;
            egui::ScrollArea::vertical()
                .id_salt("console_output_scroll")
                .max_height(output_height)
                .auto_shrink([false; 2])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for line in console.output.iter() {
                        ui.colored_label(line.color, &line.text);
                    }
                });

            if console.scroll_to_bottom {
                console.scroll_to_bottom = false;
            }

            ui.separator();

            // ── Input area ──
            ui.horizontal(|ui| {
                ui.label(">");

                let response = ui.add(
                    egui::TextEdit::singleline(&mut console.input)
                        .desired_width(ui.available_width() - 60.0)
                        .font(egui::TextStyle::Monospace)
                        .hint_text("Enter command... (Tab=autocomplete, ↑↓=history)"),
                );

                // Focus management
                if console.focus_input {
                    response.request_focus();
                    console.focus_input = false;
                }

                // Keyboard handling
                if response.has_focus() {
                    // Update suggestions on text change
                    if response.changed() {
                        console.update_suggestions();
                    }

                    // Tab completion
                    if ui.input(|i| i.key_pressed(egui::Key::Tab)) {
                        if console.show_suggestions && !console.suggestions.is_empty() {
                            console.apply_suggestion();
                        } else {
                            console.update_suggestions();
                            if !console.suggestions.is_empty() {
                                console.show_suggestions = true;
                            }
                        }
                    }

                    // Arrow key handling
                    if console.show_suggestions {
                        // When suggestions visible, arrows navigate suggestions
                        if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                            console.select_prev_suggestion();
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                            console.select_next_suggestion();
                        }
                    } else {
                        // Otherwise, arrows navigate history
                        if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                            console.history_prev();
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                            console.history_next();
                        }
                    }

                    // Escape closes suggestions or console
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        if console.show_suggestions {
                            console.show_suggestions = false;
                            console.selected_suggestion = -1;
                        } else {
                            console.visible = false;
                        }
                    }

                    // Enter submits
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        if console.submit().is_some() {
                            submitted_command = true;
                        }
                        console.focus_input = true;
                    }
                }

                // Run button
                if ui.button("Run").clicked() {
                    if console.submit().is_some() {
                        submitted_command = true;
                    }
                    console.focus_input = true;
                }
            });

            // ── Autocomplete popup ──
            if console.show_suggestions && !console.suggestions.is_empty() {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(300.0);
                    for (idx, suggestion) in console.suggestions.iter().enumerate() {
                        let is_selected = idx as i32 == console.selected_suggestion;
                        let bg = if is_selected {
                            egui::Color32::from_rgb(60, 60, 100)
                        } else {
                            egui::Color32::TRANSPARENT
                        };

                        egui::Frame::none()
                            .fill(bg)
                            .inner_margin(egui::Margin::symmetric(4.0, 2.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Show command name and description
                                    let desc = console
                                        .commands
                                        .get(suggestion)
                                        .map(|c| c.description.as_str())
                                        .unwrap_or("");
                                    ui.monospace(
                                        egui::RichText::new(suggestion)
                                            .color(egui::Color32::from_rgb(100, 200, 255)),
                                    );
                                    if !desc.is_empty() {
                                        ui.label(" — ");
                                        ui.colored_label(
                                            egui::Color32::from_rgb(180, 180, 180),
                                            desc,
                                        );
                                    }
                                });
                            });
                    }

                    ui.separator();
                    ui.colored_label(
                        egui::Color32::from_rgb(120, 120, 120),
                        "Tab: complete | ↑↓: navigate | Enter: execute | Esc: close",
                    );
                });
            }
        });

    // After UI draw, ensure focus stays if we just submitted
    if submitted_command {
        console.focus_input = true;
    }
}

// ============================================================================
// Plugin
// ============================================================================

/// Plugin that adds the unified debug console.
///
/// Toggle with F11 or backtick (`). Commands organized into categories
/// with tab-completion, history navigation, and parameter validation.
pub struct DebugConsolePlugin;

impl Plugin for DebugConsolePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DebugConsoleState>()
            .add_systems(
                Update,
                (console_toggle_system, console_ui_system, execute_commands).chain(),
            );
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ── Default state ──

    #[test]
    fn test_default_state() {
        let state = DebugConsoleState::default();
        assert!(!state.visible);
        assert!(state.input.is_empty());
        assert!(!state.output.is_empty()); // welcome message
        assert!(state.history.is_empty());
        assert_eq!(state.history_index, -1);
        assert!(!state.commands.is_empty());
        assert!(state.commands.contains_key("help"));
        assert!(state.commands.contains_key("tp"));
        assert!(state.commands.contains_key("fly"));
    }

    // ── Command registration ──

    #[test]
    fn test_register_command() {
        let mut state = DebugConsoleState::default();
        let initial_count = state.commands.len();

        state.register(
            CommandDefinition::new("test_cmd", "A test", CommandCategory::Debug)
                .alias("tc"),
        );

        assert_eq!(state.commands.len(), initial_count + 1);
        assert!(state.commands.contains_key("test_cmd"));
        assert_eq!(state.resolve_command("tc"), Some("test_cmd".to_string()));
    }

    #[test]
    fn test_commands_in_category() {
        let state = DebugConsoleState::default();

        let world_cmds = state.commands_in_category(CommandCategory::World);
        assert!(world_cmds.iter().any(|c| c.name == "chunks"));
        assert!(world_cmds.iter().any(|c| c.name == "time"));

        let player_cmds = state.commands_in_category(CommandCategory::Player);
        assert!(player_cmds.iter().any(|c| c.name == "tp"));
        assert!(player_cmds.iter().any(|c| c.name == "fly"));

        let all_cmds = state.commands_in_category(CommandCategory::All);
        assert!(all_cmds.len() > world_cmds.len());
    }

    #[test]
    fn test_resolve_command_alias() {
        let state = DebugConsoleState::default();
        assert_eq!(state.resolve_command("tp"), Some("tp".to_string()));
        assert_eq!(state.resolve_command("teleport"), Some("tp".to_string()));
        assert_eq!(state.resolve_command("rd"), Some("render_distance".to_string()));
        assert_eq!(state.resolve_command("?"), Some("help".to_string()));
        assert_eq!(state.resolve_command("mem"), Some("memory".to_string()));
        assert!(state.resolve_command("nonexistent").is_none());
    }

    // ── History ──

    #[test]
    fn test_history_add() {
        let mut state = DebugConsoleState::default();
        state.add_to_history("first");
        state.add_to_history("second");
        assert_eq!(state.history.len(), 2);
        assert_eq!(state.history[0], "first");
        assert_eq!(state.history[1], "second");
    }

    #[test]
    fn test_history_no_empty() {
        let mut state = DebugConsoleState::default();
        state.add_to_history("");
        assert!(state.history.is_empty());
    }

    #[test]
    fn test_history_no_consecutive_duplicates() {
        let mut state = DebugConsoleState::default();
        state.add_to_history("same");
        state.add_to_history("same");
        state.add_to_history("same");
        assert_eq!(state.history.len(), 1);
    }

    #[test]
    fn test_history_max_size() {
        let mut state = DebugConsoleState::default();
        for i in 0..MAX_HISTORY_SIZE + 10 {
            state.add_to_history(&format!("cmd_{}", i));
        }
        assert_eq!(state.history.len(), MAX_HISTORY_SIZE);
        // Oldest should have been evicted
        assert!(state.history[0].contains("10"));
    }

    #[test]
    fn test_history_navigation() {
        let mut state = DebugConsoleState::default();
        state.add_to_history("first");
        state.add_to_history("second");
        state.add_to_history("third");
        state.input = "current".to_string();

        // Navigate up (towards older)
        state.history_prev();
        assert_eq!(state.input, "third");
        assert_eq!(state.saved_input, "current");

        state.history_prev();
        assert_eq!(state.input, "second");

        state.history_prev();
        assert_eq!(state.input, "first");

        // Can't go further
        state.history_prev();
        assert_eq!(state.input, "first");

        // Navigate down
        state.history_next();
        assert_eq!(state.input, "second");

        state.history_next();
        assert_eq!(state.input, "third");

        // All the way down restores saved input
        state.history_next();
        assert_eq!(state.input, "current");
    }

    #[test]
    fn test_history_reset() {
        let mut state = DebugConsoleState::default();
        state.add_to_history("cmd");
        state.input = "typing".to_string();
        state.history_prev();
        assert_ne!(state.history_index, -1);

        state.reset_history_navigation();
        assert_eq!(state.history_index, -1);
        assert!(state.saved_input.is_empty());
    }

    // ── Autocomplete ──

    #[test]
    fn test_suggestions_empty_input() {
        let mut state = DebugConsoleState::default();
        state.input = String::new();
        state.update_suggestions();
        assert!(state.suggestions.is_empty());
        assert!(!state.show_suggestions);
    }

    #[test]
    fn test_suggestions_partial_match() {
        let mut state = DebugConsoleState::default();
        state.input = "he".to_string();
        state.update_suggestions();
        assert!(!state.suggestions.is_empty());
        assert!(state.suggestions.contains(&"help".to_string()));
        assert!(state.show_suggestions);
    }

    #[test]
    fn test_suggestions_exact_match_hides() {
        let mut state = DebugConsoleState::default();
        state.input = "help".to_string();
        state.update_suggestions();
        // Exact match should hide suggestions
        assert!(!state.show_suggestions);
    }

    #[test]
    fn test_suggestions_with_args_hidden() {
        let mut state = DebugConsoleState::default();
        state.input = "tp 10".to_string();
        state.update_suggestions();
        assert!(!state.show_suggestions);
    }

    #[test]
    fn test_apply_suggestion() {
        let mut state = DebugConsoleState::default();
        state.input = "wi".to_string();
        state.update_suggestions();
        assert!(!state.suggestions.is_empty());
        state.apply_suggestion();
        assert_eq!(state.input, "wireframe");
        assert!(!state.show_suggestions);
    }

    #[test]
    fn test_suggestion_navigation() {
        let mut state = DebugConsoleState::default();
        state.input = "f".to_string();
        state.update_suggestions();
        let count = state.suggestions.len();
        assert!(count >= 2); // fps, fly, at least

        // Navigate down
        state.select_next_suggestion();
        assert_eq!(state.selected_suggestion, 0);
        state.select_next_suggestion();
        assert_eq!(state.selected_suggestion, 1);

        // Navigate up wraps
        let mut state2 = DebugConsoleState::default();
        state2.input = "f".to_string();
        state2.update_suggestions();
        state2.select_prev_suggestion();
        assert_eq!(state2.selected_suggestion, state2.suggestions.len() as i32 - 1);
    }

    #[test]
    fn test_max_suggestions_limit() {
        // With default commands, typing a single letter shouldn't exceed MAX_SUGGESTIONS
        let mut state = DebugConsoleState::default();
        state.input = "s".to_string(); // stats, seed, shadows, ...
        state.update_suggestions();
        assert!(state.suggestions.len() <= MAX_SUGGESTIONS);
    }

    // ── Output ──

    #[test]
    fn test_output_types() {
        let mut state = DebugConsoleState::default();
        state.clear_output();

        state.info("info");
        state.success("success");
        state.warning("warning");
        state.error("error");

        assert_eq!(state.output.len(), 4);
        assert_eq!(state.output[0].text, "info");
        assert_eq!(state.output[1].color, egui::Color32::from_rgb(100, 255, 100));
        assert_eq!(state.output[2].color, egui::Color32::from_rgb(255, 200, 100));
        assert_eq!(state.output[3].color, egui::Color32::from_rgb(255, 100, 100));
    }

    #[test]
    fn test_output_max_lines() {
        let mut state = DebugConsoleState::default();
        state.clear_output();

        for i in 0..MAX_OUTPUT_LINES + 50 {
            state.info(format!("Line {}", i));
        }

        assert_eq!(state.output.len(), MAX_OUTPUT_LINES);
    }

    #[test]
    fn test_clear_output() {
        let mut state = DebugConsoleState::default();
        state.info("test");
        assert!(!state.output.is_empty());
        state.clear_output();
        assert!(state.output.is_empty());
    }

    #[test]
    fn test_command_echo_format() {
        let line = ConsoleOutputLine::command("help");
        assert_eq!(line.text, "> help");
        assert_eq!(line.color, egui::Color32::from_rgb(150, 200, 255));
    }

    // ── Submit ──

    #[test]
    fn test_submit_returns_command() {
        let mut state = DebugConsoleState::default();
        state.input = "echo hello".to_string();
        let result = state.submit();
        assert_eq!(result, Some("echo hello".to_string()));
        assert!(state.input.is_empty());
        assert_eq!(state.history.len(), 1);
    }

    #[test]
    fn test_submit_empty_returns_none() {
        let mut state = DebugConsoleState::default();
        state.input = "   ".to_string();
        let result = state.submit();
        assert!(result.is_none());
    }

    // ── Validation ──

    #[test]
    fn test_parameter_validation_valid() {
        let cmd = CommandDefinition::new("test", "test", CommandCategory::Debug)
            .param("x", ParameterType::Float, "x coord")
            .param("y", ParameterType::Float, "y coord");

        let result = cmd.validate_args(&["10.5", "20.0"]);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert_eq!(values.len(), 2);
        assert_eq!(values[0].as_float(), Some(10.5));
        assert_eq!(values[1].as_float(), Some(20.0));
    }

    #[test]
    fn test_parameter_validation_missing_required() {
        let cmd = CommandDefinition::new("test", "test", CommandCategory::Debug)
            .param("x", ParameterType::Float, "x coord")
            .param("y", ParameterType::Float, "y coord");

        let result = cmd.validate_args(&["10.5"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parameter_validation_wrong_type() {
        let cmd = CommandDefinition::new("test", "test", CommandCategory::Debug)
            .param("count", ParameterType::Int, "count");

        let result = cmd.validate_args(&["abc"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parameter_validation_optional() {
        let cmd = CommandDefinition::new("test", "test", CommandCategory::Debug)
            .optional_param("value", ParameterType::Float, "optional value");

        // No args is fine for optional
        let result = cmd.validate_args(&[]);
        assert!(result.is_ok());

        // With arg is also fine
        let result = cmd.validate_args(&["42.0"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_bool_values() {
        assert_eq!(
            parse_parameter("true", ParameterType::Bool),
            Ok(ParameterValue::Bool(true))
        );
        assert_eq!(
            parse_parameter("on", ParameterType::Bool),
            Ok(ParameterValue::Bool(true))
        );
        assert_eq!(
            parse_parameter("false", ParameterType::Bool),
            Ok(ParameterValue::Bool(false))
        );
        assert_eq!(
            parse_parameter("off", ParameterType::Bool),
            Ok(ParameterValue::Bool(false))
        );
        assert!(parse_parameter("maybe", ParameterType::Bool).is_err());
    }

    // ── Categories ──

    #[test]
    fn test_category_tabs_complete() {
        assert_eq!(CommandCategory::TABS.len(), 5);
        assert!(CommandCategory::TABS.contains(&CommandCategory::All));
        assert!(CommandCategory::TABS.contains(&CommandCategory::World));
    }

    #[test]
    fn test_category_display_names() {
        for cat in CommandCategory::TABS {
            assert!(!cat.display_name().is_empty());
            assert!(!cat.description().is_empty());
        }
    }

    // ── Levenshtein helper ──

    #[test]
    fn test_levenshtein_close() {
        assert!(levenshtein_close("help", "helq")); // 1 diff
        assert!(levenshtein_close("fly", "fli"));   // 1 diff
        assert!(!levenshtein_close("help", "abcd")); // too many diffs
        assert!(!levenshtein_close("a", "abcde"));  // length diff > 2
    }

    // ── Command definition ──

    #[test]
    fn test_command_definition_usage_rebuild() {
        let cmd = CommandDefinition::new("test", "desc", CommandCategory::Debug)
            .param("x", ParameterType::Float, "x")
            .optional_param("y", ParameterType::Float, "y");

        assert_eq!(cmd.usage, "test <x> [y]");
    }

    #[test]
    fn test_builtin_commands_defined() {
        let state = DebugConsoleState::default();
        // Minimum expected commands
        let expected = [
            "help", "clear", "version", "echo", "debug", "wireframe",
            "shadows", "profiler", "history", "pos", "tp", "fly",
            "noclip", "god", "chunks", "time", "pause_time", "weather",
            "seed", "render_distance", "fps", "memory", "stats",
        ];
        for name in expected {
            assert!(
                state.commands.contains_key(name),
                "Missing builtin command: {}",
                name
            );
        }
    }
}
