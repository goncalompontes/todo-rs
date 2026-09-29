//! Ariadne-based diagnostic rendering.

use ariadne::{Color, Config, IndexType, Label, Report, ReportKind, Source};

use crate::parse::{Issue, Severity};

/// Render a set of issues against `source` (the whole file), returning the
/// ANSI-coloured report as a string.
pub fn render(file: &str, source: &str, issues: &[Issue]) -> String {
    let mut out: Vec<u8> = Vec::new();
    for issue in issues {
        let kind = match issue.severity {
            Severity::Error => ReportKind::Error,
            Severity::Warning => ReportKind::Warning,
        };
        let color = match issue.severity {
            Severity::Error => Color::Red,
            Severity::Warning => Color::Yellow,
        };
        let range = issue.span.clone();
        let report = Report::build(kind, (file, range.clone()))
            .with_config(Config::new().with_index_type(IndexType::Byte))
            .with_message(&issue.message)
            .with_label(
                Label::new((file, range))
                    .with_message(&issue.message)
                    .with_color(color),
            )
            .finish();
        // Writing to a Vec cannot fail.
        let _ = report.write((file, Source::from(source)), &mut out);
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Number of issues at `Error` severity.
pub fn error_count(issues: &[Issue]) -> usize {
    issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .count()
}
