# Peekaboo — Design Spec

**Date:** 2026-10-09
**Owner:** Devx Labs
**Status:** Draft for review

## 1. Purpose

Peekaboo is a desktop companion for AI coding agents, shipped as a Devx product. A small ghost mascot ("Boo") hides in the Mac notch (or the top screen edge on Windows/Linux) and peeks out when an agent needs attention. From the overlay, the user can see what every agent session is doing, approve or deny tool permissions, answer agent questions, review live diffs, check plan usage, chat with models, and trigger integrations — without switching to the terminal.

Inspired by Coucou (MIT, Louis Raillé). Peekaboo is an independent implementation: it reuses no branded assets (name, character, sounds, art). Any code borrowed from Coucou keeps its MIT notice in `THIRD_PARTY_NOTICES.md`.

### Success criteria
- Installs on macOS, Windows, and Linux from a single codebase.
- With Peekaboo running, a Claude Code permission prompt can be approved from the overlay and the agent continues within 1s of the click.
- With Peekaboo **not** running (or crashed, or slow), every agent behaves exactly as it does without Peekaboo. An agent is never blocked.
- Adding a new agent requires only a new adapter module plus installer entry; no core changes.
- No telemetry, no account. Secrets live in the OS credential store.

## 2. Platform & stack

| Layer | Choice |
|---|---|
| App shell | Tauri 2 (Rust backend) |
| UI | TypeScript + React + Vite, Canvas for mascot |
| Hook binary | `peekaboo-hook`, small standalone Rust binary |
| IPC | Unix domain socket (macOS: `~/Library/Application Support/Peekaboo/peekaboo.sock`; Linux: `$XDG_RUNTIME_DIR/peekaboo.sock`), Windows named pipe `\\.\pipe\peekaboo` |
| Secrets | `keyring` crate (Keychain / Credential Manager / Secret Service) |
| Persistence | JSON files in the app data dir (settings, session history); SQLite only if history needs querying later |
| Tests | `cargo test` for Rust, Vitest for UI |

Repo layout (Cargo workspace + npm):
```
peekaboo/
  crates/
    protocol/     # shared message types (serde), versioned
    hook/         # peekaboo-hook binary
  src-tauri/      # Tauri app: core, adapters, installer, ipc server
  src/            # React overlay UI
  docs/
```

## 3. Architecture

```
agent CLI ──hook cmd──▶ peekaboo-hook
                            │ length-prefixed JSON over socket / pipe
                            ▼
                    Rust core (src-tauri)
                    ├─ ipc::server     accepts connections, routes messages
                    ├─ sessions        session store + state machine
                    ├─ approvals       pending requests, waits for UI decision
                    ├─ adapters/*      per-agent event normalization + response formatting
                    ├─ installer       per-agent hook install/uninstall with backup
                    ├─ diffs           per-session file change tracking
                    ├─ usage           plan usage readers (phase 4)
                    ├─ chat            model providers (phase 4)
                    ├─ integrations/*  (phase 5)
                    └─ secrets         keyring wrapper
                            │ Tauri events / commands
                            ▼
                    Overlay UI (React)
                    collapsed pill ⇄ expanded panel
```

### 3.1 Hook protocol (v1)

`peekaboo-hook` is invoked by the agent's hook system with the raw hook payload on stdin and `--agent <name> --event <name>` args. It:

1. Connects to the socket/pipe with a 200 ms connect timeout. On failure: exit 0 with no output (agent proceeds as if no hook).
2. Sends a `HookMessage`:
   ```json
   { "v": 1, "id": "<uuid>", "agent": "claude", "event": "PreToolUse",
     "cwd": "...", "pid": 1234, "payload": { ...raw agent JSON... },
     "expects_reply": true }
   ```
3. If `expects_reply` is false (pure notifications), it exits immediately after sending.
4. Otherwise it waits for a `HookReply` up to the configured approval timeout (default 10 min; on timeout exit 0 with no output → agent shows its own prompt):
   ```json
   { "v": 1, "id": "<same uuid>", "decision": "allow|deny|ask|answer",
     "always": false, "answer": null, "reason": null }
   ```
5. The adapter-specific stdout/exit code is produced **in the core** and returned in the reply as `{ "stdout": "...", "exit_code": 0 }`, so the hook binary stays agent-agnostic.

Framing: 4-byte big-endian length + UTF-8 JSON. Unknown `v` → core replies with passthrough (`stdout: ""`, `exit_code: 0`).

### 3.2 Adapters

```rust
trait AgentAdapter {
    fn id(&self) -> &'static str;                         // "claude"
    fn normalize(&self, msg: &HookMessage) -> Vec<CoreEvent>;
    fn needs_reply(&self, msg: &HookMessage) -> bool;
    fn render_reply(&self, msg: &HookMessage, d: &Decision) -> HookOutput; // stdout + exit code
    fn installer(&self) -> Box<dyn HookInstaller>;
}
```

`CoreEvent` is the agent-neutral vocabulary: `SessionStarted`, `SessionEnded`, `PromptSubmitted`, `ToolStarted{tool, input}`, `ToolFinished{tool, result}`, `PermissionRequested{tool, input}`, `QuestionAsked{questions}`, `Notification{text}`, `Idle`, `FileChanged{path, added, removed}`.

Adapters, by phase: **Phase 1:** Claude Code. **Phase 2:** Cursor, Codex, Gemini CLI, Copilot CLI. Later: OpenCode, Amp, others via a generic `--agent` passthrough adapter that maps only notifications.

### 3.3 Sessions

Session key = `(agent, session_id from payload or pid+cwd)`. States: `Starting → Working ⇄ WaitingApproval / WaitingAnswer → Idle → Ended`. Sessions with no event for 30 min are marked `Stale` and hidden. The store emits a `sessions:changed` Tauri event on every change.

### 3.4 Approvals & "Always"

A pending approval holds the open connection to `peekaboo-hook`. UI choices:
- **Allow / Deny** → reply once.
- **Always** → reply allow and persist a rule `{agent, tool, match}` in `settings.json`; future matching requests auto-allow without showing UI (still logged in history). Rules are editable in Settings.
- **Ask in terminal** → passthrough reply; the agent shows its native prompt.

If the user closes the agent while a request is pending, the socket closes and the request is dropped from the UI.

### 3.5 Questions (AskUserQuestion)

Claude Code's `AskUserQuestion` tool arrives as a `PreToolUse` for that tool. The UI renders its questions/options; the reply is `permissionDecision: "allow"` with `updatedInput` = the original input plus `answers: {<question text>: <label(s), comma-joined>}`, which Claude Code documents as the way hooks answer the tool. "Answer in terminal" is a passthrough.

### 3.6 Hook installer

For each agent: locate config (e.g. `~/.claude/settings.json`), write a timestamped backup to the app data dir, merge Peekaboo hook entries (idempotent, tagged so uninstall removes only ours), and verify the hook binary path. Settings shows install status per agent with Install / Uninstall / Restore backup.

### 3.7 Diffs (Phase 2)

On `ToolFinished` for file-editing tools, the adapter extracts path + edit content; otherwise the core runs `git diff --numstat` in the session cwd (if a git repo) after each tool. UI shows per-session file list with +/− and an expandable unified diff.

### 3.8 Mascot "Boo" (Phase 3)

A ghost drawn on Canvas at 60 fps with procedural animation (float, blink, peek). Mood follows aggregate state: sleeping (no sessions), floating (working), peeking out + bounce (needs approval/answer), happy spin (task done), worried (error). Original sounds (short synthesized cues) with per-event toggles and a global mute. Wardrobe/skins are a later nice-to-have.

### 3.9 Usage gauges (Phase 4)

Claude and Codex plan usage read from the local data each CLI already stores (e.g. Claude Code transcripts in `~/.claude/projects`), summed into the current 5-hour and weekly windows. No network calls with user credentials unless an official API exists.

### 3.10 Chat (Phase 4)

Provider trait with Anthropic, OpenAI, and OpenAI-compatible local endpoints (Ollama, LM Studio). Streaming responses. Default Anthropic model is the latest Claude model. Keys stored via `secrets`. Optional "attach session context" sends the selected session's recent events.

### 3.11 Integrations (Phase 5)

`Integration` trait: `id`, `auth` (token in keyring), `status()`, `actions()`. First set: GitHub (PR/CI status for the session's repo), Vercel (latest deployment status), Slack (post session summary to a channel). Each integration is optional and off by default.

## 4. Overlay UI

- **Collapsed:** pill at the notch/top-center showing Boo + count of active sessions; badge pulses when something needs attention.
- **Expanded (hover or click; global shortcut ⌥⌘P / Ctrl+Alt+P):** list of sessions (agent icon, project name, state, last action), pending approvals at the top with Allow / Deny / Always / Ask-in-terminal, question cards, diffs tab, chat tab, usage strip.
- **Settings window:** hooks per agent, Always-rules, timeouts, sounds, keys, integrations, launch at login.
- macOS: transparent, borderless, always-on-top window positioned over the notch (falls back to top-center on non-notch displays). Windows/Linux: same window at top-center. Click-through when collapsed except on the pill.
- Keyboard: `A` allow, `D` deny, `Esc` collapse, `1–9` pick a question option.

## 5. Error handling

- Hook binary: every failure path (no socket, refused, timeout, malformed reply) → exit 0, empty stdout. Never prints to stderr in a way the agent would treat as an error.
- Core: malformed messages are logged and answered with passthrough. Adapter panics are caught per-message.
- Installer: never writes config without a backup; on JSON parse failure of an existing config, refuse and show the error instead of overwriting.
- Logs: rotating log file in the app data dir; "Copy diagnostics" button in Settings.

## 6. Privacy & security

No telemetry, no account. Socket file permissions `0600`; named pipe restricted to current user. Approval replies only accepted from the UI process, never from the socket. Secrets only in the OS credential store.

## 7. Testing

- `protocol`: round-trip serialization, framing, version handling.
- `hook`: integration tests against an in-test socket server covering allow, deny, timeout, server-absent, and malformed reply.
- Adapters: fixture-based tests from real captured hook payloads per agent.
- Installer: tests on temp dirs (fresh config, existing config, re-install idempotency, uninstall, malformed config).
- Sessions/approvals: state machine unit tests.
- UI: Vitest + Testing Library for session list and approval card.

## 8. Phases

| Phase | Scope | Ships |
|---|---|---|
| 1 Core | Workspace, protocol, hook binary, IPC server, sessions, approvals, Always rules, questions, Claude Code adapter + installer, overlay pill/panel, settings | Usable Claude Code approver on all 3 OSes |
| 2 Diffs + agents | Diffs; Cursor, Codex, Gemini CLI, Copilot CLI adapters | Multi-agent tracking |
| 3 Mascot | Boo animation, moods, sounds | Brand experience |
| 4 Usage + chat | Usage gauges, chat providers, secrets | Power-user features |
| 5 Integrations | GitHub, Vercel, Slack | Workflow hooks |

Each phase gets its own implementation plan. Packaging (signed .dmg, .msi, AppImage/.deb/.rpm) and auto-update are set up at the end of Phase 1 so every later phase ships.

## 9. Prerequisites & open items

- Rust toolchain is not installed on the dev machine; Phase 1 starts with `rustup` install.
- Code signing: Apple Developer ID + notarization and a Windows code-signing cert are needed before public release (Devx to procure).
- Trademark/domain check for "Peekaboo" before public launch.
- GitHub remote: to be created under the Devx org once confirmed.
