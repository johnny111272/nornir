# Checkpoint: Security Tools Output Analysis

**STALE:** This checkpoint predates the processing restructure and enforcement output tool replacement (2026-02-19). `security_tools` has been replaced by `enforcement_output_tool` in the enforcement section. Processing fields moved to `task.processing`. See `DEFINITION_FORMAT.md` for current schema.

**Session date:** 2026-02-19
**Status:** Mid-analysis, ready to continue

## What's Done

### Nornir Build: COMPLETE
- All 6 phases implemented, all tests pass (46 tests, 0 failures)
- `cargo check/clippy/test --workspace` all clean
- 4 CLI tools built in release mode, symlinked to `~/.ai/tools/bin/`
- 12 PyO3 gate modules built via maturin, deployed to `~/.ai/tools/lib/`
- Deployment script: `tools/build_nornir.py` (run successfully, all 16 components verified)
- 3 deferred gates + 2 deferred CLI stubs await permissions-resolved and universal-format schemas
- Task #7 marked completed

### Security Analysis: IN PROGRESS

We're analyzing the `security_tools` subsection to determine if its field set can describe ALL meaningful bounded write operations for current and future agents.

## Key Documents Read This Session

1. **`standards/toolcompose/TRANSFORMER_LOGIC.md`** — READ IN FULL. The 7-phase permission resolution algorithm. This is the canonical source for how permissions are computed. Key concepts:
   - Phase 1: Collect FS grants (implicit from structure + explicit from security_paths)
   - Phase 2: Path validation security gate (blocklist + traversal — HARD STOP)
   - Phase 3: Derive implicit tool needs (structural requirements, bypass intersection)
   - Phase 4: Derive possible capabilities from explicit path grants
   - Phase 5: Intersect: `explicit_granted = requested ∩ possible - prohibited`
   - Phase 6: Union: `final = implicit ∪ explicit_granted - prohibited`
   - Phase 7: Bind capabilities to paths, produce outputs for engineering controls
   - The `write-tool` implicit capability is separate from write/append/edit capabilities
   - Engineering controls are platform-agnostic; format transformers implement enforcement

2. **`documentation/agent_skill_generator/WRITE_TOOL_RESOLUTION.md`** — READ IN FULL. Current deployed system's three write patterns:
   - Pattern 1: Full-path tool (agent calls tool by absolute path with schema + JSON + output path)
   - Pattern 2: Wrapper tool (agent calls $PATH wrapper that hardcodes schema + output path)
   - Pattern 3: Batch stdin (agent pipes records via heredoc to batch writer)

3. **`definitions/agents/embedding-normalize-combined-opus.toml`** — The only TOML definition with full `security_tools` section declared
4. **`definitions/agents/agent-rebuilder.toml`** — Has no `security_tools` section

5. **All 10 agents in `.claude/agents/`** — Analyzed output patterns:
   - ALL use Bash-invoked custom scripts for output (never Write/Edit tools directly)
   - ALL validate output against schema before writing (constraint_schema = true for all)
   - 8 write JSONL, 2 write JSON, 0 write markdown
   - Three write tools: append_validated_jsonl.py, append_validated_jsonl_batch.py, write_validated_json.py
   - Two wrappers: append_qc_line, write_qa_record

6. **7 agents in `~/.claude/agents/`** — Different pattern: all use Write tool directly, none validate against schema, write markdown or JSONL whole-file

## The Current Field Set Under Analysis

```
security_tools (OPTIONAL)
  security_tools_output_capability: enum [write, edit, append]
  security_tools_output_constraint_mode: enum [record, batch, file]
  security_tools_output_constraint_batch_size: integer >=1 (ONLY WHEN mode=batch)
  security_tools_output_constraint_schema: boolean
  security_tools_output_constraint_format: enum [jsonl, json, markdown, text]
  security_tools_output_constraint_location: boolean
```

## Analysis So Far

### Field-by-field interpretation (in context of full security section):

- **capability** = which bound output tool shape the agent receives (not "can it write" — that's security_capabilities)
- **constraint_mode** = calling pattern: record (N calls), batch (ceil(N/batch_size) calls), file (1 call)
- **batch_size** = max records per tool invocation in batch mode
- **constraint_schema** = whether the bound tool self-validates. If true, format transformer doesn't add schema enforcement. If false, format transformer must add it.
- **constraint_format** = what the tool accepts/produces
- **constraint_location** = whether the tool hardcodes its write destination. If true, agent can't choose path. If false, agent operates within security_paths_allowed_write directories.

### Tool matrix (meaningful bounded write operations):

**4 tool shapes cover all cases:**
1. `append_validated_record` — append + record + jsonl/text
2. `append_validated_batch` — append + batch + jsonl/text
3. `write_validated_file` — write + file + any format
4. `edit_validated_file` — edit + file + json/markdown/text

**Invalid combinations the schema currently allows:**
- append + json (physically impossible)
- append + markdown (structurally dangerous)
- edit + record/batch (unclear semantics)
- append + file (degenerate — same as write)

**Open question: write_per_record**
QA agent creates `quarantine/{id}.json` — one new file per input record. This is write+record, but user's mapping says mode=record means append_record. Options:
1. write + record overloads mode (capability distinguishes)
2. Handle via dispatcher invoking agent once per record
3. New mode value

## Where We Stopped

The user asked three questions which I answered from the documentation:
1. How security_paths vs auto-derived paths differ (explicit=possibilities, implicit=structural requirements)
2. Regular tool grants vs bound output tool (capability set vs constrained write mechanism)
3. The intersection model (7-phase resolution, `final = implicit ∪ (requested ∩ possible) - prohibited`)

**Next step:** The user was about to "fill me in on the rest" — meaning there's additional context beyond what's documented that they want to share. We were building toward determining whether the current 6-field security_tools schema is sufficient for all meaningful future agent output patterns.

## How the security layers compose (for quick reference)

```
PATH_BLOCKLIST        hard ceiling — /, ~/, .claude/, .env, ..
security_paths        fence — workspace root + allowed read/write directories
security_capabilities verbs — requested ∩ possible - prohibited
security_io           routes — specific input/output file or directory
security_tools        behavior — how the bound output tool works
security_schema       shapes — which schemas validate input/output
context               material — what to read before working (auto-derives read permissions)
```
