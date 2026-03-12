# RFC 0004: Module Taxonomy and Capability Boundaries

- Status: Implemented
- Owner: Language Design
- Tracking issue: TBD
- Last updated: 2026-03-12

## Summary

Define one consistent mental model for API placement and authority checks:

1. Keep only ubiquitous pure value types in ambient scope.
2. Place specialized pure APIs, including all collection families, in pure modules with explicit import visibility.
3. Keep built-in effectful APIs in explicit modules such as `io` and `fs`.
4. Separate visibility from authority:
   - `import` controls whether a name can be referenced.
   - runtime manifest grants control whether an effect may execute.
   - source-level `with ...` remains the function-effect contract surface, not the visibility gate for built-in `io`/`fs` calls.

This RFC defines the shipped model on `main`.

## Motivation

Two different kinds of confusion showed up repeatedly before the collection visibility cleanup and capability work landed:

1. Specialized pure types growing the ambient surface made name lookup less predictable.
2. Users could easily confuse "I imported the module" with "I am authorized to perform the effect."
3. Old examples blurred built-in effect modules and user-defined effect contracts into one mechanism.

Kyokara needs one simple rule for:

1. where a name comes from,
2. when an effect call is allowed to run,
3. which concepts belong to `import`, `with`, and runtime manifests.

## Design Goals

1. Keep the default user-visible surface small and predictable.
2. Make specialized pure APIs discoverable through explicit modules.
3. Keep built-in effect modules explicit without pretending import is the same as authority.
4. Preserve RFC 0001 direction: owner-first methods, type-owned constructors, module-qualified no-owner APIs.

## Non-Goals

1. Redesigning effect typing itself.
2. Defining package/dependency boundaries.
3. Re-specifying exact collection method names or mutability semantics already frozen by RFC 0009 / RFC 0010 / RFC 0012.
4. Defining future domain modules exhaustively.

## Proposal

### P1. Two-axis model: Visibility vs Authority

Every call site follows this matrix:

| Surface category | Import required | Source-level `with` required | Manifest grant required |
|---|---|---|---|
| Ambient pure value APIs (`Int`, `String`, `Option`, `Result`, etc.) | No | No | No |
| Pure module APIs (`collections.*`, `math.*`, member imports from those modules, etc.) | Yes | No | No |
| Built-in effect module APIs (`io.*`, `fs.*`, etc.) | Yes | No | Yes |
| User-defined effect-annotated functions | Normal name-resolution rules | Yes | Yes |

For the last two rows, manifest authority is checked only when a manifest is present; no manifest preserves the current allow-all runtime behavior.

Key rules:

1. `import` controls visibility only.
2. Import never grants authority.
3. Built-in effect modules are authorized by the runtime manifest when one is present.
4. Source-level `with ...` declares function-effect contracts and participates in static effect checking.
5. Built-in capability names are strict, case-sensitive lowercase identifiers that match module names when applicable (`io`, `fs`).

### P2. Namespace tiers

#### Tier A: Ambient core

Keep only ubiquitous pure value APIs ambient:

1. `Int`, `Float`, `Bool`, `String`, `Char`, `Unit`
2. `Option`, `Result`, `ParseError`
3. traversal constructors such as `start..<end` and `seed.unfold(step)`

#### Tier B: Pure feature modules

Specialized pure APIs live in explicit modules.

Current shipped rule:

1. All collection family names are explicit under `collections`.
2. `from collections import List, MutableMap, ...` is the canonical local-binding style for repeated use.
3. `collections.X` and `import collections as c` / `c.X` remain valid explicit namespace paths.
4. Exact collection surfaces are defined by RFC 0009, RFC 0010, and RFC 0012 rather than duplicated here.

#### Tier C: Built-in effect modules

Built-in effect APIs remain module-qualified and import-visible:

1. `io.*`
2. `fs.*`
3. future built-in effect modules follow the same import + manifest model

These calls compile once the module is imported. When a manifest is present, execution is deny-by-default unless the required capability is granted.

#### Tier D: User-defined effect contracts

`with ...` remains the source-level effect contract surface for user-defined capabilities.

Example:

1. `fn write(msg: String) -> Unit with Audit { ... }`
2. callers must satisfy the static effect checker for `Audit`
3. when a manifest is present, runtime also checks that `Audit` is granted before entering that function

### P3. Collection visibility contract

Collection families are not ambient globals.

Valid visibility paths:

1. `import collections` then `collections.List<T>` / `collections.List.new()`
2. `import collections as c` then `c.List<T>` / `c.List.new()`
3. `from collections import List` then `List<T>` / `List.new()`

Once a value exists, behavior remains owner methods on the value itself. This RFC governs visibility and placement only; immutable vs mutable method naming belongs to RFC 0009, dense-bit specifics belong to RFC 0010, and priority-queue specifics belong to RFC 0012.

### P4. Canonical examples

Pure specialized collection use:

```kyokara
from collections import MutableDeque

fn queue_size() -> Int {
  let q = MutableDeque.new().push_back(1).push_back(2).to_deque()
  q.len()
}
```

Built-in effect module use:

```kyokara
import io

fn main() -> Unit {
  io.println("ok")
}
```

Behavior:

1. This compiles once `io` is imported.
2. With no manifest, it runs.
3. With `--caps`, the manifest must grant `io` or the runtime denies the effect.

Visible but unauthorized built-in effect:

```kyokara
import io

fn main() -> Unit {
  io.println("no")
}
```

This is not a missing-`with` compile error. Under `--caps` without `io`, it fails at runtime with a capability-denied error.

User-defined effect contract:

```kyokara
effect Audit

fn record(msg: String) -> String with Audit {
  msg
}

fn main() -> String with Audit {
  record("ok")
}
```

Here `with Audit` is the function-effect contract surface. The static effect checker enforces it regardless of whether any built-in module is involved.

## RFC 0001 alignment

This RFC clarifies RFC 0001 without changing its core laws:

1. L2/L5 remain: owner methods + type/module namespace placement.
2. L4 remains: effects are capability-scoped.
3. L16 remains: core behavior binds by identity, not names.
4. Practical policy: ambient scope is reserved for ubiquitous pure value APIs; specialized pure APIs prefer modules.

## Acceptance Criteria

1. The import/authority model is documented as:
   - import for visibility,
   - manifest for built-in runtime authority,
   - `with` for function-effect contracts.
2. Collection families are explicit under `collections`, not ambient globals.
3. Built-in effect module calls require import visibility and, when a manifest is present, matching manifest authority, but not source-level `with io` / `with fs`.
4. User-defined effect contracts continue to use `with ...` and E0011-style static checking, with manifest checks applying when a manifest is present.
5. Examples and diagnostics reflect one canonical model.

## Alternatives Considered

### A1. Keep all collections ambient

Pros:

1. Shortest call-site spelling.

Cons:

1. Grows global surface continuously.
2. Reduces predictability for name resolution, completions, and AI generation.

Decision: rejected.

### A2. Make import also grant authority

Pros:

1. Fewer concepts to explain.

Cons:

1. Collapses visibility and authority into one mechanism.
2. Makes capability boundaries less explicit and less auditable.

Decision: rejected.

### A3. Require `with io` / `with fs` as the built-in module visibility gate

Pros:

1. Superficially resembles user-defined effect declarations.

Cons:

1. Confuses built-in module import with effect-contract declarations.
2. Does not match the shipped implementation.
3. Adds a second visibility-like gate where import already serves that purpose.

Decision: rejected.
