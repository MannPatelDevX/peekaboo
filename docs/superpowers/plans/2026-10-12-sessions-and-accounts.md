# Sessions & Accounts Implementation Plan

> Executed inline (superpowers:executing-plans). Spec: `docs/superpowers/specs/2026-10-09-peekaboo-design.md` §10.

1. **`crates/core/src/sessions_index/`** — `parse_transcript(path) -> Option<TranscriptSession>`, `SessionIndex::scan(root, now) -> Vec<TranscriptSession>` with (mtime,size) cache, 30-day / 200 cap. Tests: fixture JSONL (titles precedence, meta prompts skipped, sidechain usage ignored, 1M window, malformed lines tolerated, cache reuse).
2. **`crates/core/src/claudex/parse.rs`** — port `gauge`, `parse_usage`, `parse_list`, `parse_current`, `parse_pool_status`, `parse_autoswitch` (from doctor). Tests on claudex-dash fixtures + CLI-error and unknown-shape → None.
3. **`crates/core/src/claudex/mod.rs`** — `find_claudex()`, `Claudex::snapshot(force)` (cached reads, timeouts), `Claudex::act(Action)` with `Action::{Switch(name), Pool(on), Autoswitch(on)}`; `argv_for(action, known_accounts) -> Result<Vec<String>>` rejects unknown names. Tests: argv allowlist, unknown/flag-like names rejected, fake `claudex` script on PATH for timeout + cache.
4. **`crates/core/src/terminal.rs`** — `resume_command(cwd, id)` shell-quoted, `launch_plan(os, cwd, id) -> Vec<(program,args)>`; tests for quoting and per-OS plans.
5. **Tauri commands** `list_transcripts`, `resume_session`, `claudex_snapshot`, `claudex_act` (blocking work off the main thread).
6. **UI** — tabs Agents | Sessions | Accounts | Settings; `SessionsPanel`, `AccountsPanel` (confirm dialog), pill weekly %. Vitest per component.
7. **THIRD_PARTY_NOTICES.md**, README update; full suite; final review.
