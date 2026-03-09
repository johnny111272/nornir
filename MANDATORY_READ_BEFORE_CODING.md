# MANDATORY: Read Before Coding in Nornir

**STOP. Do not write any code until you have read this file completely.**

This workspace has been destroyed and rebuilt multiple times because LLM sessions added "quick" code that violated organizational conventions. Each violation is small. The cumulative effect is irreversible. The project gets abandoned and rebuilt from scratch.

**This is not hypothetical. It has happened over a dozen times across this user's projects.**

## The Rule

**No compliance, no coding.**

If you cannot or will not follow the conventions in this workspace, you must say so immediately and stop. Do not attempt to "just get it working" and clean up later. There is no later. The next LLM session will pattern-match off your mess and amplify it.

## Required Reading

Before writing ANY code in nornir, you MUST read:

1. **`NORNIR_NAMING.md`** — Naming conventions for binaries, crates, directories, schemas
2. **`NORNIR_ORGANIZATION.md`** — Directory structure, dependency tiers, deploy scripts, templates

These are in the nornir root directory (`~/.ai/smidja/nornir/`).

## Compliance Declaration

Before writing code, you MUST state to the user:

```
I have read NORNIR_NAMING.md and NORNIR_ORGANIZATION.md.
I understand and will comply with:
- Verb-prefix naming for all binaries
- Directory name = package name = binary name
- Three-tier dependency model (core → capability → binary)
- Workspace dependencies for all shared crates
- Deploy script integration for all new binaries

If I encounter a situation where compliance is unclear, I will ask
before proceeding.
```

**This is not optional.** If you cannot make this declaration, you cannot write code in this workspace.

## The Deploy Script Rule

Every binary must be added to the appropriate deploy script. Building and symlinking manually in the terminal is forbidden.

**Why:** Manual `cargo build && ln -s` works in the moment but:
- The next session won't know the binary exists
- The next rebuild will miss it
- The deploy script's verification step won't cover it
- It creates a silent gap between "what's deployed" and "what should be deployed"

**The deploy scripts are:**
- `deploy_gates.py` — CLI check tools + PyO3 gate modules
- `deploy_hooks.py` — Hook binaries
- `deploy_writers.py` — Writer binaries
- `deploy_rewriters.py` — Rewriter binaries
- `deploy_senders.py` — Sender binaries
- `deploy_converters.py` — Converter binaries
- `deploy_watchers.py` — Watcher binaries
- `deploy_dispatchers.py` — Dispatcher binaries
- `deploy_tools.py` — Specialist tools (saga, syn)

Every binary category has a deploy script. If your crate doesn't fit an existing one, create a new one following the same pattern.

## Pre-Coding Checklist

Before writing any new crate or modifying an existing one:

- [ ] Read `NORNIR_NAMING.md` and `NORNIR_ORGANIZATION.md`
- [ ] Stated compliance declaration to user
- [ ] Identified which category directory the crate belongs in
- [ ] Verified the name follows verb-prefix convention
- [ ] Verified directory name matches package name matches binary name
- [ ] Identified which deploy script to add the crate to
- [ ] Checked that dependencies use workspace versions where available
- [ ] Confirmed internal dependencies follow the tier model

## What Goes Wrong Without This

1. A session adds `my_tool` to `cli/` — no verb prefix, wrong category
2. Next session sees `my_tool` in `cli/` and adds `another_tool` following the same pattern
3. Third session tries to understand the naming and fails — adds `do_thing` with a hyphenated variant
4. Fourth session can't find anything, creates a new top-level directory
5. Fifth session gives up trying to understand the organization
6. **Project abandoned. Rebuild from scratch.**

This is not a slippery slope argument. This is documented history across 12+ projects.

## When Conventions Are Unclear

If you encounter a situation where the naming or organization conventions don't clearly apply:

1. **Ask the user.** Do not guess.
2. **Default to the most restrictive interpretation.** If it might be wrong, it is wrong.
3. **Look at existing examples.** The patterns in the codebase are intentional.
4. **Never create a new category directory** without explicit user approval.

## Existing Violations

If you discover an existing naming or organization violation during your work:

1. **Document it.** Note what's wrong and what it should be.
2. **Report it to the user.** Do not silently fix it — the fix may have downstream effects.
3. **Do not use the violation as precedent.** A wrong name in the codebase does not make it right.
4. **Do not propagate the violation.** If `json_to_toml` exists without a `convert_` prefix, your new converter still gets the prefix.

## The Fundamental Truth

**The current state of the codebase is the strongest weight for an LLM.**

If nornir is slightly wrong when you start, it will be significantly more wrong when you finish. Every line you write is influenced by what you've read. If what you've read is non-compliant, what you write will be non-compliant.

This is why we audit before coding. This is why we fix violations before adding new code. This is why these documents exist.

**Read them. Follow them. Or don't code here.**
