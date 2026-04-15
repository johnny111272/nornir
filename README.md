# Nornir

Validation-as-code Rust monorepo. Produces compiled binaries, PyO3 gate modules, and libraries for schema-driven LLM agent infrastructure.

## What This Is

Nornir is a Rust workspace containing 97 crates organised in a strict three-tier architecture:

- **Core** (pure, no I/O) — type definitions, validation engines, formatting, diffing, error types
- **Capability** (I/O boundary) — file operations, schema embedding, hook protocols, datagram emission
- **Binaries** — CLI tools, pipeline gates, hooks, watchers, senders, a reverse proxy daemon

Everything that needs to be compiled, fast, or trusted lives here. Python projects in the ecosystem (Draupnir, Regin, Galdr, Bifrost) call into Nornir's compiled artifacts — either as PyO3 modules imported directly into Python, or as standalone binaries.

## Key Subsystems

### Pipeline Gates

35 compiled Rust/PyO3 gate modules that enforce JSON Schema validation at every stage boundary of the [agent definition pipeline](#the-agent-definition-pipeline). Each gate embeds its schema at compile time — self-contained validators with no runtime dependencies. Gates handle all file I/O for the Python pipeline stages: Python code between gates transforms data, gates read and write it.

### Gleipnir — Code Quality Detection

Tree-sitter-based AST guardrail engine. Statically analyses source files (Python, Rust, TypeScript, Svelte) for structural violations. Pure computation — receives source bytes, returns typed Violation structs. No I/O, no disk access, no policy decisions. Gleipnir is the detection layer only.

Every check produces educational messages with three fields: **signal** (what was detected), **direction** (how to address it), and **canary** (how to detect if the fix is superficial rather than genuine). Violations are not style complaints — they are structural indicators. Function length signals accumulated responsibilities. Short names signal that the author did not think about the reader. Print statements in library code signal misunderstanding of orchestration boundaries.

### Saga — Quality Truth Recording

Runs analysis tools (Gleipnir for AST checks, Ruff for Python linting, Basedpyright for type checking) on source files and writes unfiltered `.qa` sidecar reports adjacent to each source file. Saga records every issue found — no filtering, no policy. Raw truth only. It is the only component that generates `.qa` files.

### Syn — Quality Policy Enforcement

Reads `.qa` sidecar files and applies configurable three-tier filtering and enforcement policy. Report mode provides an informational view with ad-hoc filter overrides. Gate mode provides deterministic per-file enforcement locked to configuration only — no CLI overrides — ensuring reproducible accept/reject decisions in automated hooks.

The separation between detection (Gleipnir), recording (Saga), and policy (Syn) is strict. Each can evolve independently, and multiple consumers can read the same truth data at zero analysis cost.

### Hook Binaries

Compiled Rust binaries that handle pre-execution security gating, post-execution quality assessment, session initialisation, and context re-injection. Each hook receives JSON on stdin from the host environment and returns structured decisions (allow/warn/ask/deny). Security gating enforces layered rules: permanent floor rules that block access to credentials regardless of configuration, configurable probing detection, and gaming detection for circumvention attempts.

### Bifrost Proxy

An async Rust reverse proxy (hyper/tokio) that intercepts LLM API traffic in both directions. Validates requests against an embedded wire-format schema, classifies traffic by type, rewrites compaction requests with injected instructions, and parses SSE response streams to strip and side-log thinking blocks. Designed as the vendor-independent enforcement layer for agent supervision.

## The Agent Definition Pipeline

Nornir's gates are one stage of a five-stage pipeline that transforms authored agent definitions into deployable, auditable agent artifacts:

**[Verdandi](https://github.com/johnny111272/verdandi)** (type system) → **[Draupnir](https://github.com/johnny111272/draupnir)** (schema generation) → **Nornir Gates** (compiled validation) → **[Regin](https://github.com/johnny111272/regin)** (pipeline resolution) → **Galdr** (composition and render)

Each stage produces typed artifacts consumed by the next. One declarative TOML definition produces many benchmarkable, auditable, reproducible agent configurations.

## Architecture

```
core/           Pure Rust libraries. No I/O, no side effects.
capability/     I/O boundary crates. File operations, schema embedding, protocols.
gates/          PyO3 gate modules for pipeline stage validation.
hooks/          Hook binaries for session security and quality enforcement.
cli/            CLI tools: saga, syn, cc_launch, and pipeline check utilities.
senders/        Datagram emission binaries for the monitoring dashboard.
writers/        Atomic file-write binaries for specific output formats.
watchers/       File-watching and traffic-diffing binaries.
rewriters/      Request rewriting binaries (compaction injection).
interceptors/   Traffic interception modules.
daemons/        Long-running services (bifrost_proxy).
```

## Building

All crates are built and deployed through `nornir_deploy`, which wraps Cargo and Maturin to handle the mixed binary/PyO3 workspace. Direct `cargo build --release` produces binaries that miss PyO3 module extraction and deployment steps.

```bash
nornir_deploy --build all        # Build everything
nornir_deploy --build gates      # Build pipeline gate modules only
nornir_deploy --build tools      # Build CLI tools only
nornir_deploy --build hooks      # Build hook binaries only
```

## Licence

Copyright (c) 2025–2026 John Oker-Blom

This program is free software: you can redistribute it and/or modify it under the terms of the GNU Affero General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.

See [LICENSE](LICENSE) for the full licence text.
