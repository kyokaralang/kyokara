# RFC 0010: Rust-like Trait System and Constraint Semantics

- Status: Draft
- Owner: Language Design
- Tracking issue: [#253](https://github.com/kyokaralang/kyokara/issues/253)
- Last updated: 2026-03-07

## Summary

Add a Rust-like trait system to Kyokara as the canonical replacement for hardcoded type-class-like allowlists.

This RFC defines:

1. Explicit `trait` declarations.
2. Explicit `impl` blocks.
3. Generic trait bounds using `T: Trait` syntax.
4. Optional `deriving(...)` for eligible nominal types.
5. Static trait resolution with deterministic coherence rules.

The immediate implementation target is the subset needed to replace current ad-hoc `hashable` / `sortable` checks and to give `Map` / `Set` / ordered collection APIs first-class type constraints. The RFC also fixes the long-term surface so later phases do not redesign the model.

## Motivation

Kyokara currently encodes several semantic constraints directly in compiler internals:

1. `Map` / `Set` keys and elements use hardcoded hashability allowlists.
2. `List.sort()` / `binary_search()` use hardcoded orderability allowlists.
3. Equality and comparison behavior are partly hardcoded to primitive families.
4. User-defined types cannot opt into these capabilities.

This is inconsistent with Kyokara's design goals:

1. rules should be explicit and mechanically checkable,
2. library constraints should be representable in the language itself,
3. user-defined types should be able to participate in core APIs without compiler edits.

## Design goals

1. Keep the surface explicit and static.
2. Make conformance discoverable from source, not ambient resolution.
3. Support trait-bounded stdlib APIs without introducing trait objects or runtime dictionary lookup in v1.
4. Give user-defined nominal types a canonical path to `Eq` / `Ord` / `Hash` / `Show`.
5. Preserve room for phased implementation without revisiting the language model.

## Non-goals

1. Dynamic trait objects or existentials in phase 1.
2. Runtime reflection over traits.
3. Specialization, negative impls, or open-ended blanket impls in phase 1.
4. Solving all numeric-semantic edge cases in the first implementation phase.
5. Designing collection-specific APIs in this RFC.

## Proposal

### P1. Trait declaration surface

Traits are declared explicitly:

```kyokara
pub trait Eq {
  fn eq(self, other: Self) -> Bool
}

pub trait Ord: Eq {
  fn compare(self, other: Self) -> Int
}

pub trait Hash: Eq {
  fn hash(self) -> Int
}

pub trait Show {
  fn show(self) -> String
}
```

Rules:

1. Trait methods are declarations only in phase 1; default method bodies are deferred.
2. Supertrait requirements use `:` with `+` for multiple parents when needed.
3. Traits live in the same namespace tier as other type-level items; imports control visibility, not resolution authority.

### P2. Impl surface

Conformance is declared explicitly:

```kyokara
impl Eq for Int {
  fn eq(self, other: Self) -> Bool { ... }
}

impl<T: Eq> Eq for Box<T> {
  fn eq(self, other: Self) -> Bool { ... }
}
```

Rules:

1. Impl blocks are the only user-written way to establish trait conformance.
2. Generic impl headers may carry bounds.
3. Method signatures in an impl must exactly match the trait declaration after substituting `Self` and type parameters.
4. All required methods must be implemented in phase 1.

### P3. Trait bounds

Trait bounds use Rust-like inline syntax:

```kyokara
fn contains<K: Hash + Eq, V>(m: Map<K, V>, key: K) -> Bool { ... }
fn sort<T: Ord>(xs: List<T>) -> List<T> { ... }
```

Phase-1 rule:

1. Inline bounds are canonical.
2. `where`-clause trait bounds are deferred.
3. Bound checking is compile-time only.

### P4. Deriving

Nominal record and ADT types may opt into synthesized conformances:

```kyokara
type Point deriving (Eq, Ord, Hash, Show) = { x: Int, y: Int }

type Token deriving (Eq, Hash, Show) =
  | IntLit(Int)
  | Ident(String)
```

Derive rules:

1. Deriving is explicit; there is no automatic conformance for user-defined types.
2. A derive succeeds only if every contained field or payload already satisfies the required trait bounds.
3. Derived `Eq` compares variant/tag first, then fields/payloads structurally.
4. Derived `Ord` uses declaration order for variants, then lexicographic field/payload comparison.
5. Derived `Hash` includes variant/tag identity and field/payload hashes in declaration order.
6. Derived `Show` emits a stable source-oriented textual form suitable for diagnostics and debugging.

Phase-1 restriction:

1. Deriving applies to nominal ADTs and nominal record aliases.
2. Anonymous structural record literals do not receive user-written impls in phase 1.

### P5. Resolution and coherence

Trait resolution is static and explicit.

Rules:

1. Imports make traits and impl-bearing types visible; imports do not change which impl is selected when multiple candidates would exist.
2. At most one applicable impl may exist for a given `(Trait, Type)` after substitution.
3. Ambiguous impl sets are compile errors.
4. The coherence rule for phase 1 is Rust-like or stricter: an impl is legal only if the trait or the self type is defined in the current project.
5. The project may not define overlapping impls.
6. There is no runtime instance search or ambient dictionary selection.

### P6. Builtin trait set and phase split

Core named traits in this RFC:

1. `Eq`
2. `Ord`
3. `Hash`
4. `Show`

To preserve existing numeric behavior without overloading `Eq` / `Ord`, the architecture also reserves:

1. `PartialEq`
2. `PartialOrd`

Phase split:

1. Phase 1 must fully support `Eq`, `Ord`, `Hash`, and `Show` for collection and conformance use.
2. `PartialEq` / `PartialOrd` are part of the long-term architecture so float-compatible operator semantics have a stable home.
3. Numeric-operator migration onto the trait system can land in a later implementation phase without redesigning the trait surface.

### P7. Builtin conformances

Phase-1 builtin defaults:

1. `Int`, `Bool`, `String`, `Char`, `Unit` implement `Eq`, `Ord`, `Hash`, and `Show` where semantically valid.
2. `Float` implements `Show` in phase 1 and participates in later partial-order work under `PartialEq` / `PartialOrd`.
3. Builtin container constraints use traits, not hardcoded allowlists:
   - `Map<K, V>` and `Set<T>` require `K: Hash + Eq` / `T: Hash + Eq`
   - ordered collection operations require `Ord`
4. Existing builtin methods such as `to_string()` remain surface conveniences; `Show` becomes the underlying capability model for future generalized display.

Phase-1 compatibility note:

1. Current primitive float operators and float sort behavior remain temporary compatibility behavior until the later numeric-semantics phase retargets them cleanly.
2. This RFC locks the architectural home for that later work instead of forcing a second trait redesign.

### P8. Compiler/runtime integration targets

The first implementation phase under this RFC must absorb current hardcoded constraint locations for:

1. `Map` / `Set` hashability checks.
2. `List.sort()` and `binary_search()` orderability checks.
3. Trait-bounded type inference and diagnostics.
4. Derived conformance generation for user nominal types.

Later phases under the same RFC may extend trait-driven behavior to:

1. equality and comparison operators,
2. formatting / interpolation,
3. broader generic stdlib APIs.

### P9. Diagnostics

The trait system must provide explicit diagnostics for:

1. missing required trait bounds,
2. unknown traits,
3. duplicate or conflicting impls,
4. illegal orphan/coherence violations,
5. failed derives due to missing field/payload conformances,
6. attempted use of deferred features such as trait objects.

Diagnostics should stay structured and mechanically actionable, consistent with RFC 0001.

## Phase-1 implementation contract

Phase 1 is successful when:

1. user-defined nominal types can derive or implement `Eq` / `Ord` / `Hash` / `Show`,
2. `Map` / `Set` no longer depend on hardcoded key allowlists for conforming user types,
3. `List.sort()` / `binary_search()` can accept conforming user types,
4. trait bounds participate in type-checking and error reporting,
5. no runtime trait objects or dynamic dispatch are required.

## Deferred features

These are deliberately deferred beyond phase 1 but remain compatible with this RFC:

1. trait objects / existential values,
2. default method bodies,
3. specialization,
4. negative impls,
5. broad blanket impls,
6. `where` clauses for bounds,
7. operator retargeting for float-compatible partial-order semantics.

## RFC alignment

### RFC 0001

This RFC strengthens RFC 0001's explicitness and mechanically-checkable rules:

1. constraints become source-visible,
2. conformance becomes explicit,
3. stdlib behavior no longer depends on hidden allowlists.

### RFC 0009

This RFC is the prerequisite for any future ordered/hash-constrained collection that wants first-class user-defined key or priority types.

### RFC 0011

RFC 0011 depends on this RFC for `Ord`-based priority typing.

## Acceptance criteria

1. The trait surface is fully specified: `trait`, `impl`, bounds, and `deriving(...)`.
2. Coherence and resolution rules are explicit and non-ambient.
3. Phase-1 trait usage is sufficient to replace current ad-hoc collection constraint checks.
4. Deferred features are explicitly listed so later phases extend, rather than redesign, the model.
5. Priority-queue design can depend on `Ord` without reopening the trait surface.
