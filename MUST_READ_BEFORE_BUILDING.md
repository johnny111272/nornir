# Building Nornir

Do NOT run `cargo build --release` or `maturin build` directly.
Use `nornir_deploy` instead.

## Why

1. **Dependency coherence.** When a shared crate changes (e.g., `datagram_io`, `schemas_embedded`), ALL consumers must rebuild together. Running `cargo build --release -p one_crate` leaves other binaries in the same category running stale code. Strange runtime bugs with no obvious cause.

2. **Deployment.** `cargo build` puts binaries in `target/release/`. They are not deployed until symlinked to `~/.ai/tools/bin/`. The deploy tool handles build, symlink, and verification as one atomic operation.

3. **Gates require maturin.** PyO3 gate modules cannot be built with `cargo build`. They need `maturin build`, wheel extraction, and `.so` deployment to `~/.ai/tools/lib/`. The deploy tool handles all of this.

4. **Schema embedding.** Schemas are embedded at compile time via `include_str!()`. A changed `.schema.json` has no effect until the consuming binary is rebuilt and redeployed.

## Commands

```
nornir_deploy --all                  # rebuild + deploy everything
nornir_deploy --non-pyo3             # all cargo crates, skip maturin gates
nornir_deploy --build hooks          # single category
nornir_deploy --build hooks,writers  # multiple categories
nornir_deploy --list                 # show categories and crate counts
```

Categories are defined in `deploy_categories.toml`.

## Adding a new crate

1. Add the crate to `deploy_categories.toml` under the appropriate category.
2. Run `nornir_deploy --build <category>`.

## What is safe to run directly

- `cargo check` — compilation checking, no deployment
- `cargo test` — run tests
- `cargo clippy` — lint checking
- `cargo build` (debug, no `--release`) — compilation checking, produces debug binaries only

A hook intercepts `cargo build --release` and `maturin build` to remind you.
