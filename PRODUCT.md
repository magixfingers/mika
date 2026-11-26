# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

The interface is web-rendered (React in a Tauri 2 webview) but ships only as a macOS menu-bar app, so it follows Mac conventions: overlay traffic lights, menu-bar-only presence (no Dock icon), hide-to-menu-bar on close.

## Users

Developers who run several Claude Code and Codex sessions at once on their own Mac and step away while the agents work. Their job: know the moment a session is blocked on them, answer it fast, and keep an eye on token spend, plan limits and machine load without watching terminals.

## Product Purpose

Mika is an observability and control layer for local AI coding agents. It reports each session's state, token use and cost, remaining Claude and Codex plan limits, and machine and per-agent load. When a session needs the user, it raises an always-on-top alert with a ringtone, lets the user answer the permission request in place, and brings the right terminal tab to the front.

Success: the user never misses a blocked agent, and one glance at the menu bar or window tells them where everything stands.

## Positioning

- **Answer in place:** approve, always-allow or deny a permission request from Mika's popup, and jump straight to the exact Terminal or iTerm2 tab.
- **Fully local:** everything comes from the agents' local logs, hooks and the process list. Nothing is uploaded; the only network call is the optional Claude plan-limits request.
- **One glance:** status, tokens, limits and machine load for every agent in a single menu-bar window.
- **A character:** the Mika ghost shows the agents' mood (idle, thinking, working, your turn, needs you, done) in the header, the menu bar and the alert.

## Operating Context

- Lives in the macOS menu bar; the window is opened, glanced at, and hidden again.
- Agents run in Terminal, iTerm2, other terminals, or the ChatGPT app (Codex).
- Hooks are optional: tokens, sessions and machine health work without them; hooks add live status, alerts and in-place answers. Hooks are installed from Settings → Integrations.
- Pages: Overview (machine load, token totals and cost, plan limits, models, activity heatmap and trend), Agents (running agents with process trees, session list), Settings. A separate always-on-top alert popup.

## Capabilities and Constraints

- macOS only, for now.
- Supports Claude Code and Codex (CLI and ChatGPT app). Claude Code supports Always allow; Codex supports Allow and Deny.
- The window is small and should show all of Overview without scrolling; it sizes itself to fit.
- Two skins stay supported: Classic and Liquid Glass.
- Costs are estimates from public API prices, not the actual bill.
- Claude limits come from an undocumented Anthropic endpoint that may change.
- Claude Code does not record context size, so the context bar appears for Codex sessions only.

## Brand Commitments

- Name: Mika.
- The Mika ghost character is a fixed brand asset (`src/Mika.tsx`, `src-tauri/icons/mika-icon.svg`, tray icon).
- Open source under the MIT licence.

## Evidence on Hand

- README with capabilities, architecture and privacy details (`README.md`).
- Architecture diagrams (`docs/architecture/*.svg`).
- App screenshots: Overview, Agents, Settings (`docs/screenshots/*.webp`).
- Product website source (`web/`).
- No testimonials, user counts, press or benchmarks exist; do not invent them.

## Product Principles

1. **Never miss a blocked agent.** Alerts and the "needs you" state outrank everything else.
2. **Glanceable over exhaustive.** The essentials fit in one small window; detail is a click away, not in the way.
3. **Local and trustworthy.** Read only what's on the machine, say plainly what leaves it, and leave the user's own hooks untouched.
4. **Act, don't just watch.** Wherever Mika shows a problem, it offers the next step: answer, focus the session, or open the right app.
5. **Personality with restraint.** The ghost carries the character; the rest of the interface stays quiet and precise.
