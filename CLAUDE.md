# Nornir — Session Instructions

## After Schema Changes

When schemas are regenerated (by draupnir), redeploy nornir:

```bash
cd /Users/johnny/.ai/spaces/bragi/tools/nornir && ./deploy_gates.py
```

This builds CLI binaries with cargo, builds gate modules with maturin, extracts `.so` files from wheels, creates symlinks in `~/.ai/tools/bin/`, and verifies everything works.

Do NOT use bare `cargo build` — it skips gate module deployment.

## After Writer Changes

```bash
cd /Users/johnny/.ai/spaces/bragi/tools/nornir && ./deploy_writers.py
```

## Key Rules

- Schemas are embedded at compile time via `include_str!()`. Changed `.schema.json` files have no effect until `deploy_gates.py` runs.
- Do not hand-write validation logic in Python. Import the gate module and call `validate()`.
- Do not bypass the gate API. Every gate returns `{"ok": bool, "data": ..., "error": ...}`. Check `ok` before using `data`.
