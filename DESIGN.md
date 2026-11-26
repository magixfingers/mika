---
name: Mika
description: A quiet instrument in the menu bar for watching local AI coding agents.
colors:
  ink: "#171717"
  graphite: "#4d4d4d"
  fog: "#8f8f8f"
  ash: "#a1a1a1"
  hairline: "#ebebeb"
  hairline-soft: "#f2f2f2"
  paper: "#fafafa"
  card-white: "#ffffff"
  switch-off: "#d4d4d4"
  signal-blue: "#0070f3"
  alarm-red: "#ee0000"
  turn-violet: "#7928ca"
  done-mint: "#50e3c2"
typography:
  display:
    fontFamily: "Geist Variable, Inter, Arial, sans-serif"
    fontSize: "clamp(1.9rem, 8vw, 2.8rem)"
    fontWeight: 600
    lineHeight: 1
    letterSpacing: "-0.05em"
    fontFeature: "tnum"
  headline:
    fontFamily: "Geist Variable, Inter, Arial, sans-serif"
    fontSize: "1.43rem"
    fontWeight: 600
    lineHeight: 1.4
    letterSpacing: "-0.02em"
  body:
    fontFamily: "Geist Variable, Inter, Arial, sans-serif"
    fontSize: "clamp(13px, 0.72rem + 0.4vw, 15px)"
    fontWeight: 400
    lineHeight: 1.43
  body-small:
    fontFamily: "Geist Variable, Inter, Arial, sans-serif"
    fontSize: "0.86rem"
    fontWeight: 400
    lineHeight: 1.33
  label:
    fontFamily: "Geist Mono Variable, ui-monospace, SFMono-Regular, Menlo, monospace"
    fontSize: "0.78rem"
    fontWeight: 500
    lineHeight: 1.33
  numeral:
    fontFamily: "Geist Mono Variable, ui-monospace, SFMono-Regular, Menlo, monospace"
    fontSize: "1rem"
    fontWeight: 500
    fontFeature: "tnum"
rounded:
  cell: "2px"
  inner: "4px"
  control: "6px"
  card: "10px"
  pill: "9999px"
spacing:
  hair: "2px"
  xs: "4px"
  sm: "6px"
  md: "8px"
  lg: "12px"
  xl: "16px"
components:
  card:
    backgroundColor: "{colors.card-white}"
    rounded: "{rounded.card}"
    padding: "10px 12px"
  button-primary:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.card-white}"
    rounded: "{rounded.control}"
    height: "2em"
    padding: "0 10px"
  button-secondary:
    backgroundColor: "{colors.card-white}"
    textColor: "{colors.ink}"
    rounded: "{rounded.control}"
    height: "2em"
    padding: "0 10px"
  pill-button:
    backgroundColor: "{colors.card-white}"
    textColor: "{colors.graphite}"
    rounded: "{rounded.pill}"
    height: "2em"
    padding: "0 14px"
  pill-button-primary:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.card-white}"
    rounded: "{rounded.pill}"
    height: "2em"
    padding: "0 14px"
  segmented-selected:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.card-white}"
    rounded: "{rounded.pill}"
    height: "2.1em"
  icon-button:
    backgroundColor: "{colors.card-white}"
    textColor: "{colors.graphite}"
    rounded: "{rounded.pill}"
    size: "2.4em"
  toggle-off:
    backgroundColor: "{colors.switch-off}"
    rounded: "{rounded.pill}"
    width: "36px"
    height: "20px"
  toggle-on:
    backgroundColor: "{colors.ink}"
    rounded: "{rounded.pill}"
    width: "36px"
    height: "20px"
  meter:
    backgroundColor: "{colors.signal-blue}"
    rounded: "{rounded.pill}"
    height: "3px"
---

# Design System: Mika

## Overview

**Creative North Star: "The Quiet Instrument"**

Mika is a gauge on the desk, not a dashboard on the wall. It is read in a glance and then put away, so everything at rest is ink and grey on near-white paper: hairline cards, mono eyebrows, tabular numbers. Nothing competes for attention while the agents are fine. When something changes, the instrument speaks through exactly two channels: the color of the Mika ghost and the status line beside it. Red means one thing only, an agent is blocked on you.

The system descends from Vercel's Geist language: Geist Sans for reading, Geist Mono for labels and figures, a flat paper-and-hairline surface, and pill-shaped ink selections. Density is high but calm. The window is small (360pt wide by default) and shows all of Overview without scrolling, so spacing is tight and every row earns its place. Personality lives in the ghost and in motion (springy pops, a shake on alert, bars that fill on entrance), never in decoration.

A second skin, Liquid Glass, replaces the paper with a WebGPU-rendered glass scene; the cards lose their fill and border and are drawn as glass. Both skins share every token below; only the surface material changes.

**Key Characteristics:**
- Ink on paper at rest; color appears only for agent state and data.
- Flat surfaces with hairline borders; no shadows on cards.
- Geist Sans for text, Geist Mono for eyebrows, rates and figures, tabular numerals everywhere numbers change.
- Ink pills mark the selected option in every segmented control.
- Compact: the whole Overview fits one small window.
- The Mika ghost is the only illustrative element.

## Colors

A near-monochrome paper palette with five state colors held in reserve.

### Primary
- **Signal Blue** (#0070f3): the one accent with more than one job, and every job is data or state: thinking/working ghost color, filled portion of every meter and limit bar, the activity heatmap ramp and trend line, and the 2px focus ring. Never a fill for buttons or a decorative tint.

### Secondary
- **Alarm Red** (#ee0000): "needs you". The ghost, the pulsing status line, the alert popup's label, and the risk badge on a dangerous command. Also the serious/critical thermal warning. Nothing else may be red.
- **Turn Violet** (#7928ca): "your turn". A calm handback, deliberately not red: the agent finished and is waiting, nothing is blocked.
- **Done Mint** (#50e3c2): "done". The ghost after an agent finishes, before it settles to idle.

### Neutral
- **Ink** (#171717): primary text, headings, the selected pill in every segmented control, primary buttons, toggles when on.
- **Graphite** (#4d4d4d): secondary text, unselected control labels, cost line.
- **Fog** (#8f8f8f): muted text, mono eyebrows, idle ghost, axis labels.
- **Ash** (#a1a1a1): faintest text.
- **Hairline** (#ebebeb): every card and control border, dividers between limit groups and settings rows, slider track.
- **Hairline Soft** (#f2f2f2): empty heatmap cells.
- **Paper** (#fafafa): the window canvas behind the cards (Classic skin).
- **Card White** (#ffffff): card and control surfaces; text on ink.
- **Switch Off** (#d4d4d4): the track of a toggle that is off.

Meter tracks are Signal Blue at 12% (`rgba(0, 112, 243, 0.12)`); the heatmap fills cells with Signal Blue at the cell's level.

### Named Rules
**The State-Only Rule.** Color always means an agent state or a data value. Chrome (buttons, tabs, cards, icons) stays ink, grey and white. If a new element wants color, it must be showing a state or a number.

**The One Red Rule.** Alarm Red is reserved for "needs you" and genuine danger. A "your turn" handback is violet, a finished agent is mint, and an error that doesn't need the user right now is not red.

## Typography

**Display Font:** Geist Variable (with Inter, Arial)
**Body Font:** Geist Variable (with Inter, Arial)
**Label/Mono Font:** Geist Mono Variable (with ui-monospace, SFMono-Regular, Menlo)

**Character:** A crisp, engineered sans for reading paired with a mono for anything that is a label, a rate or a measurement. The mono makes the instrument readable at a glance: eyebrows and figures line up and never jitter.

### Hierarchy
- **Display** (600, clamp(1.9rem, 8vw, 2.8rem), line-height 1, -0.05em, tabular): the token total on Overview. One per page, rolling as it counts.
- **Headline** (600, 1.43rem, 1.4, -0.02em): the "Mika" wordmark in the header.
- **Body** (400, 13–15px fluid with window width, 1.43): list rows, setting labels, session titles.
- **Body Small** (400, 0.86rem, 1.33): status line, limit rows, hints, cost, segmented and button labels.
- **Label** (Geist Mono 500, 0.78rem, 1.33, uppercase, Fog): card eyebrows (LIMITS, MODELS, ACTIVITY), load labels (CPU, GPU, RAM, TEMP), axis text at 0.72rem.
- **Numeral** (Geist Mono 500, tabular): tokens per second in the tab bar, slider readouts, process-table figures.

### Named Rules
**The Tabular Rule.** Every number that updates live uses tabular figures (`font-variant-numeric: tabular-nums`) so values change in place without shifting the layout.

**The Mono Eyebrow Rule.** Section titles are small uppercase Geist Mono in Fog, never large bold headings. The data is the headline.

## Layout

A single column of cards inside a fixed window, built as a three-row grid: header, the page (which scrolls only if it must), and the tab bar. The window opens 360pt wide (minimum 320×480) and sizes its height to fit the whole Overview page, capped at the screen's work area.

- **Window padding:** 34px top (clears the overlay traffic lights), 12px sides and bottom.
- **Rhythm:** 8px between header, page and tab bar, and between cards. Inside a card, 8px between rows; 6px within lists; 4–6px inside a limit group.
- **Grids inside cards:** the system-load card is a 2×2 grid (CPU | GPU / RAM | TEMP), each cell a label and value line above a 3px meter. Limit rows are one line: label and reset time, a 48px bar, then "% left". The token total and cost share one baseline.
- **Wide windows:** from 760px wide, Overview and Agents sit side by side in two equal columns; Settings always takes the full width, capped at 640px.
- **Scrolling:** pages scroll on their own with a 4px scrollbar that stays invisible until the user scrolls; its space is reserved so cards keep the header's width.

## Elevation & Depth

Flat by default. Cards and controls sit on the paper with a 1px hairline border and no shadow; hierarchy comes from the white card on the near-white canvas, from type, and from ink selections. The only shadows are functional and tiny: the toggle knob (`0 1px 2px rgba(0,0,0,0.2)`) and the floating glass controls in the Classic skin.

The Liquid Glass skin is where depth lives: a WebGPU scene renders the backdrop and draws cards, the range switch and the tab bar as refracting glass (transparency, frost, refraction, shine and shadow are user settings). In that skin the cards drop their fill and border.

### Shadow Vocabulary
- **Glass float** (`box-shadow: 0 2px 2px rgba(0,0,0,0.04), 0 8px 16px -4px rgba(0,0,0,0.08)` with `backdrop-filter: blur(20px) saturate(180%)` over `rgba(255,255,255,0.6)`): the range switch and tab bar in the Classic skin.
- **Knob** (`box-shadow: 0 1px 2px rgba(0,0,0,0.2)`): the toggle's thumb.

### Named Rules
**The Flat Card Rule.** Cards never cast shadows. Only floating controls (range switch, tab bar) and the Liquid Glass skin get depth.

## Shapes

Two shape families. Containers are gently rounded rectangles: cards at 10px, inputs, buttons and the settings choice control at 6px, the choice's inner options at 4px, heatmap cells at 2px. Anything you tap to switch modes or pages is a full pill: the range switch, tab bar, alert buttons, the round settings button, toggles and meters. Borders are always 1px hairlines. The Mika ghost is the only free-form silhouette.

## Components

### Buttons
Precise and quiet; the primary action is ink, everything else is white with a hairline.
- **Shape:** 6px radius in Settings; full pills in the alert popup.
- **Primary:** Ink fill, white text, 2em tall, 10px (settings) or 14px (alert) side padding, weight 500, 0.86rem.
- **Secondary:** Card White with a hairline border and Ink text; in the alert, Graphite text that turns Ink on hover.
- **Ghost (alert "back to terminal"):** no border or fill, Fog text, 4px padding; quieter than the answers.
- **Focus:** 2px Signal Blue outline, 2px offset. **Disabled:** 50% opacity.

### Segmented Controls (range switch, tab bar, settings choices)
- **Style:** a pill track with 4px padding; options are text-only in Graphite, weight 500.
- **Selected:** a single Ink pill slides under the active option (GSAP), and its label turns white. In Settings, the choice control is a 6px-radius bordered box with 4px-radius Ink selections instead.

### Cards / Containers
- **Corner Style:** 10px.
- **Background:** Card White on Paper (Classic); transparent glass (Liquid Glass).
- **Shadow Strategy:** none (see The Flat Card Rule).
- **Border:** 1px Hairline.
- **Internal Padding:** 10px 12px; 8px between rows.

### Meters
- **Style:** 3px tall pill; track Signal Blue at 12%, fill Signal Blue scaled by the value. Fills ease over 400ms; on entrance they grow from zero in sequence.
- **Mini meter:** the same bar, narrower, for context fill and per-session share.

### Toggles and Sliders
- **Toggle:** 36×20 pill, Switch Off track, white 16px knob; Ink track when on, knob slides 16px in 150ms.
- **Slider:** 2px Hairline track, 14px Ink thumb with a white ring; mono Fog readout beside it.

### Navigation
- **Tab bar:** a floating pill at the bottom with Overview and Agents (with a running count), selected in Ink, and the live tokens-per-second rate in Geist Mono at the right.
- **Header:** the Mika ghost (32px), the wordmark, and a one-line status that ends in "…"; when an agent needs you it turns red, pulses, and becomes a link to that session. The range switch and a round settings button sit at the right; the gear rotates 60° when Settings is open.

### The Mika Ghost (signature)
The ghost's body color is the state: Fog idle, Signal Blue thinking or working, Alarm Red needs you, Turn Violet your turn, Done Mint done. Color changes cross-fade over 300ms, the ghost pops on each state change and the header shakes on an alert. It appears in the header, the menu bar and the alert popup, and is the only illustration in the product.

### Alert Popup
A borderless always-on-top window filled edge to edge with Card White: a mono uppercase eyebrow with a red label, the title, the command in mono (clamped to three lines, red border plus a red RISK badge when dangerous), and a row of pill buttons (Allow, Always allow, Deny, and a ghost "back to terminal").

## Do's and Don'ts

### Do:
- **Do** keep chrome in Ink, Graphite, Fog and Card White; use color only for agent state and data values.
- **Do** reserve Alarm Red (#ee0000) for "needs you" and genuinely dangerous commands.
- **Do** label sections with small uppercase Geist Mono eyebrows in Fog (0.78rem, 500).
- **Do** use tabular figures for every live number.
- **Do** mark the selected option with an Ink pill and white text.
- **Do** give cards a 1px Hairline (#ebebeb) border, 10px radius and no shadow.
- **Do** keep Overview short enough to fit the window without scrolling; prefer one-line rows.
- **Do** use the 2px Signal Blue focus ring with 2px offset on every interactive element.
- **Do** keep both skins working: any new surface must read on paper and on Liquid Glass.

### Don't:
- **Don't** use color for decoration, emphasis or branding outside the ghost and data.
- **Don't** use red for "your turn" or "done"; those are violet and mint.
- **Don't** add shadows to cards or lists.
- **Don't** use large bold section headings; the data is the headline.
- **Don't** add illustrations, icons-as-decoration or a second mascot; the Mika ghost is the only character.
- **Don't** let a new card push Overview past the window height.
