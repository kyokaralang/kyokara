//! Test runner — discovery, generation loop, shrinking integration.

use kyokara_eval::error::RuntimeError;
use kyokara_eval::interpreter::Interpreter;
use kyokara_eval::intrinsics::Args;
use kyokara_hir::ModulePath;
use kyokara_hir::{
    activate_synthetic_imports, check_module, check_project, collect_item_tree,
    register_builtin_intrinsics, register_builtin_methods, register_builtin_types,
    register_static_methods, register_synthetic_modules,
};
use kyokara_hir_def::item_tree::FnItemIdx;
use kyokara_intern::Interner;
use kyokara_span::{FileId, TextRange, TextSize};
use kyokara_stdx::FxHashMap;
use kyokara_syntax::SyntaxNode;
use kyokara_syntax::ast::AstNode;
use kyokara_syntax::ast::nodes::SourceFile;

use crate::choice::{ChoiceRecorder, ChoiceReplayer, ChoiceSequence};
use crate::corpus::{self, CorpusEntry};
use crate::generate::{self, GenResult};
use crate::report::{FailureInfo, FnTestResult, TestReport, TestableKind};
use crate::shrink::{self, ShrinkOutcome};

/// Maximum ratio of total attempts to desired passing tests.
///
/// When a property has a `where` constraint, many generated candidates get
/// discarded. We allow up to `num_tests * MAX_DISCARD_RATIO` attempts before
/// declaring the constraint unsatisfiable.
const MAX_DISCARD_RATIO: usize = 100;

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

/// A property that's eligible for testing.
struct TestableProperty {
    fn_idx: FnItemIdx,
    name: String,
    param_types: Vec<kyokara_hir_def::type_ref::TypeRef>,
    gen_specs: Vec<kyokara_hir_def::item_tree::GenSpec>,
}

/// Parse, type-check, and run property-based tests on a single source file.
pub fn run_tests(source: &str, config: &TestConfig) -> Result<TestReport, String> {
    let file_id = FileId(0);

    // 1. Parse.
    let parse = kyokara_syntax::parse(source);
    let parse_error_ranges: Vec<TextRange> = parse
        .errors
        .iter()
        .map(|err| normalize_parse_error_range(err.range_start, err.range_end, source.len() as u32))
        .collect();

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

    // 5. Register intrinsics and canonical API surface.
    register_builtin_intrinsics(
        &mut item_result.tree,
        &mut item_result.module_scope,
        &mut interner,
    );
    register_builtin_methods(&mut item_result.module_scope, &mut interner);
    register_synthetic_modules(
        &mut item_result.tree,
        &mut item_result.module_scope,
        &mut interner,
    );
    register_static_methods(&mut item_result.module_scope, &mut interner);
    activate_synthetic_imports(
        &item_result.tree,
        &mut item_result.module_scope,
        &mut interner,
    );

    // 6. Type-check.
    let type_check = check_module(
        &root,
        &item_result.tree,
        &item_result.module_scope,
        &parse_error_ranges,
        file_id,
        &mut interner,
    );

    if let Some(err) = collect_compile_errors_single(
        &parse.errors,
        &item_result.diagnostics,
        &type_check.diagnostics,
        &type_check.body_lowering_diagnostics,
    ) {
        return Err(err);
    }

    // 7. Discover testable functions and properties.
    let testable = discover_testable(&item_result.tree, &type_check.fn_bodies, &interner);
    let properties = discover_properties(&item_result.tree, &interner);

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
    run_test_loop(&mut interp, &testable, &properties, config)
}

/// Parse, type-check, and run PBT on a multi-file project.
pub fn run_project_tests(
    entry_file: &std::path::Path,
    config: &TestConfig,
) -> Result<TestReport, String> {
    let mut project = check_project(entry_file);
    if let Some(err) = collect_compile_errors_project(&project.parse_errors, &project) {
        return Err(err);
    }

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
    register_builtin_methods(&mut entry_info.scope, &mut project.interner);
    register_synthetic_modules(
        &mut entry_info.item_tree,
        &mut entry_info.scope,
        &mut project.interner,
    );
    register_static_methods(&mut entry_info.scope, &mut project.interner);
    activate_synthetic_imports(
        &entry_info.item_tree,
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
    let properties = discover_properties(&entry_info.item_tree, &project.interner);

    let mut interp = Interpreter::new(
        entry_info.item_tree.clone(),
        entry_info.scope.clone(),
        fn_bodies,
        FxHashMap::default(),
        project.interner,
        None,
    );

    run_test_loop(&mut interp, &testable, &properties, config)
}

fn collect_compile_errors_single<T: std::fmt::Debug>(
    parse_errors: &[T],
    lowering_diagnostics: &[kyokara_diagnostics::Diagnostic],
    type_diagnostics: &[kyokara_diagnostics::Diagnostic],
    body_lowering_diagnostics: &[kyokara_diagnostics::Diagnostic],
) -> Option<String> {
    let mut lines = Vec::new();

    if !parse_errors.is_empty() {
        lines.push(format!(
            "parse errors: {}",
            parse_errors
                .iter()
                .map(|e| format!("{e:?}"))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }

    for d in lowering_diagnostics.iter().filter(is_error) {
        lines.push(format!("compile error: {}", d.message));
    }
    for d in type_diagnostics.iter().filter(is_error) {
        lines.push(format!("compile error: {}", d.message));
    }
    for d in body_lowering_diagnostics.iter().filter(is_error) {
        lines.push(format!("compile error: {}", d.message));
    }

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("; "))
    }
}

fn collect_compile_errors_project<T: std::fmt::Debug>(
    parse_errors_by_module: &[(ModulePath, Vec<T>)],
    project: &kyokara_hir::ProjectCheckResult,
) -> Option<String> {
    let mut lines = Vec::new();

    for (module_path, parse_errors) in parse_errors_by_module {
        if parse_errors.is_empty() {
            continue;
        }
        let module = format_module_path(module_path, &project.interner);
        lines.push(format!(
            "module {module}: parse errors: {}",
            parse_errors
                .iter()
                .map(|e| format!("{e:?}"))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }

    for d in project.lowering_diagnostics.iter().filter(is_error) {
        lines.push(format!("compile error: {}", d.message));
    }

    for (module_path, tc) in &project.type_checks {
        let module = format_module_path(module_path, &project.interner);
        for d in tc.diagnostics.iter().filter(is_error) {
            lines.push(format!("module {module}: compile error: {}", d.message));
        }
        for d in tc.body_lowering_diagnostics.iter().filter(is_error) {
            lines.push(format!("module {module}: compile error: {}", d.message));
        }
    }

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("; "))
    }
}

fn is_error(d: &&kyokara_diagnostics::Diagnostic) -> bool {
    d.severity == kyokara_diagnostics::Severity::Error
}

fn normalize_parse_error_range(start: u32, end: u32, source_len: u32) -> TextRange {
    let start = start.min(source_len);
    let end = end.min(source_len);
    if start < end {
        return TextRange::new(TextSize::from(start), TextSize::from(end));
    }
    if source_len == 0 {
        return TextRange::new(TextSize::from(0), TextSize::from(0));
    }
    if start < source_len {
        let right = (start + 1).min(source_len);
        return TextRange::new(TextSize::from(start), TextSize::from(right));
    }
    let left = start.saturating_sub(1);
    TextRange::new(TextSize::from(left), TextSize::from(start))
}

fn format_module_path(path: &ModulePath, interner: &Interner) -> String {
    if path.0.is_empty() {
        "<root>".to_string()
    } else {
        path.0
            .iter()
            .map(|seg| seg.resolve(interner).to_string())
            .collect::<Vec<_>>()
            .join(".")
    }
}

/// Discover functions with contracts that have generatable parameter types.
fn discover_testable(
    item_tree: &kyokara_hir_def::item_tree::ItemTree,
    fn_bodies: &FxHashMap<FnItemIdx, kyokara_hir_def::body::Body>,
    interner: &Interner,
) -> Vec<TestableFunction> {
    // Collect FnItemIdx values that back property definitions so we can
    // skip them — properties are tested via `discover_properties`.
    let property_fn_idxs: std::collections::HashSet<_> = item_tree
        .properties
        .iter()
        .filter_map(|(_, p)| p.fn_idx)
        .collect();

    let mut testable = Vec::new();

    for (idx, fn_item) in item_tree.functions.iter() {
        // Skip synthetic functions backing property definitions.
        if property_fn_idxs.contains(&idx) {
            continue;
        }

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
            !body.requires.is_empty() || !body.ensures.is_empty() || !body.invariant.is_empty();
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

/// Discover properties with bodies and generatable parameter types.
fn discover_properties(
    item_tree: &kyokara_hir_def::item_tree::ItemTree,
    interner: &Interner,
) -> Vec<TestableProperty> {
    let mut properties = Vec::new();

    for (_, prop) in item_tree.properties.iter() {
        let Some(fn_idx) = prop.fn_idx else {
            continue;
        };

        let all_generatable = prop
            .params
            .iter()
            .all(|ps| generate::is_generatable(&ps.param.ty, item_tree, interner));

        if !all_generatable {
            continue;
        }

        let param_types: Vec<_> = prop.params.iter().map(|ps| ps.param.ty.clone()).collect();
        let gen_specs: Vec<_> = prop.params.iter().map(|ps| ps.gen_spec.clone()).collect();
        properties.push(TestableProperty {
            fn_idx,
            name: prop.name.resolve(interner).to_string(),
            param_types,
            gen_specs,
        });
    }

    properties
}

/// The core test loop: for each testable function/property, run explore + corpus.
fn run_test_loop(
    interp: &mut Interpreter,
    testable: &[TestableFunction],
    properties: &[TestableProperty],
    config: &TestConfig,
) -> Result<TestReport, String> {
    let mut results = Vec::new();
    let mut skipped = Vec::new();

    // Collect fn_idxs that are property-backing synthetic functions.
    let property_fn_idxs: Vec<FnItemIdx> = properties.iter().map(|p| p.fn_idx).collect();

    // Collect all function names from the item tree for the "skipped" list.
    let all_fn_names: Vec<(String, FnItemIdx)> = {
        let it = interp.item_tree();
        let int = interp.interner();
        it.functions
            .iter()
            .map(|(idx, f)| (f.name.resolve(int).to_string(), idx))
            .collect()
    };

    let testable_names: Vec<&str> = testable.iter().map(|t| t.name.as_str()).collect();
    for (name, idx) in &all_fn_names {
        // Skip main, tested functions, and property-backing synthetic functions.
        if name != "main"
            && !testable_names.contains(&name.as_str())
            && !property_fn_idxs.contains(idx)
        {
            skipped.push(name.clone());
        }
    }

    for func in testable {
        let result = test_single_function(interp, func, config);
        results.push(result);
    }

    for prop in properties {
        let result = test_single_property(interp, prop, config);
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
                    kind: TestableKind::Function,
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
                        kind: TestableKind::Function,
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
        kind: TestableKind::Function,
        passed,
        discarded,
        total,
        failure: None,
    }
}

/// Test a single property: generate inputs and check that it returns true.
///
/// `where` constraints are lowered as `requires` preconditions on the
/// property body. `PreconditionFailed` → discard (same as contracted functions).
fn test_single_property(
    interp: &mut Interpreter,
    prop: &TestableProperty,
    config: &TestConfig,
) -> FnTestResult {
    let mut passed = 0usize;
    let mut discarded = 0usize;
    let mut total = 0usize;

    // Phase 1: Replay corpus entries.
    let corpus_entries = corpus::load_entries(&config.corpus_base, &prop.name);
    for entry in &corpus_entries {
        let seq = ChoiceSequence::new(entry.choices.clone(), entry.maxima.clone());
        total += 1;
        match run_single_property_test(interp, prop, &seq) {
            TestOutcome::Pass => passed += 1,
            TestOutcome::Discard => discarded += 1,
            TestOutcome::Fail(error, args_display) => {
                return FnTestResult {
                    name: prop.name.clone(),
                    kind: TestableKind::Property,
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
    //
    // Use a discard budget: keep generating until `num_tests` pass or we
    // exhaust `num_tests * MAX_DISCARD_RATIO` total attempts. This handles
    // `where` constraints that filter out most candidates (e.g., `x > 0` on
    // a full-range Int generator).
    if config.explore {
        let max_attempts = config.num_tests.saturating_mul(MAX_DISCARD_RATIO);
        let mut attempt = 0usize;

        while passed < config.num_tests && attempt < max_attempts {
            let seed = config
                .seed
                .wrapping_add(prop.fn_idx.into_raw().into_u32() as u64 * 10000 + attempt as u64);
            attempt += 1;
            let mut recorder = ChoiceRecorder::new(seed);

            let args = match generate_property_args(prop, &mut recorder, interp) {
                PropertyArgsResult::Ready(args) => args,
                PropertyArgsResult::Exhausted => {
                    discarded += 1;
                    total += 1;
                    continue;
                }
                PropertyArgsResult::Unsupported => {
                    total += 1;
                    return FnTestResult {
                        name: prop.name.clone(),
                        kind: TestableKind::Property,
                        passed,
                        discarded,
                        total,
                        failure: Some(FailureInfo {
                            error: "invalid or unsupported generator configuration".to_string(),
                            args_display: vec![],
                            choices: ChoiceSequence::new(vec![], vec![]),
                        }),
                    };
                }
            };

            let seq = recorder.into_sequence();
            total += 1;

            match call_and_classify_property(interp, prop.fn_idx, args) {
                TestOutcome::Pass => passed += 1,
                TestOutcome::Discard => discarded += 1,
                TestOutcome::Fail(error, args_display) => {
                    // Shrink the failing case.
                    let shrunk = shrink_property_failure(interp, prop, &seq);

                    // Re-run with shrunk sequence to get the display args.
                    let (shrunk_error, shrunk_args) =
                        replay_property_for_display(interp, prop, &shrunk)
                            .unwrap_or((error, args_display));

                    // Save to corpus.
                    let entry = CorpusEntry {
                        function: prop.name.clone(),
                        choices: shrunk.choices.clone(),
                        maxima: shrunk.maxima.clone(),
                        error: shrunk_error.clone(),
                        args_display: shrunk_args.clone(),
                    };
                    let _ = corpus::save_entry(&config.corpus_base, &entry);

                    return FnTestResult {
                        name: prop.name.clone(),
                        kind: TestableKind::Property,
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

    // Check for unsatisfiable `where` constraint: if all explored cases
    // were discarded (no passing cases found), report as failure.
    if config.explore && passed == 0 && discarded > 0 && total > 0 {
        return FnTestResult {
            name: prop.name.clone(),
            kind: TestableKind::Property,
            passed,
            discarded,
            total,
            failure: Some(FailureInfo {
                error: format!(
                    "unsatisfiable or overly restrictive `where` constraint: \
                     all {} test cases were discarded",
                    total
                ),
                args_display: vec![],
                choices: ChoiceSequence::new(vec![], vec![]),
            }),
        };
    }

    FnTestResult {
        name: prop.name.clone(),
        kind: TestableKind::Property,
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

// ── Property-specific helpers ──────────────────────────────────────

enum PropertyArgsResult {
    Ready(Args),
    Exhausted,
    Unsupported,
}

/// Generate arguments for a property using its generator specs.
fn generate_property_args(
    prop: &TestableProperty,
    source: &mut dyn crate::choice::ChoiceSource,
    interp: &Interpreter,
) -> PropertyArgsResult {
    let item_tree = interp.item_tree();
    let module_scope = interp.module_scope();
    let interner = interp.interner();

    let mut args = Args::new();
    for (ty, spec) in prop.param_types.iter().zip(prop.gen_specs.iter()) {
        match generate::generate_from_spec(spec, ty, source, item_tree, module_scope, interner) {
            GenResult::Ok(val) => args.push(val),
            GenResult::Unsupported => return PropertyArgsResult::Unsupported,
            GenResult::Exhausted => return PropertyArgsResult::Exhausted,
        }
    }
    PropertyArgsResult::Ready(args)
}

/// Call a property and classify the result.
///
/// Properties return Bool: true → Pass, false → Fail.
/// `PreconditionFailed` (from `where` constraint) → Discard.
fn call_and_classify_property(
    interp: &mut Interpreter,
    fn_idx: FnItemIdx,
    args: Args,
) -> TestOutcome {
    use kyokara_eval::value::Value;

    let args_display: Vec<String> = args.iter().map(|v| v.display(interp.interner())).collect();

    match interp.call_fn_by_idx(fn_idx, args) {
        Ok(Value::Bool(true)) => TestOutcome::Pass,
        Ok(Value::Bool(false)) => {
            TestOutcome::Fail("property returned false".to_string(), args_display)
        }
        Ok(_) => TestOutcome::Fail("property did not return Bool".to_string(), args_display),
        Err(RuntimeError::PreconditionFailed(_)) => TestOutcome::Discard,
        Err(e) => TestOutcome::Fail(format!("runtime error: {e}"), args_display),
    }
}

/// Run a single property test case from a choice sequence.
fn run_single_property_test(
    interp: &mut Interpreter,
    prop: &TestableProperty,
    seq: &ChoiceSequence,
) -> TestOutcome {
    let mut replayer = ChoiceReplayer::new(seq.clone());
    match generate_property_args(prop, &mut replayer, interp) {
        PropertyArgsResult::Ready(args) => call_and_classify_property(interp, prop.fn_idx, args),
        PropertyArgsResult::Exhausted => TestOutcome::Discard,
        PropertyArgsResult::Unsupported => TestOutcome::Fail(
            "invalid or unsupported generator configuration".to_string(),
            vec![],
        ),
    }
}

/// Shrink a failing property choice sequence.
///
/// If the property has refinement predicates, candidates that violate the
/// predicate are treated as `Invalid` (not `Passes`), ensuring the shrunk
/// counterexample still satisfies the constraint.
fn shrink_property_failure(
    interp: &mut Interpreter,
    prop: &TestableProperty,
    failing_seq: &ChoiceSequence,
) -> ChoiceSequence {
    shrink::shrink(
        failing_seq,
        &mut |candidate| match run_single_property_test(interp, prop, candidate) {
            TestOutcome::Fail(_, _) => ShrinkOutcome::StillFails,
            TestOutcome::Pass => ShrinkOutcome::Passes,
            TestOutcome::Discard => ShrinkOutcome::Invalid,
        },
    )
}

/// Replay a shrunk property sequence and capture the display values.
fn replay_property_for_display(
    interp: &mut Interpreter,
    prop: &TestableProperty,
    seq: &ChoiceSequence,
) -> Option<(String, Vec<String>)> {
    use kyokara_eval::value::Value;

    let mut replayer = ChoiceReplayer::new(seq.clone());
    let args = match generate_property_args(prop, &mut replayer, interp) {
        PropertyArgsResult::Ready(args) => args,
        PropertyArgsResult::Exhausted => return None,
        PropertyArgsResult::Unsupported => {
            return Some((
                "invalid or unsupported generator configuration".to_string(),
                vec![],
            ));
        }
    };

    let args_display: Vec<String> = args.iter().map(|v| v.display(interp.interner())).collect();

    match interp.call_fn_by_idx(prop.fn_idx, args) {
        Ok(Value::Bool(true)) => None,
        Ok(Value::Bool(false)) => Some(("property returned false".to_string(), args_display)),
        Ok(_) => Some(("property did not return Bool".to_string(), args_display)),
        Err(RuntimeError::PreconditionFailed(_)) => None, // Discard: where constraint violated.
        Err(e) => Some((format!("runtime error: {e}"), args_display)),
    }
}
