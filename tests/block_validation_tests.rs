//! Integration tests for block definition validation.
//!
//! These tests verify the BlockDefinitionValidator catches invalid block
//! definitions before they can enter the registry and cause runtime failures
//! during chunk generation.

use std::collections::HashSet;

use procedural_worlds::content::block::{
    BlockCategory, BlockDefinition, BlockPhysics, BlockVisuals,
};
use procedural_worlds::content::block_validator::{
    BlockDefinitionValidator, Severity,
};

// ============================================================================
// HELPERS
// ============================================================================

/// Create a valid block definition for testing.
fn valid_block(id: &str, numeric_id: u16) -> BlockDefinition {
    BlockDefinition {
        id: id.to_string(),
        display_name: format!("Test {}", id),
        block_type: None,
        numeric_id,
        physics: BlockPhysics::default(),
        visuals: BlockVisuals::default(),
        hardness: 1.0,
        tool_required: "any".to_string(),
        category: BlockCategory::Natural,
    }
}

// ============================================================================
// SINGLE BLOCK VALIDATION
// ============================================================================

#[test]
fn valid_block_config_passes() {
    let validator = BlockDefinitionValidator::new();
    let block = valid_block("custom_stone", 200);
    let result = validator.validate(&block);
    assert!(
        result.is_clean(),
        "A valid block should produce no issues: {:?}",
        result.issues
    );
}

#[test]
fn missing_id_fails() {
    let validator = BlockDefinitionValidator::new();
    let mut block = valid_block("", 200);
    block.id = String::new();
    let result = validator.validate(&block);
    assert!(result.has_errors(), "Empty ID should be an error");
    assert!(result.issues.iter().any(|i| i.field == "id"));
}

#[test]
fn missing_display_name_fails() {
    let validator = BlockDefinitionValidator::new();
    let mut block = valid_block("custom_stone", 200);
    block.display_name = String::new();
    let result = validator.validate(&block);
    assert!(result.has_errors(), "Empty display_name should be an error");
    assert!(result.issues.iter().any(|i| i.field == "display_name"));
}

#[test]
fn invalid_bounds_zero_hardness_passes() {
    // Zero hardness is valid (air-like blocks have 0 hardness)
    let validator = BlockDefinitionValidator::new();
    let mut block = valid_block("air_block", 200);
    block.hardness = 0.0;
    let result = validator.validate(&block);
    assert!(
        result.is_clean(),
        "Zero hardness should be valid for air-like blocks"
    );
}

#[test]
fn negative_hardness_fails() {
    let validator = BlockDefinitionValidator::new();
    let mut block = valid_block("broken_block", 200);
    block.hardness = -1.0;
    let result = validator.validate(&block);
    assert!(result.has_errors());
    assert!(result
        .issues
        .iter()
        .any(|i| i.field == "hardness" && i.severity == Severity::Error));
}

#[test]
fn nan_hardness_fails() {
    let validator = BlockDefinitionValidator::new();
    let mut block = valid_block("nan_block", 200);
    block.hardness = f32::NAN;
    let result = validator.validate(&block);
    assert!(result.has_errors());
}

#[test]
fn infinite_hardness_fails() {
    let validator = BlockDefinitionValidator::new();
    let mut block = valid_block("inf_block", 200);
    block.hardness = f32::INFINITY;
    let result = validator.validate(&block);
    assert!(result.has_errors());
}

#[test]
fn invalid_color_nan_fails() {
    let validator = BlockDefinitionValidator::new();
    let mut block = valid_block("bad_color", 200);
    block.visuals.color = [f32::NAN, 0.5, 0.5, 1.0];
    let result = validator.validate(&block);
    assert!(result.has_errors(), "NaN color should be an error");
}

#[test]
fn color_out_of_range_warns() {
    let validator = BlockDefinitionValidator::new();
    let mut block = valid_block("bright_block", 200);
    block.visuals.color = [2.0, 0.5, 0.5, 1.0];
    let result = validator.validate(&block);
    assert!(!result.has_errors(), "Out-of-range color is a warning, not error");
    assert!(!result.is_clean());
    assert!(result
        .issues
        .iter()
        .any(|i| i.field == "visuals.color" && i.severity == Severity::Warning));
}

#[test]
fn invalid_tool_type_fails() {
    let validator = BlockDefinitionValidator::new();
    let mut block = valid_block("laser_block", 200);
    block.tool_required = "laser_cannon".to_string();
    let result = validator.validate(&block);
    assert!(result.has_errors());
    assert!(result.issues.iter().any(|i| i.field == "tool_required"));
}

// ============================================================================
// DUPLICATE DETECTION
// ============================================================================

#[test]
fn duplicate_id_detected_in_batch() {
    let validator = BlockDefinitionValidator::new();
    let block_a = valid_block("custom_ore", 100);
    let block_b = valid_block("custom_ore", 101); // same string ID, different numeric

    let result = validator.validate_batch(&[block_a, block_b]);
    assert!(result.has_errors());
    assert!(result
        .issues
        .iter()
        .any(|i| i.field == "id" && i.message.contains("duplicate")));
}

#[test]
fn duplicate_numeric_id_detected_in_batch() {
    let validator = BlockDefinitionValidator::new();
    let block_a = valid_block("ore_a", 100);
    let block_b = valid_block("ore_b", 100); // different string ID, same numeric

    let result = validator.validate_batch(&[block_a, block_b]);
    assert!(result.has_errors());
    assert!(result
        .issues
        .iter()
        .any(|i| i.field == "numeric_id" && i.message.contains("duplicate")));
}

#[test]
fn duplicate_id_against_registry() {
    let validator = BlockDefinitionValidator::new();
    let block = valid_block("stone", 200); // "stone" already exists in typical registry

    let mut existing_ids = HashSet::new();
    existing_ids.insert("stone".to_string());
    let existing_numeric_ids = HashSet::new();

    let result =
        validator.validate_against_registry(&block, &existing_ids, &existing_numeric_ids);
    assert!(result.has_errors());
    assert!(result
        .issues
        .iter()
        .any(|i| i.field == "id" && i.message.contains("already exists")));
}

// ============================================================================
// VALIDATION RESULT API
// ============================================================================

#[test]
fn validation_result_into_result_ok() {
    let validator = BlockDefinitionValidator::new();
    let block = valid_block("good_block", 200);
    let result = validator.validate(&block);
    assert!(result.into_result().is_ok());
}

#[test]
fn validation_result_into_result_err() {
    let validator = BlockDefinitionValidator::new();
    let mut block = valid_block("", 200);
    block.id = String::new();
    let result = validator.validate(&block);
    let err = result.into_result();
    assert!(err.is_err());
    let error = err.unwrap_err();
    let msg = format!("{}", error);
    assert!(msg.contains("validation issue"));
}

#[test]
fn warnings_do_not_cause_error_result() {
    let validator = BlockDefinitionValidator::new();
    let mut block = valid_block("warning_block", 200);
    block.hardness = 200.0; // Warning: exceeds recommended max
    let result = validator.validate(&block);
    assert!(!result.has_errors());
    assert!(!result.is_clean());
    // into_result should be Ok, containing the warnings
    let warnings = result.into_result().expect("warnings should not be Err");
    assert!(!warnings.is_empty());
}

// ============================================================================
// BATCH VALIDATION
// ============================================================================

#[test]
fn valid_batch_passes() {
    let validator = BlockDefinitionValidator::new();
    let blocks = vec![
        valid_block("ore_a", 100),
        valid_block("ore_b", 101),
        valid_block("ore_c", 102),
    ];
    let result = validator.validate_batch(&blocks);
    assert!(result.is_clean());
}

#[test]
fn batch_with_mixed_errors_and_valid() {
    let validator = BlockDefinitionValidator::new();
    let mut bad_block = valid_block("", 100);
    bad_block.id = String::new(); // Error: empty ID
    let good_block = valid_block("good_ore", 101);

    let result = validator.validate_batch(&[bad_block, good_block]);
    assert!(result.has_errors());
    // Should have exactly the issues from the bad block
    assert!(result.issues.iter().any(|i| i.field == "id"));
}

// ============================================================================
// EDGE CASES
// ============================================================================

#[test]
fn id_with_spaces_fails() {
    let validator = BlockDefinitionValidator::new();
    let mut block = valid_block("test stone", 200);
    block.id = "test stone".to_string();
    let result = validator.validate(&block);
    assert!(result.has_errors());
}

#[test]
fn id_with_hyphens_fails() {
    let validator = BlockDefinitionValidator::new();
    let mut block = valid_block("test-stone", 200);
    block.id = "test-stone".to_string();
    let result = validator.validate(&block);
    assert!(result.has_errors());
}

#[test]
fn all_valid_tool_types_accepted() {
    let validator = BlockDefinitionValidator::new();
    for tool in &["any", "pickaxe", "shovel", "axe"] {
        let mut block = valid_block("test_block", 200);
        block.tool_required = tool.to_string();
        let result = validator.validate(&block);
        assert!(
            !result.has_errors(),
            "Tool type '{}' should be valid",
            tool
        );
    }
}

#[test]
fn extreme_color_values_warn() {
    let validator = BlockDefinitionValidator::new();
    let mut block = valid_block("neon_block", 200);
    block.visuals.color = [5.0, -1.0, 0.5, 1.0]; // R too high, G negative
    let result = validator.validate(&block);
    // Should have warnings for both out-of-range components
    let color_warnings: Vec<_> = result
        .issues
        .iter()
        .filter(|i| i.field == "visuals.color" && i.severity == Severity::Warning)
        .collect();
    assert!(
        color_warnings.len() >= 2,
        "Expected at least 2 color warnings, got {}",
        color_warnings.len()
    );
}

#[test]
fn light_level_max_is_valid() {
    let validator = BlockDefinitionValidator::new();
    let mut block = valid_block("glowstone", 200);
    block.visuals.light_level = 15;
    let result = validator.validate(&block);
    assert!(result.is_clean());
}

#[test]
fn light_level_above_max_warns() {
    let validator = BlockDefinitionValidator::new();
    let mut block = valid_block("super_glow", 200);
    block.visuals.light_level = 16;
    let result = validator.validate(&block);
    assert!(!result.is_clean());
    assert!(result
        .issues
        .iter()
        .any(|i| i.field == "visuals.light_level"));
}
