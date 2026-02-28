//! Test report types and formatting.

use crate::choice::ChoiceSequence;

/// Result of testing a single function.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FnTestResult {
    /// Function name.
    pub name: String,
    /// Number of test cases that passed.
    pub passed: usize,
    /// Number of test cases discarded (precondition failed).
    pub discarded: usize,
    /// Total test cases attempted.
    pub total: usize,
    /// Failure info, if any.
    pub failure: Option<FailureInfo>,
}

/// Details about a test failure.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FailureInfo {
    /// The error message.
    pub error: String,
    /// Displayable argument values for the counterexample.
    pub args_display: Vec<String>,
    /// The shrunk choice sequence (for replay/corpus).
    pub choices: ChoiceSequence,
}

/// Overall test report for all functions in a file.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TestReport {
    /// Per-function results.
    pub results: Vec<FnTestResult>,
    /// Functions that were skipped (no contracts, ungeneratable types, etc.).
    pub skipped: Vec<String>,
}

impl TestReport {
    /// Whether all tested functions passed.
    pub fn all_passed(&self) -> bool {
        self.results.iter().all(|r| r.failure.is_none())
    }

    /// Number of failures.
    pub fn failure_count(&self) -> usize {
        self.results.iter().filter(|r| r.failure.is_some()).count()
    }

    /// Format as human-readable text.
    pub fn format_human(&self) -> String {
        let mut out = String::new();

        for result in &self.results {
            if let Some(ref failure) = result.failure {
                out.push_str(&format!("FAIL {}\n", result.name));
                out.push_str(&format!("  error: {}\n", failure.error));
                out.push_str(&format!(
                    "  counterexample: ({})\n",
                    failure.args_display.join(", ")
                ));
                out.push_str(&format!(
                    "  ({} passed, {} discarded out of {})\n",
                    result.passed, result.discarded, result.total
                ));
            } else {
                out.push_str(&format!(
                    "ok   {} ({} passed, {} discarded)\n",
                    result.name, result.passed, result.discarded
                ));
            }
        }

        if !self.skipped.is_empty() {
            out.push_str(&format!(
                "skip {} (no testable contracts)\n",
                self.skipped.join(", ")
            ));
        }

        let total_fns = self.results.len();
        let failures = self.failure_count();
        if failures == 0 {
            out.push_str(&format!("\n{total_fns} function(s) tested, all passed.\n"));
        } else {
            out.push_str(&format!(
                "\n{total_fns} function(s) tested, {failures} failure(s).\n"
            ));
        }

        out
    }

    /// Format as JSON.
    pub fn format_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
}
