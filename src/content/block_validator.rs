//! Block Content Validation
//!
//! Validates block definitions and provides detailed error information
//! with position hints for the editor UI.

use super::BlockDefinition;

// ============================================================================
// VALIDATION TYPES
// ============================================================================

/// Severity level for validation messages
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValidationSeverity {
    Info,
    Warning,
    Error,
}

impl std::fmt::Display for ValidationSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => write!(f, "info"),
            Self::Warning => write!(f, "warning"),
            Self::Error => write!(f, "error"),
        }
    }
}

/// Position span within a block definition (for editor gutter markers)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationSpan {
    /// Field name that has the issue
    pub field: String,
    /// Line number (1-based, for UI display)
    pub line: usize,
    /// Column number (1-based)
    pub column: usize,
    /// Length of the problematic span
    pub length: usize,
}

impl ValidationSpan {
    pub fn for_field(field: &str) -> Self {
        // Map field names to approximate line positions in a RON/editor layout
        let line = match field {
            "id" => 1,
            "display_name" => 2,
            "numeric_id" => 3,
            "hardness" => 4,
            "tool_required" => 5,
            "physics.solid" => 6,
            "physics.transparent" => 7,
            "physics.passable" => 8,
            "visuals.color" => 9,
            "visuals.color.r" | "visuals.color.g" | "visuals.color.b" | "visuals.color.a" => 9,
            "visuals.light_level" => 10,
            _ => 0,
        };
        Self {
            field: field.to_string(),
            line,
            column: 1,
            length: field.len(),
        }
    }
}

/// A single validation issue
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    /// Severity of the issue
    pub severity: ValidationSeverity,
    /// Human-readable message
    pub message: String,
    /// Position/field where the issue occurs
    pub span: ValidationSpan,
    /// Optional fix suggestion
    pub suggestion: Option<String>,
}

/// Result of validating a block definition
#[derive(Debug, Clone, Default)]
pub struct ValidationResult {
    /// All issues found during validation
    pub issues: Vec<ValidationIssue>,
}

impl ValidationResult {
    pub fn new() -> Self {
        Self { issues: Vec::new() }
    }

    /// Add an issue to the result
    pub fn add(&mut self, severity: ValidationSeverity, field: &str, message: impl Into<String>, suggestion: Option<String>) {
        self.issues.push(ValidationIssue {
            severity,
            message: message.into(),
            span: ValidationSpan::for_field(field),
            suggestion,
        });
    }

    /// Returns true if there are any errors (not warnings/info)
    pub fn has_errors(&self) -> bool {
        self.issues.iter().any(|i| i.severity == ValidationSeverity::Error)
    }

    /// Returns true if there are any issues at all
    pub fn has_issues(&self) -> bool {
        !self.issues.is_empty()
    }

    /// Returns true if validation passed (no errors)
    pub fn is_valid(&self) -> bool {
        !self.has_errors()
    }

    /// Get issues filtered by severity
    pub fn by_severity(&self, severity: ValidationSeverity) -> Vec<&ValidationIssue> {
        self.issues.iter().filter(|i| i.severity == severity).collect()
    }

    /// Get issues sorted by line number
    pub fn sorted_by_line(&self) -> Vec<&ValidationIssue> {
        let mut issues: Vec<_> = self.issues.iter().collect();
        issues.sort_by_key(|i| (i.span.line, i.severity));
        issues
    }

    /// Count of errors
    pub fn error_count(&self) -> usize {
        self.issues.iter().filter(|i| i.severity == ValidationSeverity::Error).count()
    }

    /// Count of warnings
    pub fn warning_count(&self) -> usize {
        self.issues.iter().filter(|i| i.severity == ValidationSeverity::Warning).count()
    }
}

// ============================================================================
// VALIDATION LOGIC
// ============================================================================

/// Validate a block definition and return detailed results
pub fn validate_block(block: &BlockDefinition) -> ValidationResult {
    let mut result = ValidationResult::new();

    // ID validation
    if block.id.is_empty() {
        result.add(ValidationSeverity::Error, "id", "Block ID cannot be empty", 
            Some("Enter a unique identifier like 'my_block'".into()));
    } else if block.id.contains(char::is_whitespace) {
        result.add(ValidationSeverity::Error, "id", "Block ID cannot contain whitespace",
            Some("Use underscores instead of spaces".into()));
    } else if !block.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        result.add(ValidationSeverity::Error, "id", "Block ID must contain only alphanumeric characters and underscores",
            Some("Remove special characters from the ID".into()));
    }

    // Display name validation
    if block.display_name.is_empty() {
        result.add(ValidationSeverity::Warning, "display_name", "Display name is empty",
            Some("Add a human-readable name for the block".into()));
    }

    // Hardness validation
    if block.hardness < 0.0 {
        result.add(ValidationSeverity::Error, "hardness", "Hardness cannot be negative",
            Some("Set hardness to 0.0 or higher".into()));
    } else if block.hardness == 0.0 && block.physics.solid {
        result.add(ValidationSeverity::Warning, "hardness", "Solid block with zero hardness will break instantly",
            Some("Consider setting hardness > 0 for solid blocks".into()));
    } else if block.hardness > 10.0 {
        result.add(ValidationSeverity::Info, "hardness", "Very high hardness value — block will be very slow to mine", None);
    }

    // Tool validation
    let valid_tools = ["any", "pickaxe", "shovel", "axe"];
    if !valid_tools.contains(&block.tool_required.as_str()) {
        result.add(ValidationSeverity::Warning, "tool_required", 
            format!("Unknown tool type '{}' — may not work with current tools", block.tool_required),
            Some(format!("Use one of: {}", valid_tools.join(", "))));
    }

    // Color validation
    for (i, component) in block.visuals.color.iter().enumerate() {
        if !(0.0..=1.0).contains(component) {
            let channel = ["R", "G", "B", "A"][i];
            let field = format!("visuals.color.{}", channel.to_lowercase());
            result.add(ValidationSeverity::Error, &field,
                format!("Color {} component {:.2} is out of range [0.0, 1.0]", channel, component),
                Some(format!("Clamp {} to [0.0, 1.0]", channel)));
        }
    }

    // Light level validation
    if block.visuals.light_level > 15 {
        result.add(ValidationSeverity::Warning, "visuals.light_level",
            format!("Light level {} exceeds maximum of 15", block.visuals.light_level),
            Some("Set light level to 15 or below".into()));
    }

    // Physics consistency checks
    if block.physics.transparent && !block.physics.solid {
        result.add(ValidationSeverity::Info, "physics.transparent",
            "Non-solid transparent block — ensure rendering handles this correctly", None);
    }

    if block.physics.passable && block.physics.solid {
        result.add(ValidationSeverity::Warning, "physics.passable",
            "Block is marked as both solid and passable — this may cause physics issues",
            Some("Usually passable blocks should not be solid".into()));
    }

    result
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::block::{BlockPhysics, BlockVisuals, BlockCategory};

    fn make_test_block() -> BlockDefinition {
        BlockDefinition {
            id: "test_block".into(),
            display_name: "Test Block".into(),
            block_type: None,
            numeric_id: 100,
            physics: BlockPhysics::default(),
            visuals: BlockVisuals::default(),
            hardness: 1.0,
            tool_required: "pickaxe".into(),
            category: BlockCategory::Natural,
        }
    }

    #[test]
    fn test_valid_block_passes() {
        let block = make_test_block();
        let result = validate_block(&block);
        assert!(result.is_valid());
        assert_eq!(result.error_count(), 0);
    }

    #[test]
    fn test_empty_id_is_error() {
        let mut block = make_test_block();
        block.id = String::new();
        let result = validate_block(&block);
        assert!(result.has_errors());
        assert!(result.issues[0].suggestion.is_some());
    }

    #[test]
    fn test_id_with_spaces_is_error() {
        let mut block = make_test_block();
        block.id = "bad id".into();
        let result = validate_block(&block);
        assert!(result.has_errors());
    }

    #[test]
    fn test_negative_hardness_is_error() {
        let mut block = make_test_block();
        block.hardness = -1.0;
        let result = validate_block(&block);
        assert!(result.has_errors());
    }

    #[test]
    fn test_zero_hardness_solid_is_warning() {
        let mut block = make_test_block();
        block.hardness = 0.0;
        block.physics.solid = true;
        let result = validate_block(&block);
        assert!(!result.has_errors());
        assert!(result.warning_count() > 0);
    }

    #[test]
    fn test_invalid_color_is_error() {
        let mut block = make_test_block();
        block.visuals.color = [1.5, 0.0, 0.0, 1.0];
        let result = validate_block(&block);
        assert!(result.has_errors());
    }

    #[test]
    fn test_unknown_tool_is_warning() {
        let mut block = make_test_block();
        block.tool_required = "hammer".into();
        let result = validate_block(&block);
        assert!(!result.has_errors());
        assert!(result.warning_count() > 0);
    }

    #[test]
    fn test_sorted_by_line() {
        let mut block = make_test_block();
        block.id = String::new();
        block.hardness = -1.0;
        let result = validate_block(&block);
        let sorted = result.sorted_by_line();
        assert!(sorted.len() >= 2);
        assert!(sorted[0].span.line <= sorted[1].span.line);
    }

    #[test]
    fn test_empty_display_name_is_warning() {
        let mut block = make_test_block();
        block.display_name = String::new();
        let result = validate_block(&block);
        assert!(!result.has_errors());
        assert!(result.warning_count() > 0);
    }

    #[test]
    fn test_validation_span_for_field() {
        let span = ValidationSpan::for_field("id");
        assert_eq!(span.line, 1);
        assert_eq!(span.field, "id");
    }
}
