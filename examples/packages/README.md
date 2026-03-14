# Package Examples

These are actual package-layout examples for the package manager surface in RFC 0015.

They are also covered by the CLI integration harness:

```sh
cargo test -q -p kyokara-cli package_example_
```

Included examples:

- `registry_selected_closure/`
  Demonstrates adding a registry dependency while a stale higher version already exists in the local cache. The vendored transitive manifest should pin to the exact selected version.
- `git_moving_ref/`
  Demonstrates a git dependency declared as `rev = "main"`, then refreshed after the upstream branch advances.

