# Mika

**Your AI coding agents, at a glance.**

Mika is a macOS menu bar app that watches Claude Code and Codex for you. It shows what every agent is doing, how many tokens they use, how close you are to your plan limits, and how hard your Mac is working. When an agent needs you, Mika pops up on your desktop with a ringtone, lets you answer the permission request right there, and takes you to the session.

It is built with Tauri 2: a small Rust core that gathers everything on your Mac, and a React + TypeScript window that draws it.

**Contents:** [Features](#features) · [Getting started](#getting-started) · [Architecture](#architecture) · [Application flow](#application-flow) · [The interface](#the-interface) · [Reference](#reference) · [Privacy and security](#privacy-and-security) · [Development](#development) · [Good to know](#good-to-know)

---

## Features

| Area | What you get |
| --- | --- |
| **Live status** | Each session's state (thinking, working, needs you, your turn, done, idle), shown by Mika the ghost, in the header and in the menu bar title. |
| **Alerts** | An always-on-top popup with a ringtone when an agent is blocked on you, on the screen you are using. Optional alerts when an agent finishes. Quiet hours. |
| **Answer from Mika** | Approve or deny a permission request from the popup: Allow, Always allow (Claude Code), Deny, or hand it back to the terminal. |
| **Jump to the session** | One click brings the right Terminal or iTerm2 tab to the front, or the app hosting the agent. |
| **Token usage** | A live counter for today, this month or all time, with estimated cost, tokens per second and a per-model breakdown. |
| **Activity** | A year heatmap and an 88-day trend line, with a per-day readout on hover. |
| **Plan limits** | How much of your Claude and Codex limits is left, and when each one resets. |
| **Sessions** | Title, model, effort, subagents, calls, cache hit rate, tokens, cost, context fill and prompt-cache expiry for each session. |
| **Mac health** | CPU, memory and temperature for the machine, plus CPU and memory for every agent and each process it started. |
| **Settings** | Sounds, popup behaviour, default range, cost estimates, motion level, menu bar title style, the Liquid Glass skin, which agents to track, update speed, hooks, and launch at login. |

### Works with

| | Live status | Answer from Mika | Tokens and cost | Plan limits | Resources |
| --- | --- | --- | --- | --- | --- |
| **Claude Code** | thinking, tools, needs you, your turn, done | Allow, Always allow, Deny | yes | yes | yes |
| **Codex** (CLI and ChatGPT app) | working, needs you, done | Allow, Deny | yes | yes | yes |

macOS only, for now.

---

## Getting started

1. Install and open Mika. It appears in your menu bar.
2. **For live status and alerts**, open Settings → Integrations and click **Install** for Claude Code and Codex. New sessions then report to Mika. Codex asks you once to trust the new hooks; approve them in Codex. To do it by hand, merge [`hooks/claude-code-settings.json`](hooks/claude-code-settings.json) into `~/.claude/settings.json` and [`hooks/codex-hooks.json`](hooks/codex-hooks.json) into `~/.codex/hooks.json`.
3. The first time Mika shows your Claude limits, macOS asks to allow access to Claude Code's sign-in. Allow it to see them.

Token usage, sessions and Mac health work without hooks, because they come from the agents' logs and the process list. Hooks add live status, alerts and answering from Mika.

---

## Architecture

Mika gathers from four sources on your Mac, processes them in a Rust core, and shows the result in its window, its menu bar title and its alert popup.

![How Mika sees your agents: agent hooks, session logs, macOS and Anthropic's usage API feed four Rust modules, which update the Mika interface](docs/architecture/overview.svg)

| Source | Rust module | How it is read | Produces |
| --- | --- | --- | --- |
| Agent hooks (Claude Code, Codex) | `status.rs`, `permission.rs` | Each hook runs `curl` to post its JSON payload to `127.0.0.1:47321` | Session states, permission requests |
| Session logs in `~/.claude/projects` and `~/.codex/sessions` | `tokens.rs` | File-change events, then an incremental parse of the new lines only | Daily and per-model usage, sessions, cost, tokens per second, Codex limits |
| macOS process list, memory and sensors | `sampler.rs` | `sysinfo`, once a second by default | Agents, their process trees, machine load, temperature, thermal pressure |
| Anthropic's usage endpoint | `claude_limits.rs` | Claude Code's sign-in from the Keychain, then one HTTPS request every 5 minutes | Claude plan name and limit windows |

### Inside the Rust core

Four background threads keep shared state up to date. Each one stores its latest result in Tauri-managed state and emits it to the window as an event. The window asks for the stored copy when it opens, so it has data at once instead of waiting for the next tick.

![Inside Mika's Rust core: hook listener, token scanner, sampler and limits poller threads update managed state, which feeds the React UI, the alerts and the menu bar title](docs/architecture/rust-core.svg)

| Thread | Started by | Cadence | Event | Stored in |
| --- | --- | --- | --- | --- |
| Hook listener | `spawn_hook_listener` | On each hook post | `statuses` | `Statuses` (the session tracker) |
| Token scanner | `spawn_token_scanner` | On file changes, in 80–300 ms bursts; a tick every second; a full rescan every 30 s | `tokens` | `LatestTokens` |
| Sampler | `spawn_sampler` | Every 1 s (0.5–10 s in settings) | `agents`, and `statuses` again | `Latest` |
| Limits poller | `spawn_claude_limits` | Every 5 min; 15 min after a failure | `claude_limits` | `LatestClaudeLimits` |

Two more pieces of state belong to alerts: `AlertState` (what the popup is showing) and `Permissions` (requests waiting for an answer). User settings live in `AppSettings`, backed by a JSON file.

Design choices worth knowing:

- **Alerts fire from Rust, not from the window.** macOS pauses a hidden window's page, so an alert raised by the page would never fire while Mika is in the menu bar.
- **Logs are read incrementally.** The agents' logs are large and append-only. Each file keeps a read offset and a little parse state, and a scan only parses the lines added since the last one.
- **The window is one bundle.** The alert popup loads the same build at `index.html#alert` and renders `AlertPopup` instead of `App`.
- **Menu bar only.** Mika runs with the Accessory activation policy and `LSUIElement`, so it has no Dock icon. Closing or minimizing the window hides it; Cmd+Q is replaced by "Hide to Menu Bar".

---

## Application flow

### Startup

1. Tauri builds the app, registers the managed state and the commands, and creates the tray icon with its menu (Show Mika, About, Quit).
2. Settings are loaded from `settings.json` in the app config folder. Every field has a default, so older or partial files still load.
3. The main window starts invisible. It is shown unless Mika was started by the login item, which passes `--hidden`.
4. The four threads start. The token scanner does one full scan of the logs, then switches to watching for changes.
5. The window calls `latest_snapshot`, `session_statuses`, `latest_tokens` and `claude_limits` for the current data, then subscribes to the events.

### A session's status

Each hook event moves a session between states. The tracker keeps one entry per session id; the menu bar title and the header show the most urgent one.

![A session's status as a state machine: idle, thinking, working, needs you, done and your turn, driven by hook events](docs/architecture/session-states.svg)

| Hook event | New state |
| --- | --- |
| `SessionStart` | Idle |
| `UserPromptSubmit`, `PostToolUse`, `PostToolUseFailure` | Thinking |
| `PreToolUse` | Working, with the tool's name |
| `PermissionRequest` | Needs you |
| `Notification` of type `permission_prompt`, `elicitation_dialog`, `elicitation_url_dialog` or `agent_needs_input` | Needs you |
| `Notification` of type `idle_prompt` | Your turn |
| `Stop` | Done, which settles to Idle after 30 s |
| `SessionEnd` | The session is removed |

Urgency runs Idle < Done < Thinking < Working < Your turn < Needs you. Sessions with no event for 6 hours are dropped, which covers agents that crashed or were killed.

Sessions without hooks still show activity: the window marks an agent as working when it averages 5% CPU or more over its last three samples.

### Alerts and answering a permission request

When a session's state changes, `alert_on_change` decides what to do:

| Change | What happens |
| --- | --- |
| → Needs you | The popup appears with a ringtone (three rings, two seconds apart) and stays until you act. |
| → Needs you, while the popup is waiting on another session | A chime and a notification, so the first request is never covered. |
| → Your turn | Nothing is blocked, so at most one chime (no sound by default). |
| → Done | A popup that clears itself after 12 s, if you turned on "alert when finished". |

Sounds respect the sound setting and quiet hours.

Permission requests go further. Mika's `PermissionRequest` hook posts the request and waits, and Mika holds that HTTP request open until you choose.

![Answering a permission request: the hook posts to the listener and waits, the core shows the popup, you choose, and the listener replies with the decision JSON](docs/architecture/permission-flow.svg)

- **Allow** and **Deny** answer the hook with the agent's decision JSON, and the agent carries on without showing its own prompt.
- **Always allow** also saves a permission rule. Only Claude Code accepts that, so Codex never offers it, and it is never offered for commands the popup flags as risky (for example `sudo`, or piping a download into a shell).
- **In Terminal**, no answer within 9 minutes, or Mika not running all answer with nothing, and the agent falls back to its normal prompt. The hook's own timeout is longer than Mika's, so the fallback is always a clean empty reply rather than a failed hook.
- Requests are queued and shown one at a time. Each waits on its own thread, so other hooks keep flowing.
- The buttons ignore clicks for a moment after a new request appears, so a click meant for another window cannot approve something you have not read.

With "answer from Mika" off, the listener replies with an empty `204` at once and the popup only tells you which session is waiting.

### Jumping to a session

`focus_agent` finds the agent's controlling terminal with `ps`, then uses AppleScript to select the Terminal or iTerm2 tab on that terminal. If that is not possible it brings the hosting app to the front. Mika only ever opens an `.app` bundle that exists, never an arbitrary path. Some hosts run agents in a background helper; those are mapped to the app that owns the window.

### Token usage

1. `notify` watches `~/.claude/projects`, `~/.codex/sessions` and `~/.codex/session_index.jsonl` through FSEvents.
2. Agents write in bursts, so changes are gathered for up to 300 ms (ending early after an 80 ms quiet gap) and read in one pass.
3. Each changed file is read from its last offset. Claude lines carry usage per assistant message; Codex lines carry running totals, so Mika takes the difference from the previous total.
4. Usage is added up per day and model, and per session. The session lists are built for today, this month and all time, so switching range in the window is instant.
5. Cost is estimated from public list prices per million tokens (input, output, cache read, cache write). Unknown models fall back to their family's price.
6. Stats are published at least once a second, so tokens per second (a 60-second window) decays to zero when the agents stop.

Codex's limits come from the same logs. Codex's automatic review runs are counted per model and day, and shown as one summary row instead of as sessions.

### Resources

The sampler walks the process list, recognises agents by executable name (and Claude Code installed through npm by its script path), and attaches every descendant process as a tree. Codex processes that share a parent are merged into one agent, because the ChatGPT app runs Codex as sibling processes. Each agent also records the app hosting it, which is what "jump to the session" opens.

Temperature is the hottest CPU die sensor on Apple Silicon, or the SMC's CPU sensors on Intel. Thermal pressure comes from `NSProcessInfo`.

---

## The interface

The window is React 19 with TypeScript, built by Vite.

| File | Role |
| --- | --- |
| `App.tsx` | The main window: header, range picker, the Overview, Agents and Settings pages, and the tab bar. Subscribes to the events. |
| `Usage.tsx` | Totals, limits, models, sessions, the system panel, and the Activity heatmap and trend. |
| `Mika.tsx` | The ghost. Each state has a library of short clips, played in random order with a rest between them. |
| `AlertPopup.tsx` | The popup window: needs you, finished, or a permission request to answer. |
| `SettingsPage.tsx`, `settings.tsx` | The settings page and a provider that keeps settings in sync across both windows. |
| `motion.ts` | All motion: rolling numbers, the sliding pill, the staggered entrance, pops and shakes (GSAP), and the heatmap ripple, trend draw-in and hover spring (anime.js). |
| `liquid.ts` | The Liquid Glass skin, drawn with WebGPU behind the DOM. |
| `scrollbars.ts` | Thin scrollbars that only show while the mouse is active over Mika. |

- **Layout.** Narrow windows show one page at a time. From 760px wide, Overview and Agents sit side by side.
- **Motion.** Three levels: full, subtle and off. Off is forced when macOS "Reduce motion" is on. Entrances and chart animations run at full only.
- **Skins.** Classic is flat cards with hairline borders. Liquid Glass paints a soft wallpaper and a refracting glass shape under each card and control, with sliders for transparency, frost, refraction, shine and shadow, and four backdrops.
- **Menu bar title.** Full (state, agent count, and the agents' CPU and memory), compact (symbol and count) or icon only.

---

## Reference

### Events (Rust → window)

| Event | Payload | Sent when |
| --- | --- | --- |
| `agents` | `Snapshot`: machine load and every agent with its process tree | Each sample |
| `statuses` | `SessionStatus[]` | Each hook event and each sample |
| `tokens` | `TokenStats`: days, sessions per range, reviews, Codex limit, tokens per second | Each change, and at least once a second |
| `claude_limits` | `ClaudeLimits`: plan and limit windows | Each successful poll |
| `settings` | `Settings` | Settings saved |
| `alert` | `AlertInfo`, to the popup window only | The popup is shown or updated |

### Commands (window → Rust)

| Group | Commands |
| --- | --- |
| Latest data | `latest_snapshot`, `session_statuses`, `latest_tokens`, `claude_limits` |
| Settings | `get_settings`, `set_settings`, `preview_sound` |
| Hooks | `hooks_status`, `install_hooks`, `remove_hooks` |
| Alerts | `show_alert`, `current_alert`, `dismiss_alert`, `open_alert_target`, `answer_permission`, `alert`, `focus_app` |
| Diagnostics | `log_frontend` |

### Hook endpoints

The listener is `tiny_http` on `127.0.0.1:47321`. It accepts bodies up to 1 MB.

| Path | Posted by | Behaviour |
| --- | --- | --- |
| `/hook` | Claude Code status hooks | Fire and forget. The hook's `curl` gives up after 1 s, so a stopped Mika never slows an agent. |
| `/hook/codex` | Codex status hooks | The same, tagged as Codex. |
| `/permission` | Claude Code `PermissionRequest` | Held open until answered. The hook waits up to 590 s. |
| `/permission/codex` | Codex `PermissionRequest` | The same, tagged as Codex. |

### Files Mika touches

| Path | Access | Why |
| --- | --- | --- |
| `~/.claude/projects/**/*.jsonl` | Read | Claude Code token usage and sessions |
| `~/.codex/sessions/**/*.jsonl`, `~/.codex/session_index.jsonl` | Read | Codex token usage, session titles and limits |
| `~/.claude/settings.json`, `~/.codex/hooks.json` | Read and write, only from Settings → Integrations | Installing or removing hooks. The current file is backed up first as `<name>.bak-mika-<timestamp>`. Only Mika's own entries are added or removed. |
| `settings.json` in the app config folder | Read and write | Your settings |
| Keychain item `Claude Code-credentials` | Read | Claude plan limits. Can be turned off. |

---

## Privacy and security

Mika runs entirely on your Mac. It reads your agents' local logs and process list, and never uploads your code, prompts or usage anywhere.

- **One network request.** The only thing Mika asks the internet for is your Claude plan limits, from `api.anthropic.com`, using the sign-in Claude Code already keeps in the Keychain. The token is held in memory for that request and never logged. Turn "Claude limits" off in Settings and Mika touches neither the Keychain nor the network.
- **Local listener only.** The hook listener binds to `127.0.0.1`, so nothing outside your Mac can reach it.
- **Apple Events.** Mika asks for automation permission so it can select the terminal tab of the agent that needs you. Without it, Mika brings the app to the front instead.
- **Hooks are yours.** Installing is one click and removing is one click. Mika backs up your file first and leaves your own hooks alone.

---

## Development

```bash
npm install
npm run tauri dev
```

This starts Vite on port 1420 and the Rust app, with hot reload for the window. Dev builds optimize dependencies fully and Mika's own code lightly, because an unoptimized build takes about 30 s to parse the logs at startup.

| Task | Command |
| --- | --- |
| Run the Rust tests | `cd src-tauri && cargo test` |
| Type-check and build the window | `npm run build` |
| Build a universal app and DMG | `npm run release:mac` |

`release:mac` builds for Apple Silicon and Intel. With a "Developer ID Application" certificate in the keychain and `APPLE_ID`, `APPLE_PASSWORD` and `APPLE_TEAM_ID` set (in the environment or in `.env.release.local`), the app is signed, notarized and stapled. Without them it is ad-hoc signed, which other Macs only open through System Settings → Privacy & Security → Open Anyway.

### Project layout

```
src/                  React window (see "The interface")
src-tauri/src/
  lib.rs              App setup, tray, threads, commands, alert decisions
  status.rs           Hook listener and the session state machine
  permission.rs       Pending permission requests and decision JSON
  alert.rs            The popup window, ringtone, and jumping to a session
  tokens.rs           Incremental log parsing, usage, cost, Codex limits
  sampler.rs          Processes, agents, machine load, temperature
  claude_limits.rs    Claude plan limits
  settings.rs         Settings file and the hooks installer
hooks/                Hook templates for Claude Code and Codex
docs/architecture/    The diagrams on this page, as SVG and HTML
scripts/              release-macos.sh
```

### Stack

| Layer | Libraries |
| --- | --- |
| Shell | Tauri 2 (tray icon, autostart and opener plugins) |
| Core | Rust 2024, `sysinfo`, `tiny_http`, `notify`, `ureq`, `serde`, `chrono`, `objc2-foundation` |
| Window | React 19, TypeScript, Vite, GSAP, anime.js, `@liquid-dom/core` (WebGPU), Geist |

---

## Good to know

- Costs are estimates based on public API prices, not your actual bill.
- Claude Code doesn't record its context size, so the context bar appears for Codex sessions only.
- Jumping to a session selects the exact tab in Terminal and iTerm2. In other apps it brings the app to the front, but can't pick a tab or thread inside it.
- Claude limits come from a service Anthropic hasn't documented publicly and may change.
- Codex asks you once to trust newly installed hooks.

---

## Licence

MIT. See [LICENSE](LICENSE).
