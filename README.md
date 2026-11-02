# Mika

A menu bar companion that shows what each AI coding agent is doing right now, what it's costing your machine, and when it needs you.

Built with Tauri 2 (Rust) + React + TypeScript. macOS first.

## v0.1 scope

**Agents:** Claude Code and Codex.

1. **Agent status character.** An animated menu bar character, plus an optional floating window. States: idle, thinking, using a tool, waiting for you, done, error, near limit.
   - Claude Code: a local HTTP listener on `127.0.0.1` receives hook events (`UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Notification`, `Stop`). The hooks are installed with one click, with a backup and an undo.
   - Codex: state is inferred from session-file activity.
   - Notifications for "waiting for you" and "done".
2. **Per-agent resources.** CPU %, memory and uptime for each agent and its child processes, sampled every 2 seconds, with a 10-minute sparkline.
3. **Background processes.** The process tree each session started, with listening ports, orphan detection (processes still running after their session ended), and kill with confirmation.
4. **Basic tokens.** Today's tokens, estimated cost and tok/s for each agent, read from local logs. Claude 5-hour window usage with a burn-rate forecast.

**Not in v0.1:** sync, other tools, history dashboards, export, Windows/Linux, budgets, approving permissions from the widget, mobile push.

## Architecture

```
Claude Code hooks ──HTTP──▶ ┌──────────────── Rust core ─────────────────┐
Session log watcher ───────▶│ event bus → state machine (per session)    │──▶ React UI
sysinfo sampler (2s) ──────▶│ process tree + port scan (lsof/libproc)    │
                            │ SQLite (sessions, samples, token totals)   │
                            └────────────────────────────────────────────┘
```

## Milestones

1. **Process sampler.** Menu bar icon, process sampler, agent process tree. Done when CPU/memory for a live `claude` session updates within 2 seconds.
2. **Agent status.** Hook installer, event listener, state machine, and a character with all seven states. Done when a state changes within 300 ms of a hook event.
3. **Processes and tokens.** Background processes, ports and orphans. Token parsing for Claude Code and Codex, with the 5-hour forecast.
4. **Polish.** Onboarding, a signed and notarized `.dmg`, and a Homebrew cask.

## Development

Prerequisites: [Rust](https://www.rust-lang.org/tools/install), Node 20+, Xcode Command Line Tools.

```bash
npm install
npm run tauri dev
```
