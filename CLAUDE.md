# Nornir — Session Instructions

## Required Reading

Before writing ANY code, read `MANDATORY_READ_BEFORE_CODING.md`, then `NORNIR_NAMING.md` and `NORNIR_ORGANIZATION.md`.

## Deploying

Every binary category has its own deploy script. Do NOT use bare `cargo build` — deploy scripts handle symlinks, verification, and (for gates) Python module extraction.

```bash
cd /Users/johnny/.ai/smidja/nornir

./deploy_gates.py         # CLI check tools + PyO3 gate modules
./deploy_hooks.py         # Hook binaries
./deploy_writers.py       # Writer binaries
./deploy_rewriters.py     # Rewriter binaries
./deploy_senders.py       # Sender binaries
./deploy_converters.py    # Converter binaries
./deploy_watchers.py      # Watcher binaries
./deploy_dispatchers.py   # Dispatcher binaries
./deploy_tools.py         # Specialist tools (saga, syn)
```

After schema changes (draupnir regeneration): `./deploy_gates.py`

## Key Rules

- Schemas are embedded at compile time via `include_str!()`. Changed `.schema.json` files have no effect until the deploy script runs.
- All helper functions return `Result`. Only `main()` calls `process::exit()`.
- Do not hand-write validation logic in Python. Import the gate module and call `validate()`.
- Do not bypass the gate API. Every gate returns `{"ok": bool, "data": ..., "error": ...}`. Check `ok` before using `data`.
- Run `cargo test` before committing. 564 tests across 20 crates must all pass.
