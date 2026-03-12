# RFC 0001: API Surface Law (AI-first)

- Status: Accepted
- Owner: Language Design
- Tracking issue: [#243](https://github.com/kyokaralang/kyokara/issues/243)
- Last updated: 2026-03-11

## Summary

Define a normative, mechanically-checkable set of API surface rules for Kyokara stdlib and language-facing APIs.

The goal is to reduce ambiguity for AI authors and keep APIs predictable under refactoring, code generation, and automated fixes.

## Motivation

AI-first authoring degrades when APIs allow multiple equivalent spellings, inconsistent parameter order, hidden globals, or fuzzy placement rules.

This RFC defines one canonical model:

1. behavior belongs where ownership is clear
2. data flow is positional-first and pipe-compatible
3. optionality/config is explicit and named
4. side effects are capability-scoped
5. evolution is additive and machine-trackable

## Scope

In scope:

- stdlib function/method/type-constructor shape
- intrinsic exposure policy
- parameter order and named-argument rules
- pipe compatibility contract
- compatibility and evolution constraints

Out of scope:

- parser syntax details not affecting API shape
- IR/codegen internals
- concurrency model

## Laws

### L1. One canonical spelling (`MUST`)

Each operation has exactly one canonical API form in docs and generated code.

Examples:

- canonical: `io.println("hi")`
- forbidden duplicates: both `len(x)` and `x.len()` for the same operation

### L2. Owner rule for placement (`MUST`)

If behavior clearly belongs to a value, use a method.
If no clear receiver owner exists, use a module function.

Examples:

- method: `s.trim()`, `n.abs()`
- module: `math.min(a, b)`, `math.gcd(a, b)`

### L3. No runtime globals as canonical API (`MUST`)

Runtime/library calls are namespaced under modules or types.

Examples:

- canonical: `io.print`, `io.println`, `fs.read_file`
- if shorthand exists, it must desugar to canonical form

### L4. Effects via capability modules (`MUST`)

Side-effecting APIs live in capability-scoped modules and are enforceable through manifests/type-level effect checking.

Examples:

- `io.*`, `fs.*`, `net.*`

### L5. Construction in type namespace (`MUST`)

Constructors/factories are namespaced under types.

Examples:

- `collections.List.new()`, `collections.Map.new()`, `collections.Set.new()`

### L5.1 Core value types vs capability modules (`MUST`)

Core collections (`List`, `Map`, `Set`) are builtin prelude value types, not
imported capability modules.

Implications:

- constructors are module-qualified under `collections` with type ownership (`collections.List.new()`, `collections.Map.new()`, `collections.Set.new()`)
- value-owned behavior is exposed as methods (`xs.push`, `m.insert`, `s.contains`)
- no free global aliases (e.g. `list_new`, `map_new`, `set_new`) in user-facing API

By contrast, effectful APIs are module-qualified capability surfaces (`io.*`,
`fs.*`, `net.*`) and require capability enforcement (`L4`).

### L6. Fallibility semantics are explicit (`MUST`)

- `to_*` means total/safe conversion
- `parse_*` means fallible parsing and should return `Result` when represented in surface API

Principle:

- Parsing belongs to the source representation owner. For text parsing, the owner is `String` (or equivalent text type), not the destination type.

Canonical parsing forms for numeric text:

- `s.parse_int() -> Result<Int, ParseError>`
- `s.parse_float() -> Result<Float, ParseError>`
- canonical fallback/composition on `Result`:
  - `s.parse_int().unwrap_or(0)`
  - `s.parse_int().map_or(0, fn(n: Int) => n + 1)`
  - `s.parse_int().map(fn(n: Int) => n + 1).unwrap_or(0)`
  - `s.parse_int().and_then(fn(n: Int) => Result.Ok(n + 1)).unwrap_or(0)`
  - `s.parse_int().map_err(fn(e: ParseError) => e)`
- canonical fallback/composition on `Option`:
  - `parts.get(0).unwrap_or("0")`
  - `parts.get(0).map_or("0", fn(s: String) => s)`
  - `parts.get(0).map(fn(s: String) => s.trim())`
  - `parts.get(0).and_then(fn(s: String) => Option.Some(s))`

Non-canonical aliases must not be introduced as parallel public APIs:

- `parse_int(s)`
- `parse_float(s)`
- `Int.parse(s)`
- `Float.parse(s)`

### L7. No synonyms and no ad-hoc overloads (`MUST NOT`)

Do not ship duplicate entrypoints for the same semantic operation.
Do not rely on type/arity overload ambiguity as the primary API shape.

### L7A. Constrained call families (`MAY`, constrained)

Kyokara does not support general overload resolution.

Kyokara may define a constrained call family only when all of the following are
true:

1. all variants share one semantic operation under one canonical name
2. call resolution depends only on argument count and/or presence of declared named arguments
3. call resolution does not depend on argument types
4. each valid call shape maps to exactly one variant mechanically
5. no parallel synonym name is introduced for the same operation
6. docs/examples/diagnostics/completion specify the full family explicitly

This rule applies to both builtin APIs and user-defined functions/methods.
Current user-declared source syntax exposes arity-distinct families directly.
Named-only family branches are also supported by the call-family mechanism and
used by builtin APIs such as `starts_with(prefix, start: idx)`, but
user-declared named-only parameter syntax remains deferred until that source
surface is specified separately.

Canonical examples:

- `xs.count()` counts all elements
- `xs.count(f)` counts matching elements
- `s.starts_with(prefix)` checks a prefix from the beginning
- `s.starts_with(prefix, start: idx)` checks a prefix from a given offset

Related direct terminals that remain separate operations, not family variants:

- `xs.contains(value)` checks direct element membership
- `xs.frequencies()` returns `Map<T, Int>` bucket counts in first-seen key order

Non-canonical shapes:

- `xs.count_if(f)`
- `s.starts_with_at(idx, prefix)` once the constrained family exists
- any same-name family that requires type-directed dispatch

### L8. No implicit coercions (`MUST`)

Type changes are explicit in API usage.

### L9. Parameter mode contract (`MUST`)

1. arg1 is primary data value (pipe receiver), positional
2. required args follow, positional
3. optional/config args are named-only
4. positional optional args are forbidden
5. positional arguments must not appear after a named argument (for all call forms)

### L10. Required positional semantic order (`MUST`)

Use these stable pair orderings:

1. source before target
2. low before high
3. start before end

If no obvious pair exists, declaration order becomes permanent API contract.

### L11. Ambiguity guard (`MUST`)

Require named args or options objects when any are true:

1. adjacent same-category parameter types cause confusion
2. boolean flags are present
3. more than 3 required non-data parameters would be needed

### L12. Pipe desugaring (`MUST`)

`x |> f(a, b, k: v)` desugars to `f(x, a, b, k: v)`.
Argument expressions are always evaluated left-to-right in source order before slot binding.

### L13. Pipeline direction (`SHOULD`)

Pipelines are left-to-right transformations; sinks/effects should be terminal steps.

### L14. Sugar policy (`MAY`, constrained)

Human ergonomics sugar is allowed only if lossless and equivalent to canonical APIs.

### L15. Evolution policy (`SHOULD`)

Prefer additive evolution. Breaking changes require deprecation windows and migration guidance/tooling.
Before language freeze, a ratified RFC may still approve a deliberate hard-break cleanup when it is explicitly framed as pre-freeze surface finalization.

### L16. Core behavior binds by identity, not names (`MUST`)

Core method/static/constructor behavior must dispatch by internal type identity, not by surface string names.

Implications:

- user type-name shadowing (`type Result<...> = ...`, `type List<T> = ...`) must not retarget builtin core behavior
- runtime must not rely on `expect("... not registered")` for core constructor lookup paths reachable from user code
- inference/eval/KIR must agree on the same identity-based owner key model

### L17. Constructors are type-owned and imported explicitly (`MUST`)

Constructors and variants belong to their owning ADT type.

Implications:

- `Type.Variant` must always be valid in expression and pattern positions
- bare `Variant` is valid only when explicitly imported, for example `from Result import Ok, Err`
- `import path` binds a namespace only; `from path import Name` binds members directly
- no global reserved constructor-name carveout is permitted

### L18. Traversal model is collection-first and canonical (`MUST`)

Traversal operations are called directly on traversable source values.
Internal traversal engine types are not required in canonical user code.
Source constructors remain explicit and minimal.
Docs/examples/diagnostics must use collection-first canonical forms.

Canonical consequences:

- integer ranges use `start..<end` (constructor surface)
- stateful sources use `seed.unfold(step)` (constructor surface)
- `List`/`Deque` expose storage methods and traversal methods directly
- traversal transforms/terminals (`map/filter/enumerate/zip/chunks/windows/fold/any/all/find/count/contains/frequencies/to_list`) are callable on collection and producer values
- `count` is a constrained call family under `L7A`: `count()` for all elements, `count(predicate)` for matching elements
- user-defined arity-distinct families are allowed under `L7A` (for example `fn foo()` and `fn foo(x: Int)`)
- producer traversal APIs stay traversal-capable (`String.split/lines/chars`, `Map.keys/values`, `Set.values`)

## Visibility policy (canonical decision)

Kyokara uses this intrinsic visibility matrix:

| Category | Canonical form | Import required | Example |
|---|---|---|---|
| Prelude builtin value types | Type names + methods | No | `let xs: List<Int> = ...`, `xs.len()` |
| Pure collection constructors | Module-qualified under `collections` | Yes | `collections.List.new()`, `collections.Map.new()`, `collections.Set.new()` |
| Pure no-owner utilities | Module-qualified | Yes | `math.min(a, b)` |
| Effectful utilities | Capability module-qualified | Yes | `io.println("x")`, `fs.read_file(path)` |
| Internal intrinsic IDs | Not user-visible | N/A | `list_new`, `map_insert`, `set_contains` |

Global/free-function intrinsic spellings are non-canonical and rejected in user
space. If referenced, diagnostics may include migration hints to canonical forms.

## Mechanically-checkable criteria

The following can be enforced by lints/tooling:

1. duplicate canonical spellings for same operation
2. non-pipe-compatible arg1 placement for transform APIs
3. optional positional params
4. unnamed boolean flags in function signatures
5. unnamespaced canonical runtime APIs
6. unstable parameter ordering changes across versions
7. forbidden global intrinsic spellings resolving in user scope
8. core method/static/constructor dispatch keyed by surface names instead of owner identity
9. unqualified user constructors colliding with reserved core constructor names while reservation is active
10. call families that are not explicitly documented and justified under `L7A`

## API-surface conformance checklist

Use this checklist in PRs that add or change stdlib/intrinsic/public APIs.

### A. New API checklist

- [ ] Canonical spelling is unique, or any constrained call family is explicitly justified under `L7A`.
- [ ] Placement follows owner rule (method vs module vs type constructor) (`L2`, `L5`).
- [ ] Canonical form is namespaced (not a runtime global) (`L3`).
- [ ] Effectful behavior is capability-scoped (`L4`).
- [ ] No implicit coercion is required to call/use the API (`L8`).
- [ ] Signature follows parameter mode contract: data first, required positional, optional named-only (`L9`).
- [ ] Required positional semantic order is stable and documented (`L10`).
- [ ] Ambiguity guard applied (booleans/ambiguous same-category params/options object) (`L11`).
- [ ] If pipe-eligible, `x |> f(...)` desugars cleanly to `f(x, ...)` (`L12`).
- [ ] Any sugar is lossless and desugars to canonical form (`L14`).
- [ ] Parsing APIs follow source-owner placement and return `Result` (`L6`).
- [ ] Core dispatch path is identity-based (no string-name dispatch for core behavior) (`L16`).
- [ ] Constructor ownership/import rules are respected: `Type.Variant` or explicit `from Type import Variant` (`L17`).

### B. API change checklist

- [ ] Parameter order in existing APIs is unchanged unless breaking change is explicitly approved (`L10`, `L15`).
- [ ] No new synonym path is introduced, and any constrained call family remains within the `L7A` guardrails.
- [ ] Existing canonical call sites remain valid or have migration guidance (`L15`).
- [ ] Deprecation/migration notes are included when behavior or naming changes (`L15`).
- [ ] Docs and machine-facing outputs are updated in the same PR.

### C. Review gate outcome

Mark one:

- [ ] `PASS` — all required checks satisfied.
- [ ] `PASS WITH FOLLOW-UP` — non-blocking gaps tracked in issue(s).
- [ ] `BLOCKED` — violates one or more `MUST` laws.

## Rollout plan

1. publish this RFC and link from design docs
2. align stdlib/intrinsic docs to canonical forms
3. add conformance checks incrementally (lint + tests)
4. enforce evolution/deprecation policy in release process
5. keep constructor/import docs and tooling aligned with type-owned variants and `from ... import ...` member imports

## Open questions

1. Should a default prelude expose any non-capability pure helpers?
2. Should parser-level syntax support a first-class options object literal convention?
3. Which rules should be hard errors vs warnings in v0.x?
