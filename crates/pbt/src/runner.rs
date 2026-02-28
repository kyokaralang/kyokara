//! Test runner — discovery, generation loop, shrinking integration.

use kyokara_eval::error::RuntimeError;
use kyokara_eval::interpreter::Interpreter;
use kyokara_eval::intrinsics::Args;
use kyokara_hir::{
    check_module, check_project, collect_item_tree, register_builtin_intrinsics,
    register_builtin_types,
};
use kyokara_hir_def::item_tree::FnItemIdx;
use kyokara_intern::Interner;
use kyokara_span::FileId;
use kyokara_stdx::FxHashMap;
use kyokara_syntax::SyntaxNode;
use kyokara_syntax::ast::AstNode;
use kyokara_syntax::ast::nodes::SourceFile;

use crate::choice::{ChoiceRecorder, ChoiceReplayer, ChoiceSequence};
use crate::corpus::{self, CorpusEntry};
use crate::generate::{self, GenResult};
use crate::report::{FailureInfo, FnTestResult, TestReport};
use crate::shrink::{self, ShrinkOutcome};

/// Configuration for a PBT run.
#[derive(Debug, Clone)]
pub struct TestConfig {
    /// Number of random test cases per function.
    pub num_tests: usize,
    /// Whether to explore (generate random inputs) or only replay corpus.
    pub explore: bool,
    /// Optional fixed seed for deterministic runs.
    pub seed: u64,
    /// Output format: "human" or "json".
    pub format: String,
    /// Base path for corpus storage (directory containing the .ky file).
    pub corpus_base: std::path::PathBuf,
}

impl Default for TestConfig {
    fn default() -> Self {
        TestConfig {
            num_tests: 100,
            explore: false,
            seed: 0,
            format: "human".to_string(),
            corpus_base: std::path::PathBuf::from("."),
        }
    }
}

/// A function that's eligible for testing.
struct TestableFunction {
    idx: FnItemIdx,
    name: String,
    param_types: Vec<kyokara_hir_def::type_ref::TypeRef>,
}

/// Parse, type-check, and run property-based tests on a single source file.
pub fn run_tests(source: &str, config: &TestConfig) -> Result<TestReport, String> {
    let file_id = FileId(0);

    // 1. Parse.
    let parse = kyokara_syntax::parse(source);

    // 2. Build CST.
    let root = SyntaxNode::new_root(parse.green);
    let sf = SourceFile::cast(root.clone()).ok_or("failed to parse source file")?;

    // 3. Collect item tree.
    let mut interner = Interner::new();
    let mut item_result = collect_item_tree(&sf, file_id, &mut interner);

    // 4. Register builtins.
    register_builtin_types(
        &mut item_result.tree,
        &mut item_result.module_scope,
        &mut interner,
    );

    // 5. Register intrinsics.
    register_builtin_intrinsics(
        &mut item_result.tree,
        &mut item_result.module_scope,
        &mut interner,
    );

    // 6. Type-check.
    let type_check = check_module(
        &root,
        &item_result.tree,
        &item_result.module_scope,
        file_id,
        &mut interner,
    );

    // Reject files with compile errors.
    if !parse.errors.is_empty() {
        return Err(format!(
            "parse errors: {}",
            parse
                .errors
                .iter()
                .map(|e| format!("{e:?}"))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }

    // 7. Discover testable functions.
    let testable = discover_testable(&item_result.tree, &type_check.fn_bodies, &interner);

    // 8. Create interpreter.
    let mut interp = Interpreter::new(
        item_result.tree,
        item_result.module_scope,
        type_check.fn_bodies,
        FxHashMap::default(),
        interner,
        None,
    );

    // 9. Run tests.
    run_test_loop(&mut interp, &testable, config)
}

/// Parse, type-check, and run PBT on a multi-file project.
pub fn run_project_tests(
    entry_file: &std::path::Path,
    config: &TestConfig,
) -> Result<TestReport, String> {
    use kyokara_hir::ModulePath;

    let mut project = check_project(entry_file);

    // Find entry module.
    let entry_path = ModulePath::root();
    let entry_info = project
        .module_graph
        .get_mut(&entry_path)
        .ok_or("no entry module found")?;

    register_builtin_intrinsics(
        &mut entry_info.item_tree,
        &mut entry_info.scope,
        &mut project.interner,
    );

    let entry_tc = project
        .type_checks
        .iter()
        .find(|(p, _)| *p == entry_path)
        .map(|(_, tc)| tc)
        .ok_or("no type check for entry module")?;

    let mut fn_bodies: FxHashMap<FnItemIdx, kyokara_hir_def::body::Body> = FxHashMap::default();
    for (k, v) in &entry_tc.fn_bodies {
        fn_bodies.insert(*k, v.clone());
    }

    let entry_info = project
        .module_graph
        .get(&entry_path)
        .ok_or("entry module not found")?;

    let testable = discover_testable(&entry_info.item_tree, &fn_bodies, &project.interner);

    let mut interp = Interpreter::new(
        entry_info.item_tree.clone(),
        entry_info.scope.clone(),
        fn_bodies,
        FxHashMap::default(),
        project.interner,
        None,
    );

    run_test_loop(&mut interp, &testable, config)
}

/// Discover functions with contracts that have generatable parameter types.
fn discover_testable(
    item_tree: &kyokara_hir_def::item_tree::ItemTree,
    fn_bodies: &FxHashMap<FnItemIdx, kyokara_hir_def::body::Body>,
    interner: &Interner,
) -> Vec<TestableFunction> {
    let mut testable = Vec::new();

    for (idx, fn_item) in item_tree.functions.iter() {
        let name_str = fn_item.name.resolve(interner);

        // Skip main.
        if name_str == "main" {
            continue;
        }

        // Must have a body with contracts.
        let Some(body) = fn_bodies.get(&idx) else {
            continue;
        };

        let has_contracts =
            body.requires.is_some() || body.ensures.is_some() || body.invariant.is_some();
        if !has_contracts {
            continue;
        }

        // All params must have generatable types.
        let all_generatable = fn_item
            .params
            .iter()
            .all(|p| generate::is_generatable(&p.ty, item_tree, interner));

        if !all_generatable {
            continue;
        }

        let param_types: Vec<_> = fn_item.params.iter().map(|p| p.ty.clone()).collect();
        testable.push(TestableFunction {
            idx,
            name: name_str.to_string(),
            param_types,
        });
    }

    testable
}

/// The core test loop: for each testable function, run explore + corpus.
fn run_test_loop(
    interp: &mut Interpreter,
    testable: &[TestableFunction],
    config: &TestConfig,
) -> Result<TestReport, String> {
    let mut results = Vec::new();
    let mut skipped = Vec::new();

    // Collect all function names from the item tree for the "skipped" list.
    let all_fn_names: Vec<String> = {
        let it = interp.item_tree();
        let int = interp.interner();
        it.functions
            .iter()
            .map(|(_, f)| f.name.resolve(int).to_string())
            .collect()
    };

    let testable_names: Vec<&str> = testable.iter().map(|t| t.name.as_str()).collect();
    for name in &all_fn_names {
        if name != "main" && !testable_names.contains(&name.as_str()) {
            skipped.push(name.clone());
        }
    }

    for func in testable {
        let result = test_single_function(interp, func, config);
        results.push(result);
    }

    Ok(TestReport { results, skipped })
}

/// Test a single function: corpus replay + optional exploration.
fn test_single_function(
    interp: &mut Interpreter,
    func: &TestableFunction,
    config: &TestConfig,
) -> FnTestResult {
    let mut passed = 0usize;
    let mut discarded = 0usize;
    let mut total = 0usize;

    // Phase 1: Replay corpus entries.
    let corpus_entries = corpus::load_entries(&config.corpus_base, &func.name);
    for entry in &corpus_entries {
        let seq = ChoiceSequence::new(entry.choices.clone(), entry.maxima.clone());
        total += 1;
        match run_single_test(interp, func, &seq) {
            TestOutcome::Pass => passed += 1,
            TestOutcome::Discard => discarded += 1,
            TestOutcome::Fail(error, args_display) => {
                return FnTestResult {
                    name: func.name.clone(),
                    passed,
                    discarded,
                    total,
                    failure: Some(FailureInfo {
                        error,
                        args_display,
                        choices: seq,
                    }),
                };
            }
        }
    }

    // Phase 2: Explore (if enabled).
    if config.explore {
        for i in 0..config.num_tests {
            let seed = config
                .seed
                .wrapping_add(func.idx.into_raw().into_u32() as u64 * 10000 + i as u64);
            let mut recorder = ChoiceRecorder::new(seed);

            // Generate arguments.
            let args = match generate_args(func, &mut recorder, interp) {
                Some(a) => a,
                None => {
                    discarded += 1;
                    total += 1;
                    continue;
                }
            };

            let seq = recorder.into_sequence();
            total += 1;

            match call_and_classify(interp, func.idx, args) {
                TestOutcome::Pass => passed += 1,
                TestOutcome::Discard => discarded += 1,
                TestOutcome::Fail(error, args_display) => {
                    // Shrink the failing case.
                    let shrunk = shrink_failure(interp, func, &seq);

                    // Re-run with shrunk sequence to get the display args.
                    let (shrunk_error, shrunk_args) =
                        replay_for_display(interp, func, &shrunk).unwrap_or((error, args_display));

                    // Save to corpus.
                    let entry = CorpusEntry {
                        function: func.name.clone(),
                        choices: shrunk.choices.clone(),
                        maxima: shrunk.maxima.clone(),
                        error: shrunk_error.clone(),
                        args_display: shrunk_args.clone(),
                    };
                    let _ = corpus::save_entry(&config.corpus_base, &entry);

                    return FnTestResult {
                        name: func.name.clone(),
                        passed,
                        discarded,
                        total,
                        failure: Some(FailureInfo {
                            error: shrunk_error,
                            args_display: shrunk_args,
                            choices: shrunk,
                        }),
                    };
                }
            }
        }
    }

    // Check discard rate.
    let discard_rate = if total > 0 {
        discarded as f64 / total as f64
    } else {
        0.0
    };

    if discard_rate > 0.8 && total > 0 {
        eprintln!(
            "warning: {}: high discard rate ({:.0}%), preconditions may be too restrictive",
            func.name,
            discard_rate * 100.0
        );
    }

    FnTestResult {
        name: func.name.clone(),
        passed,
        discarded,
        total,
        failure: None,
    }
}

enum TestOutcome {
    Pass,
    Discard,
    Fail(String, Vec<String>),
}

/// Generate arguments for a function using a choice source.
fn generate_args(
    func: &TestableFunction,
    source: &mut dyn crate::choice::ChoiceSource,
    interp: &Interpreter,
) -> Option<Args> {
    let item_tree = interp.item_tree();
    let module_scope = interp.module_scope();
    let interner = interp.interner();

    let mut args = Args::new();
    for ty in &func.param_types {
        match generate::generate(ty, source, item_tree, module_scope, interner) {
            GenResult::Ok(val) => args.push(val),
            GenResult::Unsupported | GenResult::Exhausted => return None,
        }
    }
    Some(args)
}

/// Call a function and classify the result.
fn call_and_classify(interp: &mut Interpreter, fn_idx: FnItemIdx, args: Args) -> TestOutcome {
    let args_display: Vec<String> = args.iter().map(|v| v.display(interp.interner())).collect();

    match interp.call_fn_by_idx(fn_idx, args) {
        Ok(_) => TestOutcome::Pass,
        Err(RuntimeError::PreconditionFailed(_)) => TestOutcome::Discard,
        Err(RuntimeError::PostconditionFailed(msg)) => {
            TestOutcome::Fail(format!("postcondition failed: {msg}"), args_display)
        }
        Err(RuntimeError::InvariantViolated(msg)) => {
            TestOutcome::Fail(format!("invariant violated: {msg}"), args_display)
        }
        Err(e) => TestOutcome::Fail(format!("runtime error: {e}"), args_display),
    }
}

/// Run a single test case from a choice sequence (for corpus replay / shrinking).
fn run_single_test(
    interp: &mut Interpreter,
    func: &TestableFunction,
    seq: &ChoiceSequence,
) -> TestOutcome {
    let mut replayer = ChoiceReplayer::new(seq.clone());
    let args = match generate_args(func, &mut replayer, interp) {
        Some(a) => a,
        None => return TestOutcome::Discard,
    };
    call_and_classify(interp, func.idx, args)
}

/// Shrink a failing choice sequence.
fn shrink_failure(
    interp: &mut Interpreter,
    func: &TestableFunction,
    failing_seq: &ChoiceSequence,
) -> ChoiceSequence {
    shrink::shrink(
        failing_seq,
        &mut |candidate| match run_single_test(interp, func, candidate) {
            TestOutcome::Fail(_, _) => ShrinkOutcome::StillFails,
            TestOutcome::Pass => ShrinkOutcome::Passes,
            TestOutcome::Discard => ShrinkOutcome::Invalid,
        },
    )
}

/// Replay a shrunk sequence and capture the display values.
fn replay_for_display(
    interp: &mut Interpreter,
    func: &TestableFunction,
    seq: &ChoiceSequence,
) -> Option<(String, Vec<String>)> {
    let mut replayer = ChoiceReplayer::new(seq.clone());
    let args = generate_args(func, &mut replayer, interp)?;
    let args_display: Vec<String> = args.iter().map(|v| v.display(interp.interner())).collect();

    match interp.call_fn_by_idx(func.idx, args) {
        Err(RuntimeError::PostconditionFailed(msg)) => {
            Some((format!("postcondition failed: {msg}"), args_display))
        }
        Err(RuntimeError::InvariantViolated(msg)) => {
            Some((format!("invariant violated: {msg}"), args_display))
        }
        Err(e) => Some((format!("runtime error: {e}"), args_display)),
        Ok(_) => None,
    }
}
