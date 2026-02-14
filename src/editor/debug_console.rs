//! Debug Console with Command History and Script Execution
//!
//! A toggleable debug console for entering commands:
//! - Up/Down arrow keys navigate through command history
//! - Supports loading and executing command scripts from files
//! - Tilde (~) key toggles console visibility
//!
//! # Script Format
//! Script files are plain text with one command per line.
//! Lines starting with `#` are treated as comments.
//! Empty lines are skipped.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use std::collections::VecDeque;
use std::fs;
use std::path::Path;

/// Maximum number of commands to keep in history
const MAX_HISTORY_SIZE: usize = 100;

/// Maximum number of output lines to retain in the console log
const MAX_OUTPUT_LINES: usize = 500;

/// Debug console state and configuration
#[derive(Resource)]
pub struct DebugConsoleState {
    /// Whether the console is currently visible
    pub visible: bool,
    /// Current input text being typed
    pub input: String,
    /// Command history (most recent last)
    history: VecDeque<String>,
    /// Current position in history for navigation (-1 means not navigating)
    history_index: isize,
    /// Saved input before starting history navigation
    saved_input: String,
    /// Console output log
    output: VecDeque<String>,
    /// Whether to scroll to bottom on next frame
    scroll_to_bottom: bool,
    /// Whether input field should request focus
    request_focus: bool,
}

impl Default for DebugConsoleState {
    fn default() -> Self {
        Self {
            visible: false,
            input: String::new(),
            history: VecDeque::with_capacity(MAX_HISTORY_SIZE),
            history_index: -1,
            saved_input: String::new(),
            output: VecDeque::with_capacity(MAX_OUTPUT_LINES),
            scroll_to_bottom: false,
            request_focus: false,
        }
    }
}

impl DebugConsoleState {
    /// Add a command to history
    pub fn add_to_history(&mut self, command: String) {
        // Don't add empty commands or duplicates of the last command
        if command.is_empty() {
            return;
        }
        if let Some(last) = self.history.back()
            && last == &command
        {
            return;
        }

        // Add to history, removing oldest if at capacity
        if self.history.len() >= MAX_HISTORY_SIZE {
            self.history.pop_front();
        }
        self.history.push_back(command);
    }

    /// Navigate to previous command in history (up arrow)
    pub fn history_previous(&mut self) {
        if self.history.is_empty() {
            return;
        }

        // Save current input when starting navigation
        if self.history_index == -1 {
            self.saved_input = self.input.clone();
        }

        // Move up in history (towards older commands)
        let max_index = self.history.len() as isize - 1;
        if self.history_index < max_index {
            self.history_index += 1;
            let idx = self.history.len() - 1 - self.history_index as usize;
            if let Some(cmd) = self.history.get(idx) {
                self.input = cmd.clone();
            }
        }
    }

    /// Navigate to next command in history (down arrow)
    pub fn history_next(&mut self) {
        if self.history_index == -1 {
            return;
        }

        self.history_index -= 1;

        if self.history_index == -1 {
            // Restore saved input
            self.input = self.saved_input.clone();
        } else {
            let idx = self.history.len() - 1 - self.history_index as usize;
            if let Some(cmd) = self.history.get(idx) {
                self.input = cmd.clone();
            }
        }
    }

    /// Reset history navigation state
    pub fn reset_history_navigation(&mut self) {
        self.history_index = -1;
        self.saved_input.clear();
    }

    /// Add a line to console output
    pub fn print(&mut self, message: impl Into<String>) {
        if self.output.len() >= MAX_OUTPUT_LINES {
            self.output.pop_front();
        }
        self.output.push_back(message.into());
        self.scroll_to_bottom = true;
    }

    /// Add an error message to console output (will be styled differently)
    pub fn print_error(&mut self, message: impl Into<String>) {
        self.print(format!("[ERROR] {}", message.into()));
    }

    /// Add a success message to console output
    pub fn print_success(&mut self, message: impl Into<String>) {
        self.print(format!("[OK] {}", message.into()));
    }

    /// Clear the console output
    pub fn clear_output(&mut self) {
        self.output.clear();
    }

    /// Get command history as a slice (oldest to newest)
    pub fn history(&self) -> impl Iterator<Item = &String> {
        self.history.iter()
    }

    /// Get the number of commands in history
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    /// Execute a script file, running each line as a command
    ///
    /// Returns the number of commands executed, or an error message
    pub fn execute_script(&mut self, path: &Path) -> Result<usize, String> {
        let content =
            fs::read_to_string(path).map_err(|e| format!("Failed to read script file: {}", e))?;

        let mut executed_count = 0;

        for (line_num, line) in content.lines().enumerate() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            self.print(format!("{}> {}", line_num + 1, line));

            // Execute the command
            if let Err(e) = self.execute_command(line) {
                self.print_error(format!("Line {}: {}", line_num + 1, e));
                // Continue executing remaining commands
            } else {
                executed_count += 1;
            }
        }

        Ok(executed_count)
    }

    /// Execute a single command and handle the result
    pub fn execute_command(&mut self, command: &str) -> Result<(), String> {
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(());
        }

        let cmd = parts[0].to_lowercase();
        let args = &parts[1..];

        match cmd.as_str() {
            "help" => {
                self.print("Available commands:");
                self.print("  help              - Show this help message");
                self.print("  clear             - Clear console output");
                self.print("  history           - Show command history");
                self.print("  echo <text>       - Print text to console");
                self.print("  exec <file>       - Execute commands from a script file");
                self.print("  time              - Show current time");
                self.print("  version           - Show engine version");
                Ok(())
            }
            "clear" => {
                self.clear_output();
                Ok(())
            }
            "history" => {
                if self.history.is_empty() {
                    self.print("No command history");
                } else {
                    // Collect history first to avoid borrow conflict
                    let history_lines: Vec<String> = self
                        .history
                        .iter()
                        .enumerate()
                        .map(|(i, cmd)| format!("  {}: {}", i + 1, cmd))
                        .collect();
                    self.print(format!(
                        "Command history ({} entries):",
                        history_lines.len()
                    ));
                    for line in history_lines {
                        self.print(line);
                    }
                }
                Ok(())
            }
            "echo" => {
                let message = args.join(" ");
                self.print(message);
                Ok(())
            }
            "exec" => {
                if args.is_empty() {
                    return Err("Usage: exec <script_file>".to_string());
                }
                let path = Path::new(args[0]);
                match self.execute_script(path) {
                    Ok(count) => {
                        self.print_success(format!("Executed {} commands from {:?}", count, path));
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            }
            "time" => {
                use std::time::SystemTime;
                let now = SystemTime::now();
                let duration = now
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map_err(|e| e.to_string())?;
                self.print(format!(
                    "System time: {} seconds since epoch",
                    duration.as_secs()
                ));
                Ok(())
            }
            "version" => {
                self.print(format!(
                    "Procedural Worlds Engine v{}",
                    env!("CARGO_PKG_VERSION")
                ));
                Ok(())
            }
            _ => Err(format!(
                "Unknown command: '{}'. Type 'help' for available commands.",
                cmd
            )),
        }
    }

    /// Submit the current input as a command
    pub fn submit_input(&mut self) {
        let command = self.input.trim().to_string();
        if command.is_empty() {
            return;
        }

        // Echo the command
        self.print(format!("> {}", command));

        // Add to history
        self.add_to_history(command.clone());

        // Execute
        if let Err(e) = self.execute_command(&command) {
            self.print_error(e);
        }

        // Clear input and reset navigation
        self.input.clear();
        self.reset_history_navigation();
    }
}

/// System to toggle console visibility with tilde key
pub fn console_toggle_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut console_state: ResMut<DebugConsoleState>,
) {
    // Toggle with Grave (`) / Tilde (~) key
    if keyboard.just_pressed(KeyCode::Backquote) {
        console_state.visible = !console_state.visible;
        if console_state.visible {
            console_state.request_focus = true;
        }
    }
}

/// System to render the debug console UI
pub fn console_ui_system(mut contexts: EguiContexts, mut console_state: ResMut<DebugConsoleState>) {
    if !console_state.visible {
        return;
    }

    let ctx = contexts.ctx_mut();

    // Console window at bottom of screen
    egui::Window::new("Debug Console")
        .anchor(egui::Align2::LEFT_BOTTOM, [10.0, -10.0])
        .default_width(600.0)
        .default_height(300.0)
        .resizable(true)
        .collapsible(true)
        .show(ctx, |ui| {
            // Output area (scrollable)
            let output_height = ui.available_height() - 30.0;
            egui::ScrollArea::vertical()
                .max_height(output_height)
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for line in console_state.output.iter() {
                        let color = if line.starts_with("[ERROR]") {
                            egui::Color32::from_rgb(255, 100, 100)
                        } else if line.starts_with("[OK]") {
                            egui::Color32::from_rgb(100, 255, 100)
                        } else if line.starts_with('>') {
                            egui::Color32::from_rgb(150, 200, 255)
                        } else {
                            egui::Color32::from_rgb(200, 200, 200)
                        };
                        ui.colored_label(color, egui::RichText::new(line).monospace());
                    }
                });

            ui.separator();

            // Input area
            ui.horizontal(|ui| {
                ui.label(">");

                let input_response = ui.add(
                    egui::TextEdit::singleline(&mut console_state.input)
                        .desired_width(ui.available_width() - 60.0)
                        .font(egui::TextStyle::Monospace),
                );

                // Request focus if needed
                if console_state.request_focus {
                    input_response.request_focus();
                    console_state.request_focus = false;
                }

                // Handle keyboard input when focused
                if input_response.has_focus() {
                    // Up arrow - previous history
                    if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                        console_state.history_previous();
                    }
                    // Down arrow - next history
                    if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                        console_state.history_next();
                    }
                    // Enter - submit command
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        console_state.submit_input();
                    }
                }

                if ui.button("Run").clicked() {
                    console_state.submit_input();
                }
            });
        });
}

/// Plugin that adds the debug console functionality
pub struct DebugConsolePlugin;

impl Plugin for DebugConsolePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DebugConsoleState>()
            .add_systems(Update, (console_toggle_system, console_ui_system).chain());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_to_history() {
        let mut console = DebugConsoleState::default();

        console.add_to_history("first".to_string());
        console.add_to_history("second".to_string());

        assert_eq!(console.history_len(), 2);
        let history: Vec<_> = console.history().collect();
        assert_eq!(history[0], "first");
        assert_eq!(history[1], "second");
    }

    #[test]
    fn test_no_duplicate_consecutive_history() {
        let mut console = DebugConsoleState::default();

        console.add_to_history("same".to_string());
        console.add_to_history("same".to_string());
        console.add_to_history("same".to_string());

        assert_eq!(console.history_len(), 1);
    }

    #[test]
    fn test_empty_not_added_to_history() {
        let mut console = DebugConsoleState::default();

        console.add_to_history("".to_string());

        assert_eq!(console.history_len(), 0);
    }

    #[test]
    fn test_history_navigation() {
        let mut console = DebugConsoleState::default();

        console.add_to_history("first".to_string());
        console.add_to_history("second".to_string());
        console.add_to_history("third".to_string());

        console.input = "current".to_string();

        // Navigate up (to most recent)
        console.history_previous();
        assert_eq!(console.input, "third");

        console.history_previous();
        assert_eq!(console.input, "second");

        console.history_previous();
        assert_eq!(console.input, "first");

        // Can't go further back
        console.history_previous();
        assert_eq!(console.input, "first");

        // Navigate down
        console.history_next();
        assert_eq!(console.input, "second");

        console.history_next();
        assert_eq!(console.input, "third");

        // Back to saved input
        console.history_next();
        assert_eq!(console.input, "current");
    }

    #[test]
    fn test_history_max_size() {
        let mut console = DebugConsoleState::default();

        for i in 0..MAX_HISTORY_SIZE + 10 {
            console.add_to_history(format!("command_{}", i));
        }

        assert_eq!(console.history_len(), MAX_HISTORY_SIZE);

        // Oldest commands should be removed
        let history: Vec<_> = console.history().collect();
        assert!(history[0].contains("10")); // command_10 is first
    }

    #[test]
    fn test_print_output() {
        let mut console = DebugConsoleState::default();

        console.print("test message");
        console.print_error("error message");
        console.print_success("success message");

        assert_eq!(console.output.len(), 3);
        assert_eq!(console.output[0], "test message");
        assert!(console.output[1].contains("[ERROR]"));
        assert!(console.output[2].contains("[OK]"));
    }

    #[test]
    fn test_clear_output() {
        let mut console = DebugConsoleState::default();

        console.print("line 1");
        console.print("line 2");
        console.clear_output();

        assert!(console.output.is_empty());
    }

    #[test]
    fn test_execute_help_command() {
        let mut console = DebugConsoleState::default();

        let result = console.execute_command("help");
        assert!(result.is_ok());
        assert!(!console.output.is_empty());
    }

    #[test]
    fn test_execute_echo_command() {
        let mut console = DebugConsoleState::default();

        let result = console.execute_command("echo hello world");
        assert!(result.is_ok());
        assert!(console.output.iter().any(|l| l.contains("hello world")));
    }

    #[test]
    fn test_execute_unknown_command() {
        let mut console = DebugConsoleState::default();

        let result = console.execute_command("unknown_cmd");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown command"));
    }

    #[test]
    fn test_execute_version_command() {
        let mut console = DebugConsoleState::default();

        let result = console.execute_command("version");
        assert!(result.is_ok());
        assert!(
            console
                .output
                .iter()
                .any(|l| l.contains("Procedural Worlds"))
        );
    }

    #[test]
    fn test_submit_input() {
        let mut console = DebugConsoleState::default();

        console.input = "echo test".to_string();
        console.submit_input();

        // Input should be cleared
        assert!(console.input.is_empty());
        // Command should be in history
        assert_eq!(console.history_len(), 1);
        // Output should contain the command and result
        assert!(console.output.iter().any(|l| l.contains("> echo test")));
        assert!(console.output.iter().any(|l| l.contains("test")));
    }

    #[test]
    fn test_reset_history_navigation() {
        let mut console = DebugConsoleState::default();

        console.add_to_history("cmd".to_string());
        console.input = "current".to_string();
        console.history_previous();

        console.reset_history_navigation();

        assert_eq!(console.history_index, -1);
        assert!(console.saved_input.is_empty());
    }
}
