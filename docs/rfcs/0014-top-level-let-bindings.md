# RFC 0014: Top-Level Immutable Let Bindings

- Status: Draft
- Owner: Language Design
- Tracking issue: #410
- Last updated: 2026-03-09

## Summary

Add module-scope immutable value bindings to Kyokara:

1. `let x = expr` is allowed at top level.
2. `let x: T = expr` is allowed at top level.
3. Top-level `let` defines a module-local immutable value.
4. Top-level `let` initializers are evaluated eagerly in source order.

This RFC is intentionally narrow:

1. No `pub let`.
2. No top-level `var`.
3. No cross-module value imports or exports.
4. No lazy module initialization.
5. No cycle analysis beyond ordinary unresolved-name behavior.

RFC 0013 remains authoritative for mutable bindings: `var` is local-only and top-level `var` stays invalid.

## Motivation

Kyokara already accepts top-level `let` syntactically, but until issue #410 it had no full semantic or runtime model. That gap creates a bad outcome:

1. the syntax suggests module constants are supported,
2. but semantic checking stops early,
3. and evaluation cannot read them at runtime.

This is the wrong kind of complexity. Module-local immutable values are useful and unsurprising:

```kyokara
let off = 1

fn add(x: Int) -> Int { x + off }
```

Kyokara is already immutable-by-default. Top-level immutable bindings fit that model naturally. Top-level mutable globals do not.

## Design goals

1. Make top-level immutable bindings real language semantics, not parse-only surface.
2. Keep module initialization simple and eager.
3. Preserve deterministic, explainable name-resolution behavior.
4. Support same-module use in both single-file and project mode.
5. Keep the boundary with RFC 0013 explicit: immutable module values are allowed; mutable module bindings are not.

## Non-goals

1. `pub let`.
2. Top-level `var` or mutable globals.
3. Importing values from other modules.
4. Lazy or demand-driven module initialization.
5. Dependency graph sorting or full cycle detection between top-level values.
6. Destructuring patterns at top level.

## Proposal

### P1. Surface

Top-level `let` uses the existing binding syntax:

```kyokara
let x = expr
let x: T = expr
```

Phase-1 restrictions:

1. The binding pattern must be a simple identifier.
2. The binding is module-local even if the file contains `pub` items elsewhere.
3. `pub let` remains invalid.

Examples:

```kyokara
let off = 1
let answer: Int = 41 + 1

fn main() -> Int { answer + off }
```

### P2. Scope and visibility

Top-level immutable lets live in the module value namespace.

Rules:

1. Functions in the same module may read top-level lets.
2. A top-level let initializer may read earlier top-level lets from the same module.
3. A top-level let initializer may refer to functions, types, and effects already visible in that module.
4. Forward references from one top-level let initializer to a later top-level let are invalid in phase 1.

Example:

```kyokara
let a = 1
let b = a + 1

fn main() -> Int { b }
```

This is valid. The reverse order is not:

```kyokara
let b = a + 1
let a = 1
```

That should fail as an unresolved name in `b`'s initializer.

### P3. Initialization semantics

Top-level lets are evaluated eagerly in source order.

Rules:

1. Module initialization happens before `main` executes.
2. Any runtime entrypoint that directly invokes a user function must ensure top-level lets for that module are initialized first.
3. The initializer result is stored as an immutable module value for later reads.

This deliberately avoids hidden lazy evaluation or dependency magic. The model is: parse order, one pass, immutable results.

### P4. Project-mode behavior

Project mode keeps the same rule: functions read top-level lets from their own source module.

This means:

1. an imported function may read immutable values defined in the module where that function was declared,
2. but this RFC does not add value import/export syntax for callers.

## Diagnostics

Targeted diagnostics should stay direct:

1. `` top-level let bindings must use a simple identifier pattern ``
2. ordinary unresolved-name diagnostics for forward references to later top-level lets
3. `` top-level `var` bindings are not allowed `` remains governed by RFC 0013

## Runtime and compiler notes

Expected implementation shape:

1. item collection records top-level lets in module scope as value names,
2. body lowering lowers top-level let initializers through ordinary expression lowering,
3. type checking infers initializer bodies in source order so earlier lets are visible to later lets,
4. evaluation materializes module let values before executing user code,
5. project mode preserves per-module value lookup for imported functions.

## RFC alignment

### RFC 0013 (local mutable bindings)

Complementary, not overlapping.

RFC 0013 defines local mutable rebinding:

1. `var` is local-only,
2. assignment targets are local identifiers only,
3. top-level `var` is invalid.

This RFC does not weaken any of those rules. It adds only immutable module values.

### RFC 0004 (module taxonomy and capability boundaries)

Consistent with module-local defaults.

Top-level lets are private module state unless and until a separate RFC adds value export surface. That keeps the first implementation aligned with Kyokara's existing private-by-default story.

## Alternatives considered

### A1. Keep top-level `let` parse-only

Pros:

1. No implementation work.

Cons:

1. The surface lies about what the language supports.
2. Diagnostics cascade in confusing ways.
3. Users lose a natural immutable module-constant feature.

Decision: reject.

### A2. Add top-level `var` at the same time

Pros:

1. More imperative familiarity.

Cons:

1. Reopens global mutable state design.
2. Conflicts with RFC 0013's local-only scope for `var`.
3. Requires a much larger model around initialization, mutation visibility, and runtime isolation.

Decision: reject.

### A3. Make top-level lets lazy

Pros:

1. Could permit some forward-looking dependency patterns.

Cons:

1. Harder to explain.
2. Introduces hidden execution order.
3. Makes runtime and debugging less predictable.

Decision: reject for phase 1.

### A4. Add `pub let` immediately

Pros:

1. Broader module API surface.

Cons:

1. Requires value export/import design, not just module-local semantics.
2. Expands this RFC well beyond issue #410.

Decision: defer.

## Rollout

1. Ratify top-level immutable `let` as a module-local feature.
2. Implement semantic checking, type checking, and runtime initialization.
3. Add regression tests for same-module reads, source-order behavior, and project-mode imported-function behavior.
4. Keep top-level `var` rejection as-is under RFC 0013.

## Acceptance criteria

1. `let x = expr` works at top level.
2. `let x: T = expr` works at top level.
3. Same-module functions can read top-level lets.
4. Later top-level lets can read earlier top-level lets.
5. Earlier top-level lets cannot read later top-level lets.
6. Imported functions in project mode can read top-level lets from their own module.
7. Top-level `var` remains rejected.

## Open questions

1. Should a later RFC add `pub let` and value imports, or should public constants use functions for the foreseeable future?
2. If value exports are added later, should imported values preserve eager source-order initialization or require a stricter module-initialization contract?
