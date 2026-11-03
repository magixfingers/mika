# Mika

A macOS menu bar companion for AI coding agents. Mika shows what Claude Code and Codex are doing right now, how many tokens and how much of your plan they're using, what they cost your machine, and tells you, with a sound, when one of them needs you.

Everything runs locally. Mika reads the agents' own log files and process list on your Mac; the only network request it makes is the one Claude Code itself makes to check your plan limits.

Built with Tauri 2 (Rust) and React + TypeScript.

## Features

### Mika, the status character

A small ghost in the header that reflects the most urgent thing any agent is doing. Each state has its own color and a set of animations that play in random order, so it never loops the same move.

| State | Color | Animations |
| --- | --- | --- |
| Working | Blue | hop, typing, spin, dash, spark burst |
| Thinking | Blue | thought dots, eye scan, head tilt with "?", float |
| Needs you | Red | wiggle, jump with "!", radar ring, knock-knock |
| Done | Cyan | celebrate, happy nod |
| Idle | Grey | breathe, blink, doze |

Where the state comes from:

- **Claude Code hooks** report thinking, the tool in use, waiting for input, and done, the moment they happen (see [Agent status hooks](#agent-status-hooks)).
- **Activity** covers everything else: an agent averaging 5% CPU or more, or tokens flowing in the last minute, counts as working. This is how Codex, and Claude sessions started before the hooks were installed, still show as busy.

### Alerts

When a Claude Code session starts waiting for your input or approval, Mika:

- plays the macOS "Glass" sound,
- bounces its Dock icon,
- turns the status line red ("Needs you · ~/project") and shakes the header,
- and shows a notification where macOS allows it.

It alerts once per wait, not on repeat.

### System load

CPU, RAM, swap and thermal state for the whole Mac, updated every second.

- **CPU:** average across all cores (matches `top`).
- **RAM:** memory in use, the same figure as Activity Monitor's "Memory Used".
- **Swap:** amount swapped, marked *active* while it's growing.
- **Temp:** macOS thermal pressure (normal, fair, serious, critical).

### Tokens and cost

Read from Claude Code (`~/.claude/projects`) and Codex (`~/.codex/sessions`) logs, for **today**, **this month** or **all time**:

- **Total tokens** with an estimated cost from public API list prices. The number rolls up live as tokens arrive.
- **Tokens per second** over the last minute.
- **Models:** tokens and share per model.
- **Activity:** a year-to-date heatmap of daily usage.
- **Trend:** daily totals over the last 88 days, with the peak.

Updates are near real time: Mika is notified by macOS when a log file changes and reads only the new lines, typically within half a second. Codex usage is counted from its running totals, so a skipped usage event can't lose tokens.

### Plan limits

- **Codex:** weekly limit, % left and reset time, read from Codex's logs.
- **Claude:** every plan limit Anthropic reports (e.g. weekly, and per-model weeklies) with % left and reset time, plus your plan (e.g. Max 20x). Mika reads Claude Code's sign-in from the macOS Keychain and asks Anthropic's usage service every 5 minutes, waiting 15 minutes after an error. The token stays in memory and is never stored or logged. macOS asks once for permission. The service isn't publicly documented, so this can break if Anthropic changes it.

### Sessions

Every session in the selected range, most recent first, paginated to fit the window:

- title (or `Claude Code · model` and the session ID when untitled), exact tokens and estimated cost,
- app, model, effort and subagents active in the range,
- time of the last call, number of calls and cache hit rate,
- for Codex, how full the context window is; for Claude, the time left on its prompt cache,
- a bar sized against the busiest session.

Codex's automatic review runs are grouped into one **Codex Auto Review** row. A session that's reporting live status shows a small Mika in place of its logo.

### Running agents

Each running Claude Code or Codex agent with live CPU, memory, a CPU sparkline and its status. Click one to see its full process tree. The ChatGPT app's Codex processes are grouped into one agent.

### Menu bar and window

- The menu bar title shows the agent count, CPU and memory (e.g. `● 2 agents · 4% · 1.7 GB`), with a symbol for the most urgent state.
- Click it to show or hide the window; closing the window keeps Mika running.
- The window opens at 9:16 and grows to fit the Overview on first launch. Drag it by the top area; double-click there to maximize.
- Narrow windows switch between **Overview** and **Agents** pages; windows 760 px or wider show both side by side.

### Design

Light Vercel Geist styling (Geist Sans and Mono, ink on #fafafa, hairline cards), with liquid glass from [liquid-dom](https://github.com/AndrewPrifer/liquid-dom) on the range switch and tab bar, and GSAP motion (rolling numbers, sliding selectors, staggered entrances). All motion turns off when macOS "Reduce motion" is on.

## Agent status hooks

Mika learns what each Claude Code session is doing from [hooks](https://docs.claude.com/en/docs/claude-code/hooks). Each hook posts its JSON input to Mika's listener on `127.0.0.1:47321`, which never leaves your machine. When Mika isn't running, the hook gives up after 1 second and does nothing.

To enable it, merge the `hooks` block from [`hooks/claude-code-settings.json`](hooks/claude-code-settings.json) into `~/.claude/settings.json`. Sessions started after that report their status.

| Hook event | State |
| --- | --- |
| `SessionStart` | Idle |
| `UserPromptSubmit`, `PostToolUse` | Thinking |
| `PreToolUse` | Working (shows the tool name) |
| `Notification` | Needs you (with alert) |
| `Stop` | Done, then idle after 30 seconds |
| `SessionEnd` | Removed |

Codex has no equivalent hooks, so Codex shows as working or idle from its activity, and can't trigger "Needs you".

## Limitations

- macOS only.
- Costs are estimates from public list prices; prices for some models are guesses.
- Claude Code logs don't record the context window size or response timings, so there is no context bar for Claude sessions and no per-session tok/s.
- Codex's compressed archived sessions (`.jsonl.zst`) aren't read yet, so all-time totals can be lower than other tools.

## Development

Prerequisites: [Rust](https://www.rust-lang.org/tools/install), Node 20+, Xcode Command Line Tools.

```bash
npm install
npm run tauri dev
```

Tests:

```bash
cd src-tauri && cargo test
```

Checks against your own data (ignored by default):

```bash
cd src-tauri && cargo test --release -- --ignored --nocapture
```

These print parsed sessions, per-provider totals and system load, for comparison with Token Monitor or macOS tools.

### Layout

| Path | What it does |
| --- | --- |
| `src-tauri/src/sampler.rs` | Process tree, per-agent CPU and memory, system load |
| `src-tauri/src/status.rs` | Hook listener and per-session state machine |
| `src-tauri/src/tokens.rs` | Incremental log parsing: tokens, cost, sessions, limits |
| `src-tauri/src/claude_limits.rs` | Claude plan limits from Anthropic's usage service |
| `src-tauri/src/lib.rs` | Tray, threads, file watching, alerts, commands |
| `src/App.tsx` | Layout, pages, status logic, alerts |
| `src/Usage.tsx` | Totals, limits, models, sessions, activity, trend, system panel |
| `src/Mika.tsx` | The character and its animation library |
| `src/motion.ts` | GSAP helpers |
| `src/liquid.ts` | liquid-dom WebGPU glass |
