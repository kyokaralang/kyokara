# RFC 0008: MutableMap and MutableSet Surface (Follow-up to RFC 0005)

- Status: Draft
- Owner: Language Design
- Tracking issue: [#365](https://github.com/kyokaralang/kyokara/issues/365)
- Last updated: 2026-03-06

## Summary

Define the v0 surface for `MutableMap<K, V>` and `MutableSet<T>` using RFC 0005 policy as the baseline:

1. Explicit `Mutable*` naming.
2. Module-qualified constructors under `collections.*`.
3. No user-facing free-function aliases.

This RFC extends mutability coverage beyond `MutableList` without changing the naming/placement law.

## Context

RFC 0005 intentionally scoped to `MutableList` and marked additional mutable collections as follow-up RFC work. This document is that follow-up for map/set workloads.

## Design goals

1. Keep immutable defaults (`Map`, `Set`) intact.
2. Expose explicit mutable alternatives for alias-heavy update loops.
3. Keep constructor placement predictable and non-prelude (`collections.*`).
4. Preserve deterministic output order for `keys()/values()`.
5. Preserve static invalid key/element diagnostics.

## Proposal

### P1. Types and constructors

Add builtin types:

1. `MutableMap<K, V>`
2. `MutableSet<T>`

Constructor surface:

1. `import collections`
2. `collections.MutableMap.new()`
3. `collections.MutableSet.new()`

No prelude-global constructor aliases.

### P2. Canonical method surface

`MutableMap` (tentative canonical set):

1. `insert(key, value)`
2. `get(key) -> Option<V>`
3. `contains(key) -> Bool`
4. `remove(key)`
5. `len() -> Int`
6. `is_empty() -> Bool`
7. `keys()`
8. `values()`

`MutableSet` (tentative canonical set):

1. `insert(value)`
2. `contains(value) -> Bool`
3. `remove(value)`
4. `len() -> Int`
5. `is_empty() -> Bool`
6. `values()`

### P3. Semantics

1. Alias-visible mutation semantics (same class as `MutableList`).
2. Deterministic iteration order for `keys()/values()`.
3. Compile-time key/element validity checks aligned with immutable `Map`/`Set` diagnostics.

## Out of scope

1. Capability gates for mutable collections (remain pure APIs).
2. Concurrent/shared-memory collection models.
3. Prelude expansion of mutable constructors.

## API Law alignment

RFC 0001 and RFC 0005 remain authoritative:

1. Canonical naming over synonyms.
2. Module-qualified constructor placement.
3. Avoid legacy/free-function alias surfaces.

## TDD acceptance criteria

1. RED-first tests for valid and invalid paths in eval/api/cli layers.
2. `check`/`run` parity fixtures for constructor resolution and compile-diagnostic gating.
3. Alias semantics tests for both `MutableMap` and `MutableSet`.
4. Deterministic order tests for `keys()/values()`.
5. Clippy/lint clean for touched crates.

