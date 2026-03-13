# RFC 0015: Package Manifests and External Library Distribution

- Status: Draft
- Owner: Language Design
- Tracking issue: [#440](https://github.com/kyokaralang/kyokara/issues/440)
- Depends on: RFC 0001, RFC 0004
- Last updated: 2026-03-13

## Summary

Define the first Kyokara package system for reusable libraries and reproducible dependency resolution.

This RFC locks:

1. `kyokara.toml` as the package manifest,
2. `kyokara.lock` as the resolved dependency lockfile,
3. source-first dependency distribution via registry, git, or local path,
4. a reserved `deps.<alias>...` import namespace for external packages,
5. preservation of the existing file-path module model inside each package,
6. separation of package visibility from capability authority.

## Motivation

Kyokara already has a workable local module story:

1. file path determines module path,
2. there is no source-level `module Path` declaration,
3. `pub` controls visibility,
4. `import path` binds a module namespace and `from path import Name` binds public members directly.

That is enough for one project, but not enough for a language ecosystem.

Current gaps:

1. there is no manifest that names a package or declares dependencies,
2. there is no reproducible lockfile,
3. there is no canonical way to reference external libraries from source,
4. there is no registry or distribution model for shared Kyokara libraries,
5. import resolution would become ambiguous if external packages reused the same flat module namespace as the current package.

The package system should solve those gaps without discarding the module rules that already make Kyokara predictable.

## Design goals

1. Preserve the current module mental model inside one package.
2. Make external dependencies explicit at the import site.
3. Keep package resolution reproducible and source-first.
4. Avoid conflating import visibility with runtime authority.
5. Support local path development, git pinning, and registry publishing.
6. Keep the first shipped package model small enough to implement cleanly.

## Non-goals

1. Binary artifact distribution in v1.
2. Native extension or FFI packaging.
3. Workspace or multi-package monorepo features.
4. Star imports, relative imports, or dynamic import behavior.
5. Solving documentation hosting, discovery ranking, or package scoring.

## Proposal

### P1. Package kinds and layout

Kyokara gains one package manifest file at the package root:

1. `kyokara.toml`

The first package kinds are:

1. `lib`
2. `bin`

Canonical layouts:

Library package:

```text
my-lib/
  kyokara.toml
  src/
    lib.ky
    math.ky
    text/slug.ky
```

Binary package:

```text
my-app/
  kyokara.toml
  src/
    main.ky
    cli.ky
    parse/input.ky
```

Rules:

1. `src/lib.ky` is the root module of a `lib` package.
2. `src/main.ky` is the entry module of a `bin` package.
3. Other `.ky` files under `src/` follow the existing file-path-to-module-path rule.
4. There is no source-level module declaration override inside a package.
5. Package-internal module imports remain exactly the same as today, relative to the package source root.

Example:

```kyokara
// src/text/slug.ky
pub fn slugify(s: String) -> String { s.trim().to_lower() }
```

```kyokara
// src/lib.ky
import text.slug

pub fn normalize_title(s: String) -> String { slugify(s) }
```

### P2. Manifest format

`kyokara.toml` defines package identity, package kind, edition, and dependencies.

Initial manifest shape:

```toml
[package]
name = "acme/slug"
version = "0.1.0"
edition = "2026"
kind = "lib"

[dependencies]
json = { package = "core/json", version = "^1.4.0" }
http = { git = "https://github.com/acme/http-kit", rev = "4e2f9b1" }
local_utils = { path = "../local-utils" }
```

Rules:

1. `package.name` is the published package ID.
2. Published package IDs are slash-qualified, for example `acme/slug`.
3. Dependency table keys are local aliases and must be valid Kyokara identifiers.
4. Import sites use the dependency alias, not the published package ID.
5. Exactly one source form is allowed per dependency:
   1. registry version,
   2. git,
   3. local path.
6. `bin` packages may depend only on `lib` packages.
7. `lib` packages may depend only on `lib` packages.

Reason for alias keys:

1. published names may contain `/` and `-`,
2. import syntax should stay identifier-based and predictable,
3. local aliases are the right place to resolve naming collisions.

### P3. External import namespace

External packages are imported through a reserved `deps` namespace.

Canonical examples:

```kyokara
import deps.json
import deps.http.client
import deps.local_utils.grid
```

Rules:

1. `deps` is not a user-defined module path.
2. The segment after `deps` is a dependency alias from `[dependencies]`.
3. Remaining path segments are module paths inside that dependency's `src/` tree.
4. `import foo.bar` continues to mean the current package's own module namespace path.
5. `from foo.bar import Name` continues to use the same local-package path rules, but binds public members directly.
6. External dependencies never share the same top-level import namespace as local modules or stdlib modules.

Examples:

```toml
[dependencies]
json = { package = "core/json", version = "^1.4.0" }
```

```kyokara
// current package
import parse
import deps.json.encode
from deps.json.encode import Encoder
```

That keeps this distinction obvious:

1. `parse` is ours,
2. `deps.json.encode` is third-party,
3. `Encoder` is a direct member import from that third-party module.

### P4. Imported surface and root modules

The dependency root import maps to `src/lib.ky`.

Examples:

1. `import deps.json` binds the dependency root module namespace from `src/lib.ky`.
2. `import deps.json.encode` binds the dependency module namespace from `src/encode.ky`.
3. `from deps.json.encode import Encoder` binds a public member from `src/encode.ky`.
4. `import deps.json.http.client` binds the namespace from `src/http/client.ky`.

This preserves the existing module rule inside each package:

1. file path determines module path,
2. source files do not declare or override module identity,
3. package boundary adds only the explicit `deps.<alias>` prefix.

### P5. Lockfile and resolution

Kyokara writes a lockfile named `kyokara.lock`.

Responsibilities:

1. record the fully resolved dependency graph,
2. pin exact versions or exact git revisions,
3. store registry checksums for reproducible installs,
4. make `build`, `run`, `test`, and `check` deterministic.

Current shipped v1 shape for local path dependencies:

```toml
version = 1

[dependencies]
json = { path = "../json-pkg" }
util = { path = "../util-pkg" }
```

Rules:

1. `kyokara.toml` expresses intent.
2. `kyokara.lock` records an exact resolution.
3. In the first shipped phase, `check`, `run`, and `test` sync `kyokara.lock` for package-root entries before project loading.
4. Dependency graph changes happen only when the user edits the manifest or runs an explicit update command.
5. The first shipped lockfile records local path dependency snapshots only; git revisions, registry versions, and checksums extend the same file in later phases.

### P6. Distribution model

The first package ecosystem is source-first.

Allowed sources:

1. registry packages,
2. git dependencies,
3. local path dependencies.

Registry packages are published as source bundles plus manifest metadata.

Reasons:

1. Kyokara already has an interpreter-first execution model,
2. source distribution keeps behavior portable across host environments,
3. contracts, diagnostics, refactors, and future codegen all benefit from having source available.

Binary caches may exist as an implementation detail later, but are not part of the package contract in this RFC.

### P7. Publishing constraints

The first publishing rules are intentionally narrow.

1. Only `lib` packages are publishable.
2. Published packages must have a `package.name` and `version`.
3. Packages with `path` dependencies are not publishable.
4. Git dependencies may remain allowed for unpublished internal use, but registry publishing should require the published graph to resolve through registry packages only.

This avoids shipping manifests that cannot be reproduced outside the author's machine or company network.

### P8. Capability model remains separate

Package imports do not grant capability authority.

Examples:

1. importing `deps.http.client` does not by itself grant `net`,
2. calling a dependency function that requires `with net` still requires that capability in the caller type and at runtime,
3. the runtime manifest still controls actual authority at execution time.

This preserves the visibility-versus-authority rule from RFC 0004:

1. imports control name visibility,
2. `with` and the manifest control authority.

### P9. Initial CLI direction

This RFC does not fully specify CLI UX, but the intended command family is:

1. `kyokara add <package> --as <alias>`
2. `kyokara update`
3. `kyokara build`
4. `kyokara run`
5. `kyokara test`
6. `kyokara publish`

Example:

```text
kyokara add core/json --as json
```

Expected result:

1. add a dependency entry under `[dependencies]`,
2. resolve and write `kyokara.lock`,
3. make the package available as `deps.json`.

## Incremental rollout

This RFC describes the target package architecture, but it is expected to land in phases rather than one all-at-once implementation.

Planned rollout shape:

1. Phase 0: refactor project loading around an explicit project/package graph boundary.
2. Phase 1: support `kyokara.toml`, package root detection, and `lib` / `bin` source roots.
3. Phase 2: support local path dependencies plus the reserved `deps.<alias>` import namespace.
4. Phase 3: make package-aware loading consistent across check/run/eval/API/LSP/refactor flows.
5. Phase 4: add `kyokara.lock` and deterministic resolution behavior.
6. Phase 5+: add remote dependency sources and package-management UX.

The first intended shippable package slice is:

1. package root + manifest parsing,
2. local path dependencies,
3. `deps.<alias>` imports,
4. no registry requirement for the first landing.

Registry publishing, git dependencies, and richer package-management commands are follow-up phases unless explicitly pulled earlier by a ratified scope decision.

## Canonical examples

### Example A: Authoring a library

```toml
[package]
name = "acme/slug"
version = "0.1.0"
edition = "2026"
kind = "lib"
```

```kyokara
// src/lib.ky
import text.slug

pub fn normalize_title(s: String) -> String { slugify(s) }
```

```kyokara
// src/text/slug.ky
pub fn slugify(s: String) -> String { s.trim().to_lower() }
```

### Example B: Consuming a registry dependency

```toml
[package]
name = "acme/reporter"
version = "0.1.0"
edition = "2026"
kind = "bin"

[dependencies]
json = { package = "core/json", version = "^1.4.0" }
```

```kyokara
import deps.json

fn main() -> String {
  encode_string_map("ok", "yes")
}
```

### Example C: Mixing local modules and dependencies

```kyokara
import parse
import deps.json.decode

fn load_config(src: String) -> Result<Config, String> {
  let parsed = parse(src)?
  let raw = parse_object(parsed)?
  from_json_object(raw)
}
```

The important part is not the specific function names. The important part is that the source visibly distinguishes:

1. current-package modules,
2. dependency modules.

## Rationale

### Why `deps.<alias>` instead of bare package names?

Because bare package names would collapse local modules and dependencies into the same namespace.

That creates bad ambiguity:

1. is `import json` local or external,
2. does a new dependency silently shadow a local module,
3. does a new local module silently hide a dependency?

`deps.<alias>` avoids that ambiguity entirely.

### Why keep source-first distribution?

Because Kyokara's current strengths already assume source availability:

1. precise diagnostics,
2. refactors,
3. contracts,
4. future type-driven tooling,
5. consistent interpreter and codegen behavior.

Compiled artifact caches can be added later without changing the package contract.

### Why package aliases in the manifest?

Because aliases solve three real problems:

1. published names do not need to satisfy identifier syntax,
2. teams can choose short local names,
3. collisions are resolved explicitly in the manifest instead of implicitly in source.

## RFC alignment

### RFC 0001

This RFC follows the API surface law:

1. modules remain the unit of path-based visibility,
2. external package qualification still happens in the path prefix (`deps.<alias>`),
3. `from ... import ...` is the same member-import mechanism used for local modules and type-owned variants.

### RFC 0004

This RFC preserves the visibility-versus-authority split:

1. `deps.*` controls visibility,
2. capabilities still control authority.

## Open questions

1. Should the first release allow both `lib` and `bin` targets in one package, or keep one package = one target kind?
2. Should `kyokara publish` require a clean git tree and tagged version, or leave release policy to the registry?
3. Should registry package IDs always be namespaced like `owner/name`, or should single-segment names be reserved for a central official namespace?
4. Should dependency aliases be formatter-stable if the manifest key changes, or should source imports be rewritten by a refactor command?
5. Should dev-only dependencies and test-only dependencies be part of the first package RFC or a follow-up?

## Acceptance criteria

1. A package manifest format is documented.
2. A lockfile format and its role are documented.
3. External dependency imports have one unambiguous canonical surface.
4. Registry, git, and path dependency sources are defined.
5. Package imports remain separate from capability authority.
6. The proposal preserves the existing file-path module model inside each package.
