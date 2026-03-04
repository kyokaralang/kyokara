# RFC 0002: Collection-First Traversal Surface (Hide `Seq` from User API)

- Status: Accepted
- Owner: Language Design
- Tracking issue: TBD
- Last updated: 2026-03-04

## Summary

Adopt a single intentional user-facing traversal model:

1. Traversal methods are called directly on source values (`List`, `Deque`, `Map.keys()`, `Set.values()`, `String.lines()/chars()`, range/unfold sources).
2. `Seq` remains an internal execution type and is not part of canonical user API.
3. `xs.seq()` is removed from canonical surface (hard break in v0).

This RFC is about API clarity and pass@1 reliability for AI-generated code.
It does not add domain-specific helpers (for example interval/range-union helpers).

## Motivation

Current model requires an explicit bridge (`xs.seq()`) before traversal. This creates avoidable near-miss failures such as:

- `List.enumerate()` attempted by AI, but only `Seq.enumerate()` exists.

Observed impact:

1. Lower pass@1 due to surface-boundary misses.
2. Extra diagnostics/recovery burden for a predictable authoring pattern.
3. A fractured mental model for users (storage value first, traversal split second).

Design intent for v0 should be explicit: minimal surface, but not smaller than what predictable authoring needs.

## Scope

In scope:

1. User-visible traversal API shape.
2. Canonical spellings for traversal operations.
3. Diagnostics policy for boundary mistakes.
4. RFC 0001 amendment for storage/traversal law.

Out of scope:

1. Interval-specific stdlib additions (`merge_ranges`, `union`, etc.).
2. Runtime optimization/fusion redesign.
3. Constructor-surface syntax redesign (`..<` and universal `.unfold`) — moved to RFC 0003.

## Design Goals

1. One obvious way to do common traversal.
2. Keep API surface small and mechanically predictable.
3. Preserve expressiveness (all existing traversal capabilities stay available).
4. Keep internal runtime model intact where possible.

## Non-Goals

1. Exposing iterator protocol machinery directly (like Rust `Iterator::next`) to users.
2. Expanding stdlib with unrelated convenience helpers in this RFC.

## Proposal

### P1. User-visible traversal API

Traversal methods become directly callable on traversable sources.

Canonical user forms:

1. `xs.map(f)`
2. `xs.filter(f)`
3. `xs.enumerate()`
4. `xs.zip(ys)`
5. `xs.scan(init, f)`
6. `xs.chunks(n)`
7. `xs.windows(n)`
8. `xs.fold(init, f)`
9. `xs.any(f)`
10. `xs.all(f)`
11. `xs.find(f)`
12. `xs.count()`
13. `xs.to_list()`

These operations are available on traversal-capable values, including:

1. `List<T>`
2. `Deque<T>`
3. `Map.keys()` output
4. `Map.values()` output
5. `Set.values()` output
6. `String.split/lines/chars` outputs
7. range/unfold sources

### P2. `Seq` visibility policy

`Seq` is runtime/compiler internal and is not canonical in user API docs, examples, or diagnostics.

Consequences:

1. `xs.seq()` is removed from canonical user surface.
2. Existing internal execution model may still use `Seq` implementation types.
3. User-facing errors should not require understanding internal `Seq` to fix common code.

### P3. Source constructors

Keep source constructors minimal and explicit.

Required source constructors (user-surface):

1. integer range source
2. unfold source

Constructor spellings are defined by RFC 0003 (`start..<end` and `seed.unfold(step)`), while this RFC defines the collection-first traversal method model.

### P4. Hard-break policy (v0)

This is a hard API break in v0:

1. `xs.seq().map(...)` is non-canonical and should be rejected after transition window (or immediately, per v0 policy).
2. canonical form is `xs.map(...)`.

If immediate hard break is chosen, diagnostics must be direct and local.

### P5. Diagnostics contract

Method-resolution diagnostics must include canonical replacements for former boundary misses.

Examples:

1. `no method \`seq\` on List<T>; traversal methods are available directly (use \`list.map(...)\`)`
2. If compatibility mode exists: `\`seq()\` is deprecated; call traversal directly on the value`

### P6. Typing model (user-facing)

Traversal chains remain statically typed and lazy/evaluable per terminal, but users should not need explicit mention of internal engine types.

Key rule:

1. transform operations return traversal-capable chain values.
2. terminal operations execute the chain.
3. `to_list()` materializes.

## Before/After

### Example A: enumerate pipeline

Before:

```kyokara
let indexed = xs.seq().enumerate().map(f).to_list()
```

After:

```kyokara
let indexed = xs.enumerate().map(f).to_list()
```

### Example B: predicate terminal

Before:

```kyokara
let ok = ranges.seq().any(fn(r: IdRange) => id >= r.start && id <= r.end)
```

After:

```kyokara
let ok = ranges.any(fn(r: IdRange) => id >= r.start && id <= r.end)
```

## RFC 0001 Amendment

This RFC supersedes RFC 0001 `L18` as currently written.

Current `L18` requires explicit storage/traversal split via `xs.seq()`.

Replace with:

### L18 (Revised). Traversal model is collection-first and canonical (`MUST`)

1. Traversal operations are called directly on traversable source values.
2. Internal traversal engine types are not required in canonical user code.
3. Source constructors for traversal remain explicit and minimal.
4. Docs/examples/diagnostics must use collection-first canonical forms.

## Alternatives Considered

### A1. Keep current explicit split (`xs.seq()`) + improve diagnostics only

Pros:

1. smallest runtime/API change.

Cons:

1. preserves known pass@1 failure mode.
2. preserves fractured mental model.

### A2. Dual surface permanently (`xs.seq().map` and `xs.map`)

Pros:

1. smoother transition.

Cons:

1. violates one-canonical-spelling law.
2. increases long-term ambiguity for AI generation.

Decision: reject permanent dual surface.

## Rollout Plan

1. Introduce direct traversal methods on traversable sources.
2. Update docs/examples/tests to canonical direct traversal forms.
3. Enforce diagnostics for non-canonical `seq()` usage.
4. Remove/deprecate `seq()` per chosen v0 hard-break timing.

## Acceptance Criteria

1. Day-to-day traversal code never requires `seq()`.
2. Common previous near-miss (`List.enumerate`) is valid by construction.
3. No loss of traversal expressiveness compared with current `Seq` surface.
4. RFC 0001 law text updated to reflect canonical model.

## Follow-up

1. Constructor-surface details are captured in RFC 0003.
