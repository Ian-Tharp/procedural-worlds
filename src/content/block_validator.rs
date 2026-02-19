//! Block Definition Validation
//!
//! Validates [`BlockDefinition`] values before they are applied to the
//! [`BlockRegistry`], catching errors that would otherwise cause runtime
//! failures during chunk generation or rendering.
//!
//! # Validation Checks
//!
//! - **Required fields**: `id` and `display_name` must be non-empty.
//! - **ID format**: Must contain only lowercase ASCII, digits, and underscores.
//! - **Numeric ID range**: Must be within `u16` bounds (0–65535).
//! - **Duplicate IDs**: Both string and numeric IDs must be unique within a registry.
//! - **Hardness**: Must be non-negative and finite.
//! - **Color values**: RGBA components must be in `[0.0, 1.0]` and finite.
//! - **Light level**: Must be in range `0..=15`.
//! - **Tool type**: Must be a recognized tool string.
//!
//! # Usage
//!
//! ```rust,ignore
//! let validator = BlockDefinitionValidator::new();
//! let result = validator.validate(&block_def);
//! if result.has_errors() {
//!     for issue in &result.issues {
//!         warn!("Block validation: {}", issue);
//!     }
//! }
//! ```
//!
//! For batch validation with duplicate detection:
//!
//! ```rust,ignore
//! let result = validator.validate_batch(&block_defs);
//! ```

use std::collections::{HashMap, HashSet};
use std::fmt;

use super::BlockDefinition;

// ============================================================================
// VALIDATION TYPES
// ============================================================================

/// Severity of a block validation issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The definition is invalid and must not be applied.
    Error,
    /// The definition is technically valid but has suspicious values.
    Warning,
}

/// A single validation issue found during block definition checking.
#[derive(Debug, Clone)]
pub struct BlockValidationIssue {
    /// The block ID this issue relates to (may be empty if the ID itself is invalid).
    pub block_id: String,
    /// Which field has the issue (e.g., `"id"`, `"hardness"`, `"visuals.color"`).
    pub field: &'static str,
    /// Severity level.
    pub severity: Severity,
    /// Human-readable description of what was wrong.
    pub message: String,
}

impl fmt::Display for BlockValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let severity = match self.severity {
            Severity::Error => "ERROR",
            Severity::Warning => "WARN",
        };
        if self.block_id.is_empty() {
            write!(f, "[{}] {}: {}", severity, self.field, self.message)
        } else {
            write!(
                f,
                "[{}] block '{}' field '{}': {}",
                severity, self.block_id, self.field, self.message
            )
        }
    }
}

/// Custom error type for block validation failures.
#[derive(Debug, Clone)]
pub enum ValidationError {
    /// One or more validation issues were found.
    Invalid(Vec<BlockValidationIssue>),
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationError::Invalid(issues) => {
                write!(f, "{} block validation issue(s):", issues.len())?;
                for issue in issues {
                    write!(f, "\n  - {}", issue)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// Result of validating one or more block definitions.
#[derive(Debug, Clone)]
pub struct BlockValidationResult {
    /// All issues found during validation.
    pub issues: Vec<BlockValidationIssue>,
}

impl BlockValidationResult {
    /// Returns `true` if any errors (not just warnings) were found.
    pub fn has_errors(&self) -> bool {
        self.issues.iter().any(|i| i.severity == Severity::Error)
    }

    /// Returns `true` if no issues at all were found.
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }

    /// Convert to a `Result`, returning `Err` if any errors exist.
    pub fn into_result(self) -> Result<Vec<BlockValidationIssue>, ValidationError> {
        if self.has_errors() {
            Err(ValidationError::Invalid(self.issues))
        } else {
            // Return warnings (if any) as Ok
            Ok(self.issues)
        }
    }
}

// ============================================================================
// RECOGNIZED VALUES
// ============================================================================

/// Valid tool types for `BlockDefinition::tool_required`.
const VALID_TOOLS: &[&str] = &["any", "pickaxe", "shovel", "axe"];

/// Maximum light level (Minecraft-style 0–15 range).
const MAX_LIGHT_LEVEL: u8 = 15;

/// Maximum reasonable hardness value.
const MAX_HARDNESS: f32 = 100.0;

// ============================================================================
// VALIDATOR
// ============================================================================

/// Validates block definitions for correctness before they are registered.
///
/// The validator is stateless — duplicate detection requires calling
/// [`validate_batch`](BlockDefinitionValidator::validate_batch) or
/// [`validate_against_registry`](BlockDefinitionValidator::validate_against_registry)
/// with the appropriate context.
#[derive(Debug, Clone, Default)]
pub struct BlockDefinitionValidator;

impl BlockDefinitionValidator {
    /// Create a new validator instance.
    pub fn new() -> Self {
        Self
    }

    /// Validate a single block definition in isolation.
    ///
    /// This checks field values but cannot detect duplicate IDs (that
    /// requires registry context). Use [`validate_against_registry`] or
    /// [`validate_batch`] for duplicate detection.
    pub fn validate(&self, block: &BlockDefinition) -> BlockValidationResult {
        let mut issues = Vec::new();
        self.validate_fields(block, &mut issues);
        BlockValidationResult { issues }
    }

    /// Validate a single block definition and check for duplicate IDs
    /// against existing registry contents.
    ///
    /// `existing_ids` and `existing_numeric_ids` represent the current
    /// registry state.
    pub fn validate_against_registry(
        &self,
        block: &BlockDefinition,
        existing_ids: &HashSet<String>,
        existing_numeric_ids: &HashSet<u16>,
    ) -> BlockValidationResult {
        let mut issues = Vec::new();
        self.validate_fields(block, &mut issues);
        self.check_duplicate_id(block, existing_ids, existing_numeric_ids, &mut issues);
        BlockValidationResult { issues }
    }

    /// Validate a batch of block definitions, including cross-definition
    /// duplicate detection.
    ///
    /// This is useful when loading multiple definitions at once (e.g.,
    /// from RON files during content reload).
    pub fn validate_batch(&self, blocks: &[BlockDefinition]) -> BlockValidationResult {
        let mut issues = Vec::new();
        let mut seen_ids: HashMap<String, usize> = HashMap::new();
        let mut seen_numeric_ids: HashMap<u16, usize> = HashMap::new();

        for (idx, block) in blocks.iter().enumerate() {
            // Validate individual fields
            self.validate_fields(block, &mut issues);

            // Check for duplicates within the batch
            if !block.id.is_empty() {
                if let Some(prev_idx) = seen_ids.get(&block.id) {
                    issues.push(BlockValidationIssue {
                        block_id: block.id.clone(),
                        field: "id",
                        severity: Severity::Error,
                        message: format!(
                            "duplicate string ID '{}' (first seen at index {}; duplicate at index {})",
                            block.id, prev_idx, idx
                        ),
                    });
                } else {
                    seen_ids.insert(block.id.clone(), idx);
                }
            }

            if let Some(prev_idx) = seen_numeric_ids.get(&block.numeric_id) {
                issues.push(BlockValidationIssue {
                    block_id: block.id.clone(),
                    field: "numeric_id",
                    severity: Severity::Error,
                    message: format!(
                        "duplicate numeric ID {} (first seen at index {}; duplicate at index {})",
                        block.numeric_id, prev_idx, idx
                    ),
                });
            } else {
                seen_numeric_ids.insert(block.numeric_id, idx);
            }
        }

        BlockValidationResult { issues }
    }

    // ========================================================================
    // INTERNAL VALIDATION HELPERS
    // ========================================================================

    /// Validate all fields of a single block definition.
    fn validate_fields(&self, block: &BlockDefinition, issues: &mut Vec<BlockValidationIssue>) {
        self.validate_id(block, issues);
        self.validate_display_name(block, issues);
        self.validate_hardness(block, issues);
        self.validate_tool(block, issues);
        self.validate_visuals(block, issues);
    }

    /// Validate the string ID.
    fn validate_id(&self, block: &BlockDefinition, issues: &mut Vec<BlockValidationIssue>) {
        if block.id.is_empty() {
            issues.push(BlockValidationIssue {
                block_id: String::new(),
                field: "id",
                severity: Severity::Error,
                message: "block ID must not be empty".to_string(),
            });
            return;
        }

        // ID must be lowercase alphanumeric + underscores
        if !block
            .id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            issues.push(BlockValidationIssue {
                block_id: block.id.clone(),
                field: "id",
                severity: Severity::Error,
                message: format!(
                    "block ID '{}' contains invalid characters (only lowercase a-z, 0-9, _ allowed)",
                    block.id
                ),
            });
        }

        // ID must not start with a digit
        if block.id.starts_with(|c: char| c.is_ascii_digit()) {
            issues.push(BlockValidationIssue {
                block_id: block.id.clone(),
                field: "id",
                severity: Severity::Error,
                message: format!(
                    "block ID '{}' must not start with a digit",
                    block.id
                ),
            });
        }
    }

    /// Validate the display name.
    fn validate_display_name(&self, block: &BlockDefinition, issues: &mut Vec<BlockValidationIssue>) {
        if block.display_name.trim().is_empty() {
            issues.push(BlockValidationIssue {
                block_id: block.id.clone(),
                field: "display_name",
                severity: Severity::Error,
                message: "display_name must not be empty".to_string(),
            });
        }
    }

    /// Validate hardness value.
    fn validate_hardness(&self, block: &BlockDefinition, issues: &mut Vec<BlockValidationIssue>) {
        if !block.hardness.is_finite() {
            issues.push(BlockValidationIssue {
                block_id: block.id.clone(),
                field: "hardness",
                severity: Severity::Error,
                message: format!("hardness {} is not finite", block.hardness),
            });
        } else if block.hardness < 0.0 {
            issues.push(BlockValidationIssue {
                block_id: block.id.clone(),
                field: "hardness",
                severity: Severity::Error,
                message: format!(
                    "hardness {} must be >= 0.0",
                    block.hardness
                ),
            });
        } else if block.hardness > MAX_HARDNESS {
            issues.push(BlockValidationIssue {
                block_id: block.id.clone(),
                field: "hardness",
                severity: Severity::Warning,
                message: format!(
                    "hardness {} exceeds recommended maximum of {}",
                    block.hardness, MAX_HARDNESS
                ),
            });
        }
    }

    /// Validate tool_required value.
    fn validate_tool(&self, block: &BlockDefinition, issues: &mut Vec<BlockValidationIssue>) {
        if !VALID_TOOLS.contains(&block.tool_required.as_str()) {
            issues.push(BlockValidationIssue {
                block_id: block.id.clone(),
                field: "tool_required",
                severity: Severity::Error,
                message: format!(
                    "tool_required '{}' is not recognized (valid: {})",
                    block.tool_required,
                    VALID_TOOLS.join(", ")
                ),
            });
        }
    }

    /// Validate visual properties.
    fn validate_visuals(&self, block: &BlockDefinition, issues: &mut Vec<BlockValidationIssue>) {
        // Validate color components
        for (i, &component) in block.visuals.color.iter().enumerate() {
            let channel = ["R", "G", "B", "A"][i];
            if !component.is_finite() {
                issues.push(BlockValidationIssue {
                    block_id: block.id.clone(),
                    field: "visuals.color",
                    severity: Severity::Error,
                    message: format!(
                        "color {} component {} is not finite",
                        channel, component
                    ),
                });
            } else if !(0.0..=1.0).contains(&component) {
                issues.push(BlockValidationIssue {
                    block_id: block.id.clone(),
                    field: "visuals.color",
                    severity: Severity::Warning,
                    message: format!(
                        "color {} component {} is outside [0.0, 1.0]",
                        channel, component
                    ),
                });
            }
        }

        // Validate light level
        if block.visuals.light_level > MAX_LIGHT_LEVEL {
            issues.push(BlockValidationIssue {
                block_id: block.id.clone(),
                field: "visuals.light_level",
                severity: Severity::Warning,
                message: format!(
                    "light_level {} exceeds maximum of {} — will be clamped",
                    block.visuals.light_level, MAX_LIGHT_LEVEL
                ),
            });
        }
    }

    /// Check for duplicate string or numeric IDs against existing sets.
    fn check_duplicate_id(
        &self,
        block: &BlockDefinition,
        existing_ids: &HashSet<String>,
        existing_numeric_ids: &HashSet<u16>,
        issues: &mut Vec<BlockValidationIssue>,
    ) {
        if existing_ids.contains(&block.id) {
            issues.push(BlockValidationIssue {
                block_id: block.id.clone(),
                field: "id",
                severity: Severity::Error,
                message: format!(
                    "block with ID '{}' already exists in the registry",
                    block.id
                ),
            });
        }

        if existing_numeric_ids.contains(&block.numeric_id) {
            issues.push(BlockValidationIssue {
                block_id: block.id.clone(),
                field: "numeric_id",
                severity: Severity::Error,
                message: format!(
                    "block with numeric ID {} already exists in the registry",
                    block.numeric_id
                ),
            });
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::block::{
        BlockCategory, BlockDefinition, BlockPhysics, BlockVisuals,
    };

    /// Helper: create a valid block definition for testing.
    fn valid_block() -> BlockDefinition {
        BlockDefinition {
            id: "test_stone".to_string(),
            display_name: "Test Stone".to_string(),
            block_type: None,
            numeric_id: 200,
            physics: BlockPhysics::default(),
            visuals: BlockVisuals::default(),
            hardness: 3.0,
            tool_required: "pickaxe".to_string(),
            category: BlockCategory::Natural,
        }
    }

    #[test]
    fn test_valid_block_passes() {
        let validator = BlockDefinitionValidator::new();
        let result = validator.validate(&valid_block());
        assert!(result.is_clean(), "Valid block should pass: {:?}", result.issues);
    }

    #[test]
    fn test_empty_id_fails() {
        let validator = BlockDefinitionValidator::new();
        let mut block = valid_block();
        block.id = String::new();
        let result = validator.validate(&block);
        assert!(result.has_errors());
        assert!(result.issues.iter().any(|i| i.field == "id"));
    }

    #[test]
    fn test_uppercase_id_fails() {
        let validator = BlockDefinitionValidator::new();
        let mut block = valid_block();
        block.id = "Test_Stone".to_string();
        let result = validator.validate(&block);
        assert!(result.has_errors());
        assert!(result.issues.iter().any(|i| i.field == "id" && i.severity == Severity::Error));
    }

    #[test]
    fn test_id_starting_with_digit_fails() {
        let validator = BlockDefinitionValidator::new();
        let mut block = valid_block();
        block.id = "1stone".to_string();
        let result = validator.validate(&block);
        assert!(result.has_errors());
    }

    #[test]
    fn test_empty_display_name_fails() {
        let validator = BlockDefinitionValidator::new();
        let mut block = valid_block();
        block.display_name = "  ".to_string();
        let result = validator.validate(&block);
        assert!(result.has_errors());
        assert!(result.issues.iter().any(|i| i.field == "display_name"));
    }

    #[test]
    fn test_negative_hardness_fails() {
        let validator = BlockDefinitionValidator::new();
        let mut block = valid_block();
        block.hardness = -1.0;
        let result = validator.validate(&block);
        assert!(result.has_errors());
        assert!(result.issues.iter().any(|i| i.field == "hardness"));
    }

    #[test]
    fn test_nan_hardness_fails() {
        let validator = BlockDefinitionValidator::new();
        let mut block = valid_block();
        block.hardness = f32::NAN;
        let result = validator.validate(&block);
        assert!(result.has_errors());
    }

    #[test]
    fn test_extreme_hardness_warns() {
        let validator = BlockDefinitionValidator::new();
        let mut block = valid_block();
        block.hardness = 200.0;
        let result = validator.validate(&block);
        assert!(!result.has_errors());
        assert!(!result.is_clean());
        assert!(result.issues.iter().any(|i| i.field == "hardness" && i.severity == Severity::Warning));
    }

    #[test]
    fn test_invalid_tool_fails() {
        let validator = BlockDefinitionValidator::new();
        let mut block = valid_block();
        block.tool_required = "laser".to_string();
        let result = validator.validate(&block);
        assert!(result.has_errors());
        assert!(result.issues.iter().any(|i| i.field == "tool_required"));
    }

    #[test]
    fn test_color_out_of_range_warns() {
        let validator = BlockDefinitionValidator::new();
        let mut block = valid_block();
        block.visuals.color = [1.5, 0.0, 0.0, 1.0];
        let result = validator.validate(&block);
        assert!(!result.has_errors());
        assert!(!result.is_clean());
        assert!(result.issues.iter().any(|i| i.field == "visuals.color"));
    }

    #[test]
    fn test_nan_color_fails() {
        let validator = BlockDefinitionValidator::new();
        let mut block = valid_block();
        block.visuals.color = [f32::NAN, 0.0, 0.0, 1.0];
        let result = validator.validate(&block);
        assert!(result.has_errors());
    }

    #[test]
    fn test_high_light_level_warns() {
        let validator = BlockDefinitionValidator::new();
        let mut block = valid_block();
        block.visuals.light_level = 20;
        let result = validator.validate(&block);
        assert!(!result.has_errors());
        assert!(result.issues.iter().any(|i| i.field == "visuals.light_level"));
    }

    #[test]
    fn test_duplicate_id_in_batch() {
        let validator = BlockDefinitionValidator::new();
        let mut block_a = valid_block();
        block_a.id = "stone".to_string();
        block_a.numeric_id = 1;

        let mut block_b = valid_block();
        block_b.id = "stone".to_string(); // duplicate!
        block_b.numeric_id = 2;

        let result = validator.validate_batch(&[block_a, block_b]);
        assert!(result.has_errors());
        assert!(result
            .issues
            .iter()
            .any(|i| i.field == "id" && i.message.contains("duplicate")));
    }

    #[test]
    fn test_duplicate_numeric_id_in_batch() {
        let validator = BlockDefinitionValidator::new();
        let mut block_a = valid_block();
        block_a.id = "stone_a".to_string();
        block_a.numeric_id = 42;

        let mut block_b = valid_block();
        block_b.id = "stone_b".to_string();
        block_b.numeric_id = 42; // duplicate!

        let result = validator.validate_batch(&[block_a, block_b]);
        assert!(result.has_errors());
        assert!(result
            .issues
            .iter()
            .any(|i| i.field == "numeric_id" && i.message.contains("duplicate")));
    }

    #[test]
    fn test_duplicate_id_against_registry() {
        let validator = BlockDefinitionValidator::new();
        let block = valid_block();

        let mut existing_ids = HashSet::new();
        existing_ids.insert("test_stone".to_string()); // already registered

        let existing_numeric_ids = HashSet::new();

        let result = validator.validate_against_registry(&block, &existing_ids, &existing_numeric_ids);
        assert!(result.has_errors());
        assert!(result
            .issues
            .iter()
            .any(|i| i.field == "id" && i.message.contains("already exists")));
    }

    #[test]
    fn test_zero_hardness_is_valid() {
        let validator = BlockDefinitionValidator::new();
        let mut block = valid_block();
        block.hardness = 0.0; // Valid for air-like blocks
        let result = validator.validate(&block);
        assert!(result.is_clean());
    }

    #[test]
    fn test_valid_batch_passes() {
        let validator = BlockDefinitionValidator::new();
        let mut block_a = valid_block();
        block_a.id = "stone_a".to_string();
        block_a.numeric_id = 100;

        let mut block_b = valid_block();
        block_b.id = "stone_b".to_string();
        block_b.numeric_id = 101;

        let result = validator.validate_batch(&[block_a, block_b]);
        assert!(result.is_clean());
    }

    #[test]
    fn test_into_result_ok_with_warnings() {
        let validator = BlockDefinitionValidator::new();
        let mut block = valid_block();
        block.hardness = 200.0; // Warning only
        let result = validator.validate(&block);
        assert!(!result.has_errors());
        let warnings = result.into_result().expect("should be Ok with warnings");
        assert!(!warnings.is_empty());
    }

    #[test]
    fn test_into_result_err_with_errors() {
        let validator = BlockDefinitionValidator::new();
        let mut block = valid_block();
        block.id = String::new(); // Error
        let result = validator.validate(&block);
        assert!(result.into_result().is_err());
    }

    #[test]
    fn test_display_format() {
        let issue = BlockValidationIssue {
            block_id: "iron_ore".to_string(),
            field: "hardness",
            severity: Severity::Error,
            message: "hardness -1 must be >= 0.0".to_string(),
        };
        let formatted = format!("{}", issue);
        assert!(formatted.contains("ERROR"));
        assert!(formatted.contains("iron_ore"));
        assert!(formatted.contains("hardness"));
    }

    #[test]
    fn test_multiple_issues_collected() {
        let validator = BlockDefinitionValidator::new();
        let mut block = valid_block();
        block.id = String::new();
        block.display_name = String::new();
        block.hardness = -5.0;
        block.tool_required = "blaster".to_string();
        let result = validator.validate(&block);
        assert!(result.issues.len() >= 4, "Expected at least 4 issues, got {}: {:?}", result.issues.len(), result.issues);
    }
}
