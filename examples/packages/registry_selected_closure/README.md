# Registry Selected Closure

This example demonstrates that a fresh `--registry` add uses the selected source-registry closure even when the local cache already contains a newer stale version.

Layout:

- `app/` is the consuming package
- `registry/` is the source-first registry store
- `stale-cache/` is an intentionally stale local cache seed

Manual run:

```sh
mkdir -p app/.kyokara/registry
cp -R stale-cache/packages app/.kyokara/registry/
cargo run -q -p kyokara-cli -- add app/src/main.ky core/util --as util --registry registry --version '=1.0.0'
cargo run -q -p kyokara-cli -- run app/src/main.ky
```

Expected behavior:

- `app/.kyokara/registry/packages/core/util/1.0.0/kyokara.toml` is rewritten to pin `json` to `=1.2.0`
- running the app prints `12`

