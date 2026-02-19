//! Integration tests for block editor validation feedback

use procedural_worlds::content::block::{BlockDefinition, BlockPhysics, BlockVisuals, BlockCategory};
use procedural_worlds::content::block_validator::*;

fn test_block() -> BlockDefinition {
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
fn test_valid_block_produces_no_errors() {
    let block = test_block();
    let result = validate_block(&block);
    assert!(result.issues.is_empty(), "Valid block should produce no errors: {:?}", result.issues);
}

#[test]
fn test_empty_id_detected() {
    let mut block = test_block();
    block.id = String::new();
    let result = validate_block(&block);
    assert!(result.has_errors());
    assert!(result.issues.iter().any(|i| i.field == "id"));
}

#[test]
fn test_whitespace_id_detected() {
    let mut block = test_block();
    block.id = "bad block".into();
    let result = validate_block(&block);
    assert!(result.has_errors());
}

#[test]
fn test_special_char_id_detected() {
    let mut block = test_block();
    block.id = "block@#$".into();
    let result = validate_block(&block);
    assert!(result.has_errors());
}

#[test]
fn test_negative_hardness_detected() {
    let mut block = test_block();
    block.hardness = -5.0;
    let result = validate_block(&block);
    assert!(result.has_errors());
    assert!(result.issues.iter().any(|i| i.field == "hardness"));
}

#[test]
fn test_out_of_range_color_detected() {
    let mut block = test_block();
    block.visuals.color = [2.0, -0.5, 0.5, 1.0];
    let result = validate_block(&block);
    assert!(result.has_errors());
    let error_count = result.issues.iter().filter(|i| i.severity == Severity::Error).count();
    assert_eq!(error_count, 2); // R and G both out of range
}

#[test]
fn test_high_light_level_warning() {
    let mut block = test_block();
    block.visuals.light_level = 20;
    let result = validate_block(&block);
    let warning_count = result.issues.iter().filter(|i| i.severity == Severity::Warning).count();
    assert!(warning_count > 0);
    assert!(result.issues.iter().any(|i| i.field == "visuals.light_level"));
}

#[test]
fn test_solid_passable_warning() {
    let mut block = test_block();
    block.physics.solid = true;
    block.physics.passable = true;
    let result = validate_block(&block);
    let warning_count = result.issues.iter().filter(|i| i.severity == Severity::Warning).count();
    assert!(warning_count > 0);
}

#[test]
fn test_unknown_tool_warning() {
    let mut block = test_block();
    block.tool_required = "laser".into();
    let result = validate_block(&block);
    let warning_count = result.issues.iter().filter(|i| i.severity == Severity::Warning).count();
    assert!(warning_count > 0);
}

#[test]
fn test_multiple_issues_collected() {
    let mut block = test_block();
    block.id = String::new();
    block.hardness = -1.0;
    block.visuals.color = [2.0, 0.0, 0.0, 1.0];
    let result = validate_block(&block);
    assert!(result.issues.len() >= 3, "Expected at least 3 issues, got {}", result.issues.len());
}
