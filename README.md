# Nornir

Validation-as-code Rust monorepo. Produces compiled binaries, PyO3 gate modules, and libraries for schema-driven LLM agent infrastructure.

## What This Is

Nornir is a Rust workspace containing 97 crates organised in a strict three-tier architecture:

- **Core** (pure, no I/O) — type definitions, validation engines, formatting, diffing, error types
- **Capability** (I/O boundary) — file operations, schema embedding, hook protocols, datagram emission
- **Binaries** — CLI tools, pipeline gates, hooks, watchers, senders, a reverse proxy daemon

Everything that needs to be compiled, fast, or trusted lives here. Python projects in the ecosystem (Draupnir, Regin, Galdr, Bifrost) call into Nornir's compiled artifacts — either as PyO3 modules imported directly into Python, or as standalone binaries.

Nornir is currently a monorepo because these tools were built together and share core infrastructure. As the system matures, subsystems with independent lifecycles will be split into their own repositories.

## The Code Quality Triad: Gleipnir, Saga, Syn

The guardrail system is built on a philosophy: **you cannot make LLMs generate good code through instructions alone.** Instructions fade as context fills. Training data gravity pulls output toward the statistical centre of mass — defensive, class-heavy, mediocre patterns. The only thing that durably shapes LLM coding behaviour is experienced consequences: an environment where quality-compliant patterns pass freely and violations block progress.

This requires strict separation of concerns. Detection, recording, and enforcement are three different responsibilities. Combining them creates systems that are hard to extend, impossible to audit, and fragile when any component changes. The triad keeps them independent, connected only by the `.qa` sidecar file format.

### Gleipnir — Detection

Tree-sitter-based AST analysis engine. Pure computation — receives source bytes, returns typed Violation structs. No I/O, no disk access, no policy decisions. Supports Python, Rust, TypeScript, and Svelte.

Gleipnir is not a linter. Every check produces educational messages with three fields: **signal** (what was detected), **direction** (how to address it), and **canary** (how to detect if the LLM gamed the fix rather than solving the problem). Direction is deliberately vague — it tells the LLM which way to go without giving it enough information to satisfy the check through superficial manipulation. The canary field is unique to this system: it encodes what a fake fix looks like, so downstream consumers can detect when a metric was silenced rather than a problem solved.

Violations are not style complaints. They are structural indicators. Function length signals accumulated responsibilities that should be decomposed. Short variable names signal that the author did not think about the reader. Print statements in library code signal misunderstanding of orchestration boundaries. Each check exists because a specific category of LLM-generated code defect was observed repeatedly across independent projects.

### Saga — Truth Recording

Runs analysis tools (Gleipnir for AST checks, Ruff for Python linting, Basedpyright for type checking) on source files and writes unfiltered `.qa` sidecar reports adjacent to each source file. Records every issue found — no filtering, no policy. Raw truth only. Saga is the only component that generates `.qa` files.

During normal operation, post-execution hooks run Saga automatically on every file write, keeping sidecars current without manual intervention. The quality data is always fresh as a side effect of working — not as a separate step someone has to remember.

### Syn — Policy Enforcement

Reads `.qa` sidecars and applies configurable three-tier filtering and enforcement. Report mode provides an informational view with ad-hoc filter overrides for exploration. Gate mode provides deterministic per-file enforcement locked to configuration only — no CLI overrides — ensuring reproducible accept/reject decisions in automated pipelines.

The three tiers: a warn filter selects what is visible, CLI overrides further narrow scope (report mode only), and a deny filter identifies what blocks progress. An issue invisible to the warn filter is never evaluated for denial. This means enforcement can be tightened progressively — permitting messiness during exploration and systematically tightening as code matures.

Multiple consumers (Syn, Svalinn dashboard, hooks, future tools) read the same `.qa` truth data independently at zero analysis cost. Adding a new consumer requires no changes to the detection or recording layers.

## Enforcement Today: The Hook System

Nornir includes a suite of compiled Rust hook binaries that enforce security and quality in real time during LLM coding sessions. These are production tools, running daily, with proper test coverage.

### Pre-Execution Security Gating

Every tool invocation passes through a layered security check before execution:

- **Floor rules** — permanently block access to credentials (`.ssh/`, `.aws/`, `.gnupg/`, `.kube/`, `.docker/`, `.netrc`). Cannot be overridden by any configuration. The LLM cannot read your SSH keys regardless of what it is told.
- **Probing detection** — flags attempts to inspect the security infrastructure itself (hook scripts, guardrail configurations, Claude settings). Configurable severity: warn, ask, or block.
- **Gaming detection** — flags attempts to circumvent constraints. Configurable severity.
- **Allow-path exemptions** — for sessions that need to work *on* the security system itself, specific paths can be exempted. Manual only, never auto-selected.

Decisions are allow, warn, ask, or deny. Each carries structured context explaining *what* was detected and *why* it was flagged — the LLM receives a reason, not just a rejection.

### Post-Execution Quality Assessment

Every file write triggers the full quality pipeline: Gleipnir analysis through Saga recording through Syn policy evaluation. If violations are found, the assessment is injected as context into the LLM's next turn — the LLM receives immediate feedback on what it just wrote. If the code is clean, nothing is injected. Quality feedback is automatic, real-time, and zero-effort.

### Context Re-Injection

When the LLM's context compacts (a routine event where the provider rewrites the context window), the assembled behavioural configuration — persona, cognitive frameworks, safety guardrails — is re-injected from the stored session record. The LLM's identity survives context resets because a hook rebuilds it every time.

## The Road to Bifrost

The hook system works, but it depends on vendor-specific injection points. Hooks fire when the vendor's framework decides they should fire. If the vendor changes how hooks work — or silently drops them, or summarises their input — the enforcement degrades without warning.

### Bifrost Proxy (Current State)

Bifrost is an async Rust reverse proxy (hyper/tokio) that sits in the traffic path between the LLM client and the API. It currently handles:

- **Request interception** — every request is captured, validated against an embedded wire-format schema, classified by type, and routed
- **Compaction rewriting** — compaction requests are intercepted and rewritten with injected instructions before they reach the provider, ensuring context resets do not silently destroy alignment
- **Response stream interception** — SSE event streams are parsed in real time; thinking blocks are identified, stripped from the stream, and side-logged with renumbered block indices to maintain stream integrity

This is operational and handling live traffic, but it is an MVP focused on monitoring and compaction control.

### Projected Direction

The enforcement logic currently running in hooks — security gating, quality assessment, context re-injection — will migrate into Bifrost's traffic layer. The proxy sees every request and response regardless of what the vendor changes upstream. It cannot be silently dropped, summarised, or circumvented.

When this migration is complete, all enforcement operates at the one point in the stack that the operator fully controls. The hook system's proven logic — floor rules, probing detection, quality pipeline triggering, context re-injection — moves from vendor-dependent injection points to a vendor-independent position. The same Rust code, the same decision logic, the same test coverage — in a location that cannot be pulled out from under it.

## Other Key Tools

### cc_launch — Session Initialisation TUI

A ratatui terminal interface that assembles composable system prompts from a library of atomic XML fragments. The operator selects a workspace, persona, cognitive frameworks, system descriptors, and expertise modules through a TUI. The launcher assembles them in a strict priority order (identity → context → mode), estimates token cost, and launches the session.

Assembled prompts are stored per-session by UUID for audit and re-injection at compaction. Workspace profiles define smart defaults that cascade on selection. Personas invoke mythological archetypes that activate complex behavioural frameworks from the LLM's training data — configuring how it thinks rather than specifying what it should do.

### Pipeline Check Utilities

Eight CLI tools that validate agent definition files at specific pipeline stages, enabling quick verification without running the full pipeline.

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
