---
name: Savoia DB
description: Graphite-and-amber IDE baseline, captured before the Savoia rebrand.
colors:
  amber: "#E8B84A"
  amber-hover: "#F0C766"
  amber-pressed: "#D4A43A"
  graphite-canvas: "#1E1F22"
  graphite-panel: "#2B2D30"
  graphite-hover: "#393B40"
  graphite-row-hover: "#26282B"
  selection-blue: "#2E436E"
  ink: "#DFE1E5"
  ink-header: "#B4B8BF"
  ink-muted: "#868A91"
  warning-yellow: "#F2CC4B"
  tag-red: "#EF5B5B"
  tag-orange: "#F59E42"
  tag-yellow: "#F2CC4B"
  tag-green: "#5BC97A"
  tag-cyan: "#4FC3E8"
  tag-blue: "#5B8DEF"
  tag-purple: "#9B6BEF"
  tag-pink: "#EF6BC4"
typography:
  title:
    fontFamily: "system-ui"
    fontSize: "14px"
    fontWeight: 600
  body:
    fontFamily: "system-ui"
    fontSize: "14px"
    fontWeight: 400
  label:
    fontFamily: "system-ui"
    fontSize: "12px"
    fontWeight: 500
  meta:
    fontFamily: "system-ui"
    fontSize: "12px"
    fontWeight: 400
  data:
    fontFamily: "monospace"
    fontSize: "14px"
    fontWeight: 400
  code:
    fontFamily: "monospace"
    fontSize: "13px"
    fontWeight: 400
rounded:
  md: "6px"
  lg: "8px"
  full: "9999px"
spacing:
  hair: "2px"
  xs: "4px"
  sm: "6px"
  md: "8px"
  lg: "12px"
  xl: "16px"
components:
  button-primary:
    backgroundColor: "{colors.amber}"
    textColor: "{colors.graphite-canvas}"
    rounded: "{rounded.md}"
  button-primary-hover:
    backgroundColor: "{colors.amber-hover}"
  button-primary-active:
    backgroundColor: "{colors.amber-pressed}"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    rounded: "{rounded.md}"
  button-ghost-hover:
    backgroundColor: "{colors.graphite-hover}"
  input:
    backgroundColor: "{colors.graphite-canvas}"
    textColor: "{colors.ink}"
    rounded: "{rounded.md}"
  tree-row:
    textColor: "{colors.ink}"
    typography: "{typography.body}"
    padding: "2px 8px 2px 6px"
  tree-row-selected:
    backgroundColor: "{colors.selection-blue}"
  panel-header:
    backgroundColor: "{colors.graphite-panel}"
    typography: "{typography.title}"
    height: "34px"
    padding: "0 12px"
  toolbar:
    height: "34px"
    padding: "0 8px"
  grid-header:
    backgroundColor: "{colors.graphite-panel}"
    textColor: "{colors.ink-header}"
  grid-cell-active:
    backgroundColor: "{colors.selection-blue}"
    textColor: "{colors.ink}"
  status-bar:
    backgroundColor: "{colors.graphite-panel}"
    textColor: "{colors.ink-muted}"
    typography: "{typography.meta}"
---

<!-- BASELINE: records the amber IDE look as of 2026-10-09, before the Savoia green/red rebrand. The rebrand's new-work pass will replace this file; keep a copy as the reference for what changed. -->

# Design System: Savoia DB

## Overview

**Creative North Star: "The Graphite IDE"**

Savoia today is a quiet, dark, DataGrip-shaped workspace. Neutral graphite panels sit one step lighter than the canvas. Text is cool off-white. Only one warm color, amber, is allowed to speak. The chrome recedes so that SQL and result data are the brightest, most legible things on screen. Density is IDE-grade: 14px rows, 12px metadata, 30–34px toolbars, and no decorative whitespace.

The system is flat. Panels separate by tone and 1px hairlines, not shadows. Amber marks where you act and where you are (primary button, caret, focus ring, active grid cell), and nothing else. Connection colors are a second, functional channel that tells you which database you are touching.

Rejected: DBeaver/Eclipse clutter (toolbar soup, nested panes, gray-on-gray Java chrome), Electron SaaS gloss (gradients, big rounded cards, row-wasting whitespace) and neon-terminal cosplay (hacker green, glow, synthwave).

**Key Characteristics:**
- Quiet: the UI is dimmer than the content.
- Two-step tonal graphite, no elevation shadows in the main window.
- A single warm accent, used for action and focus only.
- Connection colors carried from explorer to console tab.
- Dense, keyboard-first IDE rows.

## Colors

A cool graphite neutral ramp with one warm amber signal and a fixed set of eight connection tags.

### Primary
- **Lamp Amber** (`amber`): primary buttons ("Save & Connect", "Add data source"), the editor caret, focus rings, the active result-cell border, the active result tab underline, and the brand dot in the title bar. It also appears in the app icon. `amber-hover` and `amber-pressed` are its hover and pressed states.

### Neutral
- **Graphite Canvas** (`graphite-canvas`): window background, editor, result grid, tab bar, and the dark text on amber buttons.
- **Graphite Panel** (`graphite-panel`): Database Explorer sidebar, title bar, status bar, grid header. This is the one-step-lighter surface.
- **Graphite Hover** (`graphite-hover`): list and button hover.
- **Row Hover** (`graphite-row-hover`): result grid row hover, kept subtler than list hover because grids are scanned rapidly.
- **Selection Blue** (`selection-blue`): selected tree row and active grid row. This is the only cool tint in the shell, and it is borrowed from JetBrains.
- **Ink** (`ink`): primary text. **Header Ink** (`ink-header`): grid column headers. **Muted Ink** (`ink-muted`): counts, versions, row numbers, status text, inactive tabs.

### Functional
- **Connection tags** (`tag-red` … `tag-pink`): user-chosen per connection, and painted on the explorer data-source icon and the console-tab database icon. Red is the conventional production tag.
- **Status colors** are inherited from GPUI Kit's defaults (Tailwind green-400, red-400 and yellow-400), not Savoia tokens. Green is the Run icon and the connected dot, red is the failed dot and errors, and yellow is the host-trust warning. `warning-yellow` is hard-coded on warning notification icons.

### Named Rules
**The One Lamp Rule.** Amber marks action and focus only, so it should be visible in only a few small places on a screen. It is never used for decoration, headings or large fills.

**The Tag Travels Rule.** A connection's color appears wherever that connection is the subject: its explorer node, its console tab and the status line. That way production is recognizable without reading.

## Typography

**UI Font:** the platform system UI font (GPUI's `.SystemUIFont`).
**Data/Code Font:** monospace. The result grid uses the generic `monospace` family, and the SQL editor uses GPUI Kit's resolved mono font.

**Character:** native and invisible. The UI type borrows the OS voice so the app feels like it belongs on the machine, and monospace marks everything that is data.

### Hierarchy
- **Title** (600, 14px): panel headings ("Database Explorer") and the app name in the title bar.
- **Body** (400, 14px): tree rows, tab labels, form copy.
- **Label** (500, 12px): form field labels.
- **Meta** (400, 12px): object counts, server versions, status bar, result tabs, hints.
- **Data** (400, 14px mono): result cells. Numbers and row numbers are right-aligned.
- **Code** (400, 13px mono): the SQL editor.

### Named Rules
**The Two Sizes Rule.** The shell uses only 14px and 12px. Hierarchy comes from weight and muted color, never from a bigger size.

## Layout

The window follows the DataGrip IDE frame: title bar, a horizontally resizable split (explorer at 320px by default, adjustable from 220 to 560px, with the console filling the rest) and a status bar. Inside a console, a toolbar sits over the SQL editor, which sits over a vertically resizable result area (300px by default, 120px minimum).

Fixed bar heights: panel header 34px, console toolbar 34px, explorer toolbar 30px, result tab strip 28px. The tree indents 14px per level from a 6px base. Toolbar groups are split by 1px vertical separators (14–16px tall). Spacing steps are 2, 4, 6, 8, 12 and 16px. The default window is 1360×860, with a 900×560 minimum.

Dialogs are the only centered compositions. The connection form is a 640px dialog of two-column field rows, with a fixed-width 110px port column.

## Elevation & Depth

The main window is flat. Depth comes from two tonal steps (canvas `#1E1F22`, panel `#2B2D30`) and 1px hairline borders in the panel tone. Overlays (dialogs, menus, tooltips, notifications) use GPUI Kit's default shadow because they genuinely float above the workspace.

### Named Rules
**The Tone Not Shadow Rule.** Docked surfaces separate by tone and hairlines. Only floating layers get a shadow.

## Shapes

Gently rounded controls (6px), with slightly softer dialogs and notifications (8px). Status dots, the brand dot and color swatches are full circles at 6px, 8px and 16px respectively. Panels and bars are square and edge-to-edge.

## Components

### Buttons
- **Shape:** gently rounded (6px). The shell uses GPUI Kit's small and xsmall sizes almost exclusively.
- **Primary:** amber fill with graphite-canvas text. Reserved for the single committing action in a view ("Save & Connect", "Add data source").
- **Ghost:** transparent, icon-only in toolbars (run, cancel, history, explain, and the explorer tools), with tooltips. Hover fills with graphite hover. The Run icon is green.
- **Default:** neutral bordered buttons for secondary actions (Test, Save, Import).

### Inputs / Fields
- **Style:** GPUI Kit small inputs and selects. The 12px medium label sits above the field with a 4px gap. Password fields have a reveal toggle.
- **Focus:** amber ring.
- **Section headers:** 12px muted text with extra top padding ("SSH tunnel").

### Navigation
- **Database Explorer tree:** chevron, then a 14px icon tinted with the connection tag (or muted), then the label, then muted metadata (count, version, "connecting…") and a 6px status dot. Selection fills with selection blue. Double-click connects.
- **Console tabs:** an underline tab bar. The tab carries a database icon tinted with the connection tag and the label `console [name]`.
- **Result tabs:** 12px text labels, with the active tab underlined in amber.

### Result Grid
A virtualized table with a fixed 44px row-number column in muted ink, monospace 14px cells, right-aligned numerics, a graphite-panel header with header ink, subtle row hover, and the active cell bordered in amber over selection blue.

### Connection Color Swatches
A row of 16px circles (no color plus eight tags), with a 2px border that switches from the border tone to ink when chosen.

## Do's and Don'ts

### Do:
- **Do** take every color from `crates/savoia-app/src/theme.rs` tokens. Connection tags come from `ConnectionColor::rgb()`.
- **Do** keep the shell at 14px and 12px, and use weight and muted ink for hierarchy.
- **Do** keep amber for primary action, caret, focus and the active cell only.
- **Do** carry the connection color onto every surface that is about that connection.
- **Do** separate docked panels with tone and 1px hairlines.

### Don't:
- **Don't** reproduce DBeaver/Eclipse clutter: no toolbar soup, nested panes, or gray-on-gray chrome.
- **Don't** use Electron SaaS gloss: no gradients, oversized rounded cards, or whitespace that costs rows.
- **Don't** use neon-terminal styling: no hacker green, glow, or synthwave accents.
- **Don't** put shadows on docked panels.
- **Don't** hard-code colors in views. The current `0xF2CC4B` warning icons are a known exception that should move into the theme.
