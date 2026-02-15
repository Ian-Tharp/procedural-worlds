//! Validation Display Component
//!
//! Renders inline validation errors, warnings, and info messages
//! in the block content editor with severity coloring and hover tooltips.

use bevy_egui::egui;

use crate::content::block_validator::{ValidationResult, ValidationSeverity, ValidationIssue};

/// Colors for each severity level
fn severity_color(severity: ValidationSeverity) -> egui::Color32 {
    match severity {
        ValidationSeverity::Error => egui::Color32::from_rgb(255, 80, 80),
        ValidationSeverity::Warning => egui::Color32::from_rgb(255, 200, 60),
        ValidationSeverity::Info => egui::Color32::from_rgb(80, 160, 255),
    }
}

/// Icon for each severity level
fn severity_icon(severity: ValidationSeverity) -> &'static str {
    match severity {
        ValidationSeverity::Error => "❌",
        ValidationSeverity::Warning => "⚠",
        ValidationSeverity::Info => "ℹ",
    }
}

/// Draw a compact validation summary bar (e.g., "2 errors, 1 warning")
pub fn draw_validation_summary(ui: &mut egui::Ui, result: &ValidationResult) {
    if !result.has_issues() {
        ui.colored_label(egui::Color32::from_rgb(80, 200, 80), "✓ Valid");
        return;
    }

    ui.horizontal(|ui| {
        let errors = result.error_count();
        let warnings = result.warning_count();
        let infos = result.issues.len() - errors - warnings;

        if errors > 0 {
            ui.colored_label(
                severity_color(ValidationSeverity::Error),
                format!("❌ {} error{}", errors, if errors == 1 { "" } else { "s" }),
            );
        }
        if warnings > 0 {
            ui.colored_label(
                severity_color(ValidationSeverity::Warning),
                format!("⚠ {} warning{}", warnings, if warnings == 1 { "" } else { "s" }),
            );
        }
        if infos > 0 {
            ui.colored_label(
                severity_color(ValidationSeverity::Info),
                format!("ℹ {} info", infos),
            );
        }
    });
}

/// Draw the full validation issues panel with gutter markers
pub fn draw_validation_panel(ui: &mut egui::Ui, result: &ValidationResult) {
    if !result.has_issues() {
        return;
    }

    ui.separator();
    ui.label(egui::RichText::new("Validation").strong());

    egui::ScrollArea::vertical()
        .id_salt("validation_issues")
        .max_height(150.0)
        .show(ui, |ui| {
            for issue in result.sorted_by_line() {
                draw_issue_row(ui, issue);
            }
        });
}

/// Draw a single issue row with gutter marker and hover tooltip
fn draw_issue_row(ui: &mut egui::Ui, issue: &ValidationIssue) {
    let color = severity_color(issue.severity);
    let icon = severity_icon(issue.severity);

    let response = ui.horizontal(|ui| {
        // Gutter marker (line number)
        if issue.span.line > 0 {
            ui.colored_label(
                egui::Color32::from_rgb(120, 120, 120),
                format!("L{}", issue.span.line),
            );
        }

        // Severity icon + field name
        ui.colored_label(color, format!("{} {}", icon, issue.span.field));

        // Message
        ui.label(&issue.message);
    });

    // Hover tooltip with suggestion
    if let Some(ref suggestion) = issue.suggestion {
        response.response.on_hover_ui(|ui| {
            ui.colored_label(color, format!("{} {}", icon, issue.message));
            ui.separator();
            ui.label(format!("💡 {}", suggestion));
            ui.separator();
            ui.colored_label(
                egui::Color32::from_rgb(120, 120, 120),
                format!("Field: {} (line {})", issue.span.field, issue.span.line),
            );
        });
    }
}

/// Draw inline validation marker next to a specific field.
/// Call this right after drawing a field's editor widget.
/// Returns true if there was an issue for this field.
pub fn draw_field_marker(ui: &mut egui::Ui, result: &ValidationResult, field: &str) -> bool {
    let field_issues: Vec<_> = result.issues.iter()
        .filter(|i| i.span.field == field)
        .collect();

    if field_issues.is_empty() {
        return false;
    }

    // Show the highest severity marker
    let worst = field_issues.iter()
        .map(|i| i.severity)
        .max()
        .unwrap_or(ValidationSeverity::Info);

    let icon = severity_icon(worst);
    let color = severity_color(worst);

    let response = ui.colored_label(color, icon);

    // Hover tooltip with all issues for this field
    response.on_hover_ui(|ui| {
        for issue in &field_issues {
            ui.colored_label(severity_color(issue.severity), 
                format!("{} {}", severity_icon(issue.severity), issue.message));
            if let Some(ref suggestion) = issue.suggestion {
                ui.label(format!("  💡 {}", suggestion));
            }
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
        let error = severity_color(ValidationSeverity::Error);
        let warning = severity_color(ValidationSeverity::Warning);
        let info = severity_color(ValidationSeverity::Info);
        assert_ne!(error, warning);
        assert_ne!(warning, info);
        assert_ne!(error, info);
    }

    #[test]
    fn test_severity_icon_returns_nonempty() {
        assert!(!severity_icon(ValidationSeverity::Error).is_empty());
        assert!(!severity_icon(ValidationSeverity::Warning).is_empty());
        assert!(!severity_icon(ValidationSeverity::Info).is_empty());
    }
}
