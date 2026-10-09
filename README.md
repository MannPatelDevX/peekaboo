# Peekaboo 👻

A desktop companion for AI coding agents, by Devx Labs. **Boo** hides in your Mac's notch (or the top edge of the screen on Windows/Linux) and peeks out when an agent needs you — approve tool permissions, answer agent questions, and see what every session is doing without switching to the terminal.

Phase 1 supports **Claude Code**. More agents, live diffs, the animated mascot, usage gauges, chat and integrations are on the [roadmap](#roadmap).

## Using it

1. Launch Peekaboo. A small pill appears at the top-center of your screen.
2. Hover it → **Settings** → **Agents** → **Install** next to Claude Code.
   Peekaboo copies its hook program to its app-data folder (so moving or updating the app never breaks Claude Code), backs up `~/.claude/settings.json` first (to its app-data `backups/` folder), adds its hooks, and leaves your own hooks untouched. **Uninstall** removes only Peekaboo's entries.
3. Use Claude Code as usual. When it needs permission or asks you a question, Boo peeks out:
   - **Allow / Deny** — answer once (keys `a` / `d` when the panel has focus)
   - **Always** — allow this again without asking, scoped to this project (for Bash: only this exact command). Manage the list in Settings
   - **Terminal** — fall back to Claude Code's own prompt
4. Toggle the panel anytime with **⌥⌘P** (Ctrl+Alt+P on Windows/Linux). `Esc` collapses it.

### The never-block guarantee

Claude Code runs a tiny `peekaboo-hook` binary that forwards the event to Peekaboo over a private local socket (`0600`; a named pipe on Windows). If Peekaboo isn't running, crashed, or nobody answers before the timeout (10 min for permissions, 60 s for questions so Claude's own prompt isn't held up), the hook exits immediately with no output and Claude Code behaves exactly as if Peekaboo weren't installed.

No telemetry, no account, no network calls.

## Development

Requirements: Rust (stable), Node 22+. Linux also needs `libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf`.

```bash
npm install
npm run tauri dev       # run the app (builds the hook sidecar first)
cargo test              # protocol, hook, core (+ end-to-end hook ↔ server tests)
npm test                # UI components
npm run tauri build     # release bundle
```

Layout:

| Path | What |
|---|---|
| `crates/protocol` | Wire protocol shared by hook and app (length-prefixed JSON, v1) |
| `crates/hook` | `peekaboo-hook` binary invoked by agents |
| `crates/core` | Sessions, approvals, Always rules, agent adapters, hook installers, IPC server (no Tauri dependency) |
| `src-tauri` | Thin Tauri shell: overlay window, commands, global shortcut |
| `src` | React overlay UI |

Adding an agent = an adapter in `crates/core/src/adapters/` plus an installer in `crates/core/src/installer/`. See `docs/superpowers/specs/2026-10-09-peekaboo-design.md`.

## Roadmap

1. **Core** (this release) — Claude Code approvals, questions, session tracking
2. **Diffs + agents** — live file diffs; Cursor, Codex, Gemini CLI, Copilot CLI
3. **Boo** — animated mascot, moods, sounds
4. **Usage + chat** — plan usage gauges; chat with Claude, OpenAI, local models
5. **Integrations** — GitHub, Vercel, Slack

Inspired by [Coucou](https://github.com/Louis-CFM/coucou) (MIT). Peekaboo is an independent implementation and uses none of Coucou's names, characters, sounds or art.
