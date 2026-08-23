//! The shape every check reports in, so one `just ci` prints one kind of
//! report whether the finding came from verify, the manifest check, the
//! audit or — later — the checks that need an engine.

use std::fmt;

/// How bad a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Said out loud, means nothing is wrong. A body the audit skips because
    /// it has no recipe is this: the skip is the rule, and a rule applied
    /// silently is a rule nobody can see applied.
    Note,
    /// Worth knowing, does not mean anything is broken. A sound with no
    /// sidecar is the library's known backfill debt, not a fault.
    Warning,
    /// The library is not what it claims to be.
    Failure,
}

impl Severity {
    /// The four-letter mark a report line starts with.
    #[must_use]
    pub const fn mark(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Warning => "WARN",
            Self::Failure => "FAIL",
        }
    }
}

/// One thing that is not as it should be — or, for a note, one thing that
/// was deliberately left alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// How bad.
    pub severity: Severity,
    /// What it is about — an asset name, a file, a profile.
    pub subject: String,
    /// What is wrong, in one line.
    pub detail: String,
}

/// The outcome of a set of checks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// How many things were looked at.
    pub checked: usize,
    /// What was found, in the order it was found.
    pub findings: Vec<Finding>,
}

impl Report {
    /// Whether anything failed. Warnings and notes do not count.
    #[must_use]
    pub fn ok(&self) -> bool {
        !self
            .findings
            .iter()
            .any(|f| f.severity == Severity::Failure)
    }

    /// How many failures.
    #[must_use]
    pub fn failures(&self) -> usize {
        self.count(Severity::Failure)
    }

    /// How many warnings.
    #[must_use]
    pub fn warnings(&self) -> usize {
        self.count(Severity::Warning)
    }

    fn count(&self, severity: Severity) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == severity)
            .count()
    }

    /// Fold another report into this one.
    pub fn absorb(&mut self, other: Self) {
        self.checked += other.checked;
        self.findings.extend(other.findings);
    }

    /// Record a failure: the library is not what it claims to be.
    ///
    /// Public because the checks that need an engine live outside this crate —
    /// the studio's rig check binds clips and reads weights — and they report
    /// into the same shape so one `just ci` prints one kind of report.
    pub fn fail(&mut self, subject: impl Into<String>, detail: impl Into<String>) {
        self.push(Severity::Failure, subject, detail);
    }

    /// Record something worth knowing that breaks nothing.
    pub fn warn(&mut self, subject: impl Into<String>, detail: impl Into<String>) {
        self.push(Severity::Warning, subject, detail);
    }

    /// Record a deliberate skip or an observation, out loud.
    pub fn note(&mut self, subject: impl Into<String>, detail: impl Into<String>) {
        self.push(Severity::Note, subject, detail);
    }

    fn push(&mut self, severity: Severity, subject: impl Into<String>, detail: impl Into<String>) {
        self.findings.push(Finding {
            severity,
            subject: subject.into(),
            detail: detail.into(),
        });
    }

    /// Every finding as a line — `FAIL <subject>  <detail>` — followed by the
    /// summary: `N checked, ok` or `N checked, F failed, W warning(s)`.
    #[must_use]
    pub fn render(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for finding in &self.findings {
            writeln!(
                f,
                "{} {:<24} {}",
                finding.severity.mark(),
                finding.subject,
                finding.detail
            )?;
        }
        write!(f, "{} checked", self.checked)?;
        let (failures, warnings) = (self.failures(), self.warnings());
        if failures == 0 && warnings == 0 {
            write!(f, ", ok")
        } else {
            if failures > 0 {
                write!(f, ", {failures} failed")?;
            }
            if warnings > 0 {
                write!(f, ", {warnings} warning(s)")?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_failures_count_against_ok() {
        let mut report = Report {
            checked: 2,
            ..Report::default()
        };
        report.note("a", "skipped on purpose");
        report.warn("b", "no sidecar");
        assert!(report.ok());
        assert_eq!(report.warnings(), 1);
        assert!(report.render().ends_with("2 checked, 1 warning(s)"));
        report.fail("c", "hash mismatch");
        assert!(!report.ok());
        assert_eq!(report.failures(), 1);
        let text = report.render();
        assert!(text.starts_with("note a"), "{text}");
        assert!(text.contains("\nFAIL c"), "{text}");
        assert!(
            text.ends_with("2 checked, 1 failed, 1 warning(s)"),
            "{text}"
        );
    }

    #[test]
    fn a_clean_report_says_ok() {
        let report = Report {
            checked: 3,
            ..Report::default()
        };
        assert_eq!(report.render(), "3 checked, ok");
    }
}
