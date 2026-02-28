//! `kyokara-pbt` — Property-based testing for Kyokara.
//!
//! Functions with `requires`/`ensures`/`invariant` contracts are automatically
//! testable: generate random inputs, call the function, check contracts, and
//! report failures with shrunk counterexamples.

pub mod choice;
pub mod corpus;
pub mod generate;
pub mod report;
pub mod runner;
pub mod shrink;

pub use report::{FnTestResult, TestReport, TestableKind};
pub use runner::{TestConfig, run_project_tests, run_tests};
