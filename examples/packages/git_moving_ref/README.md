# Git Moving Ref

This example demonstrates a git dependency declared with `rev = "main"`, then refreshed after the upstream branch advances.

Layout:

- `app/` is the consuming package
- `git-json/` is a plain package directory that should be initialized as a git repo

Manual run:

```sh
python - <<'PY'
from pathlib import Path
path = Path("app/kyokara.toml")
text = path.read_text()
path.write_text(text.replace('git = "../git-json"', f'git = "{Path("git-json").resolve()}"'))
PY
git -C git-json init -q -b main
git -C git-json config user.name "Kyokara Examples"
git -C git-json config user.email examples@kyokara.invalid
git -C git-json add .
git -C git-json commit -q -m init
cargo run -q -p kyokara-cli -- run app/src/main.ky
printf 'pub fn from_git() -> Int { 8 }\n' > git-json/src/lib.ky
git -C git-json add .
git -C git-json commit -q -m update
cargo run -q -p kyokara-cli -- update app/src/main.ky
cargo run -q -p kyokara-cli -- run app/src/main.ky
```

Expected behavior:

- the first run prints `7`
- `app/kyokara.lock` preserves `rev = "main"` and records a `commit = "<sha>"`
- after `kyokara update`, the second run prints `8`
