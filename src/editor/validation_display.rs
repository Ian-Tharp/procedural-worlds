//! Validation Display Component
//!
//! Renders inline validation errors and warnings in the block content editor
//! with severity coloring and hover tooltips. Adapts the [`BlockValidationResult`]
//! from the content validator into egui widgets.

use bevy_egui::egui;

use crate::content::block_validator::{BlockValidationResult, Severity, BlockValidationIssue};

/// Colors for each severity level
fn severity_color(severity: Severity) -> egui::Color32 {
    match severity {
        Severity::Error => egui::Color32::from_rgb(255, 80, 80),
        Severity::Warning => egui::Color32::from_rgb(255, 200, 60),
    }
}

/// Icon for each severity level
fn severity_icon(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "❌",
        Severity::Warning => "⚠",
    }
}

/// Draw a compact validation summary bar (e.g., "2 errors, 1 warning")
pub fn draw_validation_summary(ui: &mut egui::Ui, result: &BlockValidationResult) {
    if result.issues.is_empty() {
        ui.colored_label(egui::Color32::from_rgb(80, 200, 80), "✓ Valid");
        return;
    }

    ui.horizontal(|ui| {
        let errors = result.issues.iter().filter(|i| i.severity == Severity::Error).count();
        let warnings = result.issues.iter().filter(|i| i.severity == Severity::Warning).count();

        if errors > 0 {
            ui.colored_label(
                severity_color(Severity::Error),
                format!("❌ {} error{}", errors, if errors == 1 { "" } else { "s" }),
            );
        }
        if warnings > 0 {
            ui.colored_label(
                severity_color(Severity::Warning),
                format!("⚠ {} warning{}", warnings, if warnings == 1 { "" } else { "s" }),
            );
        }
    });
}

/// Draw the full validation issues panel
pub fn draw_validation_panel(ui: &mut egui::Ui, result: &BlockValidationResult) {
    if result.issues.is_empty() {
        return;
    }

    ui.separator();
    ui.label(egui::RichText::new("Validation").strong());

    egui::ScrollArea::vertical()
        .id_salt("validation_issues")
        .max_height(150.0)
        .show(ui, |ui| {
            for issue in &result.issues {
                draw_issue_row(ui, issue);
            }
        });
}

/// Draw a single issue row with severity icon and hover tooltip
fn draw_issue_row(ui: &mut egui::Ui, issue: &BlockValidationIssue) {
    let color = severity_color(issue.severity);
    let icon = severity_icon(issue.severity);

    let response = ui.horizontal(|ui| {
        // Severity icon + field name
        ui.colored_label(color, format!("{} [{}]", icon, issue.field));

        // Message
        ui.label(&issue.message);
    });

    // Hover tooltip with details
    response.response.on_hover_ui(|ui| {
        ui.colored_label(color, format!("{} {}", icon, issue.message));
        if !issue.block_id.is_empty() {
            ui.separator();
            ui.colored_label(
                egui::Color32::from_rgb(120, 120, 120),
                format!("Block: {}, Field: {}", issue.block_id, issue.field),
            );
        }
    });
}

/// Draw inline validation marker next to a specific field.
/// Call this right after drawing a field's editor widget.
/// Returns true if there was an issue for this field.
pub fn draw_field_marker(ui: &mut egui::Ui, result: &BlockValidationResult, field: &str) -> bool {
    let field_issues: Vec<_> = result.issues.iter()
        .filter(|i| i.field == field)
        .collect();

    if field_issues.is_empty() {
        return false;
    }

    // Show the highest severity marker (Error > Warning)
    let worst = if field_issues.iter().any(|i| i.severity == Severity::Error) {
        Severity::Error
    } else {
        Severity::Warning
    };

    let icon = severity_icon(worst);
    let color = severity_color(worst);

    let response = ui.colored_label(color, icon);

    // Hover tooltip with all issues for this field
    response.on_hover_ui(|ui| {
        for issue in &field_issues {
            ui.colored_label(severity_color(issue.severity),
                format!("{} {}", severity_icon(issue.severity), issue.message));
        }
    });

    true
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_color_returns_distinct_colors() {
        let error = severity_color(Severity::Error);
        let warning = severity_color(Severity::Warning);
        assert_ne!(error, warning);
    }

    #[test]
    fn test_severity_icon_returns_nonempty() {
        assert!(!severity_icon(Severity::Error).is_empty());
        assert!(!severity_icon(Severity::Warning).is_empty());
    }
}
