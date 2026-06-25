# announce — Operational Notes

> Salvaged from Claude Code auto-memory (2026-06-20). `announce` isn't otherwise documented in
> nornir's docs, so this fills the gap — but it's salvaged, not freshly verified, so check
> specifics (paths, URLs, voice IDs, clamp values) against the code before relying on them.

`announce` (`cli/announce`, core: `announce_core`; deployed to `~/.ai/tools/bin/announce` via
`nornir_deploy`) is nornir's TTS tool.

## Backends
- **kokoro** (default) — local TTS at `http://127.0.0.1:8880/v1/audio/speech` (OpenAI-compatible),
  zero credits, always caches (local = free). Voices include `af_heart`, `bm_lewis`, `am_adam`, etc.
- **elevenlabs** — cloud; needs `ELEVENLABS_API_KEY`; uses hit-count cache gating.

## Config split
- Voice config: `~/.ai/control/voice/announce.toml`
- Secrets: `~/.ai/voice/.env` (kept OUT of `control/`)
- Audio cache: `~/.ai/audio/{voice_id}/{lang_code}/{slug}_{hash4}.mp3`

## Control directory `~/.ai/control/`
- `voice/` — `announce.toml`, `hits.toml`, `lookups/`, global `SILENT.lock`/`QUIET.lock`
- `workspaces/` — `registry.db` plus per-workspace dirs holding `VOICE.lock`/`SILENT.lock`/`QUIET.lock`

## Workspace registry (`workspace_registry` crate)
SQLite + WAL at `~/.ai/control/workspaces/registry.db`. Functions: `register_workspace(name,path)`,
`register_session(session_id,workspace)`, `resolve_workspace_from_path` (longest-prefix),
`workspace_from_session`, `workspace_control_dir(name)`. Populated by `hook_start_session_orient`
on every SessionStart.

## Lock resolution order
Global first (fast common-case exit), then per-workspace: `voice/SILENT.lock` (or QUIET) →
`workspaces/{ws}/SILENT.lock` (or QUIET). `SILENT` = all TTS off; `QUIET` = CC readout only off.

## Config priority
CLI args > `VOICE.lock` > `--profile`/`--severity` > default. Voice = workspace identity
(`VOICE.lock`), NOT severity. Severity (`cc`/`trace`/`info`/`notify`/`warn`/`alert`/`critical`)
is metadata only.

## macOS CoreAudio + fork
Never `fork()` with rodio; self-exec via `Command::new(exe).arg("--play-file").spawn()`.

## ElevenLabs specifics
Speed clamped 0.7–1.2 (f64 in JSON); auth header `xi-api-key`; PCM stream
`…/stream?output_format=pcm_24000`, MP3 cache `…?output_format=mp3_22050_32`.
