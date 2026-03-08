# RFC 0011: Static Trait System and Constraint Semantics

- Status: Draft
- Owner: Language Design
- Tracking issue: [#253](https://github.com/kyokaralang/kyokara/issues/253)
- Last updated: 2026-03-08

## Summary

Add a static trait system to Kyokara as the canonical replacement for hardcoded type-class-like allowlists.

This RFC defines:

1. Explicit `trait` declarations.
2. Explicit `impl` blocks.
3. Generic trait bounds using `T: Trait` syntax.
4. Optional `deriving(...)` for eligible nominal types.
5. Qualified trait calls such as `Ord.compare(a, b)`.
6. Static trait resolution with deterministic coherence rules.

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
5. Preserve Kyokara's one-obvious-way API law by keeping dot-call syntax inherent-only.
6. Freeze the permanent trait surface now so implementation can proceed without reopening method-resolution policy later.

## Non-goals

1. Dynamic trait objects or existentials in phase 1.
2. Runtime reflection over traits.
3. Specialization, negative impls, or open-ended blanket impls such as `impl<T: Show> Show for T` in phase 1.
4. Trait methods participating in ordinary dot-call lookup.
5. Solving all numeric-semantic edge cases in the first implementation phase.
6. Designing collection-specific APIs in this RFC.

## Proposal

### P1. Syntax and grammar additions

This RFC adds the following language surface:

1. New reserved keywords: `trait`, `impl`, and `deriving`.
2. `Self` is reserved in trait declarations and impl blocks as the self-type placeholder.
3. New item kinds: trait declarations and impl blocks.
4. A derive clause on nominal type declarations.

Canonical grammar additions:

```peg
Keyword          <- ... / 'trait' / 'impl' / 'deriving'

TraitRef         <- Path

Item             <- 'pub'? (TypeDef
                   / TraitDef
                   / FnDef
                   / EffectDef
                   / PropertyDef
                   / LetBinding)
                 / ImplDef

TypeDef          <- 'type' Ident TypeParamList? DeriveClause? '=' TypeBody
DeriveClause     <- 'deriving' '(' TraitRef (',' TraitRef)* ','? ')'

TraitDef         <- 'trait' Ident TypeParamList? SupertraitList? '{' TraitMethodSig* '}'
SupertraitList   <- ':' TraitRef ('+' TraitRef)*
TraitMethodSig   <- 'fn' Ident ParamList ReturnType?

ImplDef          <- 'impl' TypeParamList? TraitRef 'for' TypeExpr '{' ImplMethodDef* '}'
ImplMethodDef    <- 'fn' Ident ParamList ReturnType? BlockExpr
```

Notes:

1. `for` is already reserved elsewhere in the language, so impl syntax reuses the existing token.
2. The grammar above is the normative surface contract; exact parser production factoring may differ internally.
3. `deriving(...)` attaches only to nominal `type` declarations, never to anonymous structural records.
4. Qualified trait calls reuse the existing qualified-call syntax surface; this RFC changes resolution policy, not the punctuation of calls.
5. `impl` blocks are not independently `pub`; visibility is attached to traits, types, and ordinary items, not to impl blocks themselves.

### P2. Trait declaration surface

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
4. Trait declarations define capability names and method signatures; they do not, by themselves, add ordinary dot-call methods to values.

### P3. Impl surface

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
5. Impl blocks may contain only implementations of the target trait's required methods in phase 1; extra inherent methods must be declared separately as ordinary `fn TypeName.method(...)` items.

### P4. Trait invocation surface

Trait methods are invoked only through qualified trait namespace calls:

```kyokara
Ord.compare(a, b)
Eq.eq(x, y)
Hash.hash(key)
Show.show(value)
```

Rules:

1. Ordinary dot-call syntax resolves inherent methods only.
2. Trait methods do not participate in general dot lookup or dot completion.
3. Imports affect trait names in bounds and qualified trait calls only; they never change which `x.foo()` calls are available.
4. This separation is permanent surface policy, not a temporary implementation restriction.
5. Operators and stdlib internals may lower to trait semantics in later phases without changing the call surface rule above.

Reason:

1. Kyokara keeps one obvious method surface: dot calls are value-owned API, while traits remain explicit capability namespaces.
2. This avoids inherent-vs-trait precedence rules, trait-import-sensitive method lookup, and other secondary resolution surfaces.

Canonical usage summary:

```kyokara
type Point deriving (Eq, Ord, Hash, Show) = { x: Int, y: Int }

fn clamp<T: Ord>(x: T, lo: T, hi: T) -> T {
  if (Ord.compare(x, lo) < 0) {
    lo
  } else if (Ord.compare(x, hi) > 0) {
    hi
  } else {
    x
  }
}

fn Point.to_string(self) -> String {
  "(".concat(self.x.to_string()).concat(", ").concat(self.y.to_string()).concat(")")
}

fn debug_point(p: Point) -> String {
  Show.show(p)
}

// Invalid in this RFC:
// p.show()
```

### P5. Trait bounds

Trait bounds use inline `T: Trait` syntax:

```kyokara
fn contains<K: Hash + Eq, V>(m: Map<K, V>, key: K) -> Bool { ... }
fn sort<T: Ord>(xs: List<T>) -> List<T> { ... }
```

Phase-1 rule:

1. Inline bounds are canonical.
2. `where`-clause trait bounds are deferred.
3. Bound checking is compile-time only.
4. Bounds govern conformance and generic validity; they do not implicitly expose trait methods through dot syntax.

### P6. Deriving

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
2. Anonymous structural record literals do not receive user-written impls or derives in phase 1.
3. If a user wants `Eq` / `Ord` / `Hash` / `Show` conformance for record-shaped data, they must introduce a nominal alias and derive or implement the traits on that alias.

Structural-record boundary example:

```kyokara
type Point deriving (Eq, Hash) = { x: Int, y: Int }

let ok: Set<Point> = ...
// Rejected in phase 1: Set<{ x: Int, y: Int }>
```

### P7. Semantic laws

Trait conformance is not just syntactic; implementations must satisfy the following laws.

`Eq`:

1. Reflexive: `Eq.eq(x, x)` is always `true`.
2. Symmetric: `Eq.eq(x, y) == Eq.eq(y, x)`.
3. Transitive: if `Eq.eq(x, y)` and `Eq.eq(y, z)`, then `Eq.eq(x, z)`.

`Ord`:

1. Total: every pair of values is comparable.
2. Antisymmetric: `Ord.compare(x, y) == 0` iff `Eq.eq(x, y)`.
3. Transitive: the ordering relation is transitive.
4. Consistent with `Eq`: if `Eq.eq(x, y)`, then `Ord.compare(x, y) == 0`.

`Hash`:

1. Equality-consistent: if `Eq.eq(x, y)`, then `Hash.hash(x) == Hash.hash(y)`.
2. Deterministic: repeated hashing of the same value within the same program semantics yields the same result.
3. `Hash` is for semantic lookup and indexing, not cryptographic guarantees.

`Show`:

1. Output must be deterministic for a given value.
2. Derived `Show` should prefer stable, source-oriented formatting suitable for diagnostics and debugging.
3. `Show` is not required to be a parse-roundtrip format in phase 1.

### P8. Resolution and coherence

Trait resolution is static and explicit.

Rules:

1. Imports make traits and impl-bearing types visible for bounds and qualified calls; imports do not change which impl is selected.
2. At most one applicable impl may exist for a given `(Trait, SelfType)` after substitution.
3. Ambiguous impl sets are compile errors.
4. The coherence rule for phase 1 is local-owner or stricter: an impl is legal only if the trait or the outermost self-type constructor is defined in the current project.
5. Generic impls over a named local or builtin outer constructor are allowed, for example `impl<T: Eq> Eq for List<T>`.
6. Open-ended blanket impls whose self type is just a type parameter, or which would apply to arbitrary unrelated self types, are not allowed in phase 1, for example `impl<T: Show> Show for T`.
7. The project may not define overlapping impls, including overlap introduced through generic substitution.
8. There is no runtime instance search or ambient dictionary selection.
9. Builtin/core impls are owned by the language implementation and participate in coherence as if defined by the standard project, not by user code.

### P9. Builtin trait set and phase split

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

### P10. Builtin conformances

Phase-1 builtin defaults:

1. `Int`, `Bool`, `String`, `Char`, `Unit` implement `Eq`, `Ord`, `Hash`, and `Show` where semantically valid.
2. `Float` implements `Show` in phase 1 and participates in later partial-order work under `PartialEq` / `PartialOrd`.
3. Builtin container constraints use traits, not hardcoded allowlists:
   - `Map<K, V>` and `Set<T>` require `K: Hash + Eq` / `T: Hash + Eq`
   - ordered collection operations require `Ord`
4. Existing builtin methods such as `to_string()` remain surface conveniences; `Show` becomes the underlying capability model for future generalized display.
5. Anonymous structural records do not implicitly satisfy `Eq` / `Ord` / `Hash` / `Show` in phase 1.

Phase-1 compatibility note:

1. Current primitive float operators and float sort behavior remain temporary compatibility behavior until the later numeric-semantics phase retargets them cleanly.
2. This RFC locks the architectural home for that later work instead of forcing a second trait redesign.

### P11. Compiler/runtime integration targets

The first implementation phase under this RFC must absorb current hardcoded constraint locations for:

1. `Map` / `Set` hashability checks.
2. `List.sort()` and `binary_search()` orderability checks.
3. Trait-bounded type inference and diagnostics.
4. Derived conformance generation for user nominal types.
5. Qualified trait-call resolution and lowering.

Later phases under the same RFC may extend trait-driven behavior to:

1. equality and comparison operators,
2. formatting / interpolation,
3. broader generic stdlib APIs.

### P12. Diagnostics

The trait system must provide explicit diagnostics for:

1. missing required trait bounds,
2. unknown traits,
3. duplicate or conflicting impls,
4. illegal orphan/coherence violations,
5. failed derives due to missing field/payload conformances,
6. attempted use of deferred features such as trait objects,
7. attempted dot-call use of trait methods,
8. attempted use of anonymous structural records where nominal trait conformance is required.

Diagnostics should stay structured and mechanically actionable, consistent with RFC 0001.

## Phase-1 implementation contract

Phase 1 is successful when:

1. user-defined nominal types can derive or implement `Eq` / `Ord` / `Hash` / `Show`,
2. `Map` / `Set` no longer depend on hardcoded key allowlists for conforming user types,
3. `List.sort()` / `binary_search()` can accept conforming user types,
4. trait bounds participate in type-checking and error reporting,
5. trait invocation is fully specified as qualified-only and does not reopen ordinary method lookup,
6. semantic laws for `Eq` / `Ord` / `Hash` / `Show` are explicit,
7. no runtime trait objects or dynamic dispatch are required.

## Deferred features

These are deliberately deferred beyond phase 1 but remain compatible with this RFC:

1. trait objects / existential values,
2. default method bodies,
3. specialization,
4. negative impls,
5. broad blanket impls beyond named local or builtin outer constructors,
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

### RFC 0012

RFC 0012 depends on this RFC for `Ord`-based priority typing.

## Acceptance criteria

1. The trait surface is fully specified: `trait`, `impl`, bounds, and `deriving(...)`.
2. Coherence and resolution rules are explicit and non-ambient.
3. Phase-1 trait usage is sufficient to replace current ad-hoc collection constraint checks.
4. Deferred features are explicitly listed so later phases extend, rather than redesign, the model.
5. Priority-queue design can depend on `Ord` without reopening the trait surface.
