# Documentation Index

This file defines the canonical documentation layout and ownership boundaries.
If a topic appears in multiple files, this index decides the source of truth.

## Canonical docs

| Topic | Source of truth | Notes |
|---|---|---|
| Project overview, quickstart, CLI examples | [`README.md`](../README.md) | Keep concise; link out for deep details. |
| Language/runtime design and roadmap | [`docs/design-v0.md`](design-v0.md) | Includes AI-first feature tracker and drift register. |
| Formal grammar and parser contract | [`spec/grammar.md`](../spec/grammar.md) | Parser-facing normative grammar. |
| API shape and naming law | [`docs/rfcs/0001-api-surface-law.md`](rfcs/0001-api-surface-law.md) | Normative API surface constraints. |
| Compiler API output contract | [`docs/rfcs/0007-optional-typed-ast-api-output.md`](rfcs/0007-optional-typed-ast-api-output.md) | Default diagnostics/holes/symbol_graph + optional `typed_ast` opt-in. |
| Traversal surface policy | [`docs/rfcs/0002-collection-first-traversal-surface.md`](rfcs/0002-collection-first-traversal-surface.md), [`docs/rfcs/0003-opaque-traversal-constructor-surface.md`](rfcs/0003-opaque-traversal-constructor-surface.md) | Collection-first methods + opaque constructor/type surface. |
| Mutable collection naming/placement | [`docs/rfcs/0005-mutable-collection-naming-and-placement.md`](rfcs/0005-mutable-collection-naming-and-placement.md) | Canonical `Mutable*` naming and `collections.*` placement rules. |
| Token-count workflow | [`docs/token-metrics.md`](token-metrics.md) | Evergreen process doc. |

## Archived and dated docs

| Type | Location | Policy |
|---|---|---|
| Dated reports | [`docs/reports/`](reports/) | Keep immutable snapshots with date-prefixed filenames. |
| Historical handover notes | removed (`HANDOVER.md`) | Do not reintroduce as a living doc; use issues/PRs + canonical docs instead. |

## Maintenance rules

1. New docs must either be canonical (added above) or archived snapshots under `docs/reports/`.
2. Avoid creating topic-specific tracker files when the topic already belongs in `docs/design-v0.md` or an RFC.
3. When adding or removing a canonical doc, update this file in the same PR.
