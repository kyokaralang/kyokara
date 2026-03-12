# RFC 0007: Optional `typed_ast` Output for Compiler API (Dual-Mode Contract)

- Status: Implemented
- Authors: Kyokara maintainers
- Tracking issue: [#241](https://github.com/kyokaralang/kyokara/issues/241)
- Related: [#235](https://github.com/kyokaralang/kyokara/issues/235) (schema versioning, deferred)
- Last updated: 2026-03-11

## Summary

This RFC standardizes a dual-mode compiler API contract:

1. Default mode remains unchanged and fast:
   - `diagnostics`
   - `holes`
   - `symbol_graph`
2. Optional mode adds `typed_ast` only when explicitly requested.

This resolves docs/implementation drift from #241 without breaking existing consumers.

## Motivation

AI agents have two distinct needs:

1. Fast iterative loops that primarily need diagnostics and symbol topology.
2. Deep structural inspection when reasoning about typed expression/pattern trees.

Always returning full typed AST payloads increases latency and payload size for the default loop. Never returning typed AST blocks richer tooling. Dual-mode output is the smallest contract that supports both.

## Decision

### API surface

Existing entrypoints are preserved:

- `check(source, file_name) -> CheckOutput`
- `check_project(entry_file) -> CheckOutput`

New option-aware entrypoints are added:

- `check_with_options(source, file_name, &CheckOptions) -> CheckOutput`
- `check_project_with_options(entry_file, &CheckOptions) -> CheckOutput`

Options type:

```rust
pub struct CheckOptions {
    pub include_typed_ast: bool, // default: false
}
```

### `CheckOutput` contract

`CheckOutput` gains an additive optional field:

```rust
pub struct CheckOutput {
    pub diagnostics: Vec<DiagnosticDto>,
    pub holes: Vec<HoleSpecDto>,
    pub symbol_graph: SymbolGraphDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typed_ast: Option<TypedAstDto>,
}
```

Default mode omits the `typed_ast` key from serialized JSON.

### Typed AST DTO (minimal contract)

The typed AST payload is structural and typed, not CST-fidelity output:

```rust
pub struct TypedAstDto {
    pub partial: bool,
    pub files: Vec<TypedAstFileDto>,
}

pub struct TypedAstFileDto {
    pub file: String,
    pub functions: Vec<TypedFnAstDto>,
}

pub struct TypedFnAstDto {
    pub id: String,
    pub name: String,
    pub root_expr: u32,
    pub expr_nodes: Vec<TypedExprNodeDto>,
    pub pat_nodes: Vec<TypedPatNodeDto>,
}

pub struct TypedExprNodeDto {
    pub id: u32,
    pub kind: String,
    pub span: SpanDto,
    pub ty: String,
    pub expr_refs: Vec<u32>,
    pub pat_refs: Vec<u32>,
    pub symbol: Option<String>,
}

pub struct TypedPatNodeDto {
    pub id: u32,
    pub kind: String,
    pub span: SpanDto,
    pub ty: String,
    pub pat_refs: Vec<u32>,
    pub symbol: Option<String>,
}
```

### Symbol-link policy

- Function/type/capability/constructor symbols reuse symbol graph ID namespaces (`fn::...`, `type::...`, `cap::...`).
- Local bindings use deterministic function-scoped IDs:
  - `local::<fn_id>::<pat_idx>`

### `partial` semantics

`typed_ast.partial = true` when full typed-body coverage is not available (for example parse/lowering disruption in some bodies/modules). The payload remains serializable and best-effort.

## CLI integration

`kyokara check` adds:

- `--emit typed-ast`

Rules:

1. `--emit typed-ast` is valid only with `--format json`.
2. `--format human --emit typed-ast` is an explicit CLI error with non-zero exit.
3. `run`, `test`, `lsp`, and `refactor` behavior is unchanged.

## Compatibility and rollout

1. No breaking change for existing default API consumers.
2. `typed_ast` is additive and opt-in only.
3. No `schema_version` field in this RFC; schema-version policy remains scoped to #235.
4. Until #235 lands, contract evolution for this payload is additive-only.

## Testing requirements

Required guardrails:

1. Default serialization contract test:
   - `check()` JSON output omits `typed_ast`.
2. Opt-in contract test:
   - `check_with_options(... include_typed_ast=true)` includes `typed_ast`.
3. Typed AST shape tests:
   - deterministic IDs
   - non-empty node kinds
   - type strings present
   - valid spans
4. Project-mode test:
   - typed AST includes multiple files with correct file names.
5. Parse-error/partial-mode test:
   - `typed_ast.partial = true` and payload remains serializable.
6. CLI tests:
   - `check --format json --emit typed-ast` includes `typed_ast`
   - `check --format human --emit typed-ast` fails with clear message

## Out of scope

1. Token/trivia-preserving AST reconstruction.
2. Mandatory always-on typed AST output.
3. Schema version field and negotiation mechanism (deferred to #235).
