---
name: Savoia DB
description: A graphite IDE named by flat Ivrea-green zone bands, with one Savoy-blue Run.
colors:
  ivrea-green: "#1F5A43"
  ivrea-green-hover: "#286C51"
  ivrea-green-pressed: "#184634"
  ivrea-line: "#5FB088"
  savoy-blue: "#1F64B4"
  savoy-blue-hover: "#2B74C7"
  savoy-blue-pressed: "#1A5498"
  savoy-ink: "#FFFFFF"
  band-ink: "#F2EFE8"
  band-ink-muted: "#B5D0C2"
  band-ink-disabled: "#8FAF9F"
  band-rule: "#2F7458"
  blueprint: "#10372A"
  graphite-canvas: "#161A17"
  graphite-panel: "#1D211E"
  graphite-seam: "#111412"
  graphite-rule: "#272C28"
  graphite-hover: "#2A302C"
  graphite-row-hover: "#1F2420"
  selection: "#213A31"
  ink: "#E9E6DF"
  ink-header: "#B8BDB8"
  ink-muted: "#8D938F"
  status-teal: "#3DB9AE"
  status-vermilion: "#F2713C"
  status-saffron: "#E5B54A"
  status-violet: "#9B8CE8"
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
  caption:
    fontFamily: "system-ui"
    fontSize: "12px"
    fontWeight: 400
  data-mono:
    fontFamily: "monospace"
    fontSize: "14px"
    fontWeight: 400
  editor-mono:
    fontFamily: "monospace"
    fontSize: "13px"
    fontWeight: 400
rounded:
  mark: "4px"
  control: "6px"
  dialog: "8px"
  dot: "9999px"
spacing:
  "2": "2px"
  "4": "4px"
  "6": "6px"
  "8": "8px"
  "12": "12px"
  "16": "16px"
components:
  button-primary:
    backgroundColor: "{colors.ivrea-green}"
    textColor: "{colors.band-ink}"
    rounded: "{rounded.control}"
  button-primary-hover:
    backgroundColor: "{colors.ivrea-green-hover}"
  button-primary-active:
    backgroundColor: "{colors.ivrea-green-pressed}"
  button-run:
    backgroundColor: "{colors.savoy-blue}"
    textColor: "{colors.savoy-ink}"
    rounded: "{rounded.control}"
  button-run-hover:
    backgroundColor: "{colors.savoy-blue-hover}"
  button-run-active:
    backgroundColor: "{colors.savoy-blue-pressed}"
  button-band:
    backgroundColor: "{colors.ivrea-green}"
    textColor: "{colors.band-ink}"
    rounded: "{rounded.control}"
  button-band-hover:
    backgroundColor: "{colors.ivrea-green-hover}"
  button-band-disabled:
    textColor: "{colors.band-ink-disabled}"
  tool-band:
    backgroundColor: "{colors.ivrea-green}"
    textColor: "{colors.band-ink}"
    height: "34px"
    padding: "0 8px"
  title-strip:
    backgroundColor: "{colors.graphite-panel}"
    textColor: "{colors.ink}"
    typography: "{typography.title}"
    height: "36px"
    padding: "0 12px"
  result-tab-strip:
    textColor: "{colors.ink-muted}"
    typography: "{typography.caption}"
    height: "28px"
    padding: "0 12px"
  status-bar:
    backgroundColor: "{colors.graphite-panel}"
    textColor: "{colors.ink-muted}"
    typography: "{typography.caption}"
  tree-row:
    textColor: "{colors.ink}"
    typography: "{typography.body}"
    padding: "2px 0 2px 6px"
  status-dot:
    rounded: "{rounded.dot}"
    size: "6px"
---

# Design System: Savoia DB

## Overview

**Creative North Star: "Olivetti, Ivrea"**

Italian computing modernism from Savoy Piedmont, set inside a working IDE. The ground is graphite with a faint green cast; flat, solid fields of Ivrea green name each zone of work, the way an Olivetti machine named its functions with color rather than ornament. The layout is the familiar DataGrip arrangement, so the developer always knows where things are; the color tells them which zone they are in.

The mood is quiet. Density is IDE-grade, but the chrome is a few flat planes and hairline rules, nothing more. One Savoy-blue control (Run) is the window's only accent. Status speaks in its own hues (teal, vermilion-orange, saffron, violet) so it is never mistaken for brand, and red is held back for danger alone.

Confirmed rejections: DBeaver/Eclipse clutter, Electron SaaS gloss, and neon terminal cosplay.

**Key Characteristics:**
- Graphite ground with a green cast; docked surfaces flat, separated by darker seams and hairline rules.
- Solid Ivrea-green tool bands (34px) carrying paper-white ink, aligned across the panel seam.
- One filled Savoy-blue control: Run.
- A lifted green (Ivrea Line) draws every focus and active line.
- Status hues of their own; red only ever means danger.
- System sans for the shell, monospace for SQL and data.

## Colors

Deep brand hues on a green-cast graphite, with bright status hues that belong to neither brand color.

### Primary
- **Ivrea Green** (`ivrea-green`): the primary brand color. Fills the explorer and console tool bands, primary buttons (such as "New data source"), and the title-bar mark tile. Hover lifts to `ivrea-green-hover`, press sinks to `ivrea-green-pressed`.
- **Ivrea Line** (`ivrea-line`): the same green lifted for strokes on graphite. Focus ring, text caret, the active result-grid cell border, and the active result tab underline.
- **Blueprint** (`blueprint`): the ER diagram's board, a blueprint drawn in deep Ivrea green rather than blue (the Run-Only Blue Rule). A `band-ink` grid rules it: minor lines every 20 diagram units at 7% opacity, major every 100 at 16%. Relationship lines use `band-ink-muted`.

### Secondary
- **Azzurro Savoia** (`savoy-blue`): the House of Savoy's blue, with white ink (`savoy-ink`). It fills the Run button and nothing else. Hover `savoy-blue-hover`, press `savoy-blue-pressed`.

### Tertiary (status)
- **Status Teal** (`status-teal`): success, such as the connected dot in the explorer tree.
- **Status Vermilion** (`status-vermilion`): danger and failure: the failed-connection dot, storage errors.
- **Status Saffron** (`status-saffron`): warning.
- **Status Violet** (`status-violet`): info, including the milestone ("arrives in M2") notices. Info sits on violet so it cannot read as the blue accent.
- Status fills carry graphite-canvas ink when text sits on them.

### Neutral
- **Graphite Canvas** (`graphite-canvas`): window background, editor, tabs, result grid body.
- **Graphite Panel** (`graphite-panel`): sidebar, title bar, status bar, table header.
- **Graphite Seam** (`graphite-seam`): the darker border between docked panels and bars.
- **Graphite Rule** (`graphite-rule`): hairline borders, input strokes, grid row rules.
- **Graphite Hover** (`graphite-hover`) and **Row Hover** (`graphite-row-hover`): list and grid hover.
- **Selection** (`selection`): a muted green fill for selected text, list rows and grid rows.
- **Ink** (`ink`), **Header Ink** (`ink-header`), **Muted Ink** (`ink-muted`): body text, grid column headers, secondary text and empty states.
- **Band Ink** (`band-ink`), **Band Ink Muted** (`band-ink-muted`), **Band Ink Disabled** (`band-ink-disabled`), **Band Rule** (`band-rule`): paper-white text, secondary text, disabled controls and hairline separators on an Ivrea band.

### Connection tags
User-chosen connection colors (`tag-red` through `tag-pink`) mark which database you are touching. They tint the data-source icon in the explorer, the console tab icon, and the connection form swatch. They are data, not brand: never reuse a tag value for chrome.

### Named Rules
**The Run-Only Blue Rule.** Savoy blue fills Run and nothing else in the window. The title-bar mark omits the app icon's blue dot to keep it that way.

**The Red Means Danger Rule.** Brand surfaces never use red. Errors and failures use vermilion-orange, and no status hue reuses Ivrea green or Savoy blue.

**The Lifted Line Rule.** Deep Ivrea green is a fill, never a 1px stroke. Any green line on graphite uses Ivrea Line.

**The Band Ink Rule.** Anything on a band uses band ink. Disabled controls on a band take Band Ink Disabled, never the toolkit's graphite-tuned disabled ink, which vanishes on green.

## Typography

**Body Font:** the platform system UI font (GPUI `.SystemUIFont`), 16px base.
**Mono Font:** the platform's generic monospace.

**Character:** neutral system sans that reads as native on every OS, with monospace held for code and data so the two registers never blur.

### Hierarchy
- **Title** (600, 14px): panel titles ("Database Explorer"), the app name in the title bar.
- **Body** (400, 14px): tree rows, toolbar labels, empty-state lines, form text.
- **Label** (500, 12px): form field labels.
- **Caption** (400, 12px): status bar, result tab strip, row counts, server versions, the schema chip on the console band.
- **Data Mono** (400, 14px): result grid cells; numeric columns and row numbers right-aligned, row numbers in muted ink.
- **Editor Mono** (400, 13px): the SQL editor.

### Named Rules
**The Two Registers Rule.** Sans for the shell, monospace for SQL and data. Never set chrome in mono or data in sans.

## Layout

A fixed DataGrip-style frame: title bar, a resizable Database Explorer (320px, 220 to 560px) on the left, console tabs with a SQL editor over a resizable result area (300px, from 120px) in the center, and a status bar.

Both panels share one register. Row one is a 36px graphite title strip (explorer title on the left, console tabs on the right, at one height). Row two is a 34px Ivrea-green tool band across both panels, broken only by the panel seam. Below sit the tree and the editor; the result area opens with a 28px tab strip.

Spacing steps are 2, 4, 6, 8, 12 and 16px. Bands pad 8px horizontally with 2px between tools; title and tab strips pad 12px. Band separators are 1 by 16px Band Rule hairlines with 4px margins. Tree rows indent from 6px in 14px steps per depth, with 2px vertical padding and 6px between chevron, icon, label and meta.

## Elevation & Depth

Docked surfaces are flat. Depth comes from tonal steps (canvas, panel, seam) and hairline rules. Only overlays (dialogs, menus, popovers, notifications) lift, using GPUI Kit's default shadow. Band and Run buttons have their shadow switched off.

### Named Rules
**The Docked-Is-Flat Rule.** If it is docked into the window, it has no shadow. Only floating surfaces lift.

## Shapes

Bars, bands, strips and panels are square. Controls round to 6px, dialogs to 8px. The title-bar mark is a 16px tile at 4px. Status dots and connection swatches are full circles. The app icon is a paper-white database cylinder on an Ivrea-green tile, with a Savoy-blue dot.

### Named Rules
**The Square Bars Rule.** Structural bars never round; radius belongs to the controls placed on them.

## Components

### Buttons
Flat and quiet. Color does the work; there is no gloss.
- **Shape:** gently rounded (6px), small size in bars.
- **Primary:** Ivrea Green fill with Band Ink, for the main action on a graphite surface ("New data source").
- **Run:** a filled Savoy-blue button with white ink and a play icon, first on the console band. It is the only one of its kind.
- **Band tool:** a ghost-like button that shares the band's green, with Band Ink icons or labels. Hover lifts to the hover green, press sinks to the pressed green. Disabled uses Band Ink Disabled.
- **Focus:** Ivrea Line ring.

### Tool Bands
- 34px solid Ivrea Green with Band Ink. Tools grouped by 1px Band Rule separators. Trailing context (such as the current schema) sits at the right in Band Ink Muted caption with a small icon.

### Inputs / Fields
- **Style:** Graphite Rule stroke on graphite, 6px radius, 12px medium label above.
- **Focus:** Ivrea Line ring and caret.

### Navigation
- **Console tabs:** underline tab bar on graphite canvas. Active tab in Ink, inactive in Muted Ink. The tab icon takes the connection's tag color.
- **Explorer tree:** chevrons and icons in muted ink. Data-source icons take their tag color. A 6px teal dot marks connected, a 6px vermilion dot marks failed, and counts and versions follow in muted caption.
- **Status bar:** graphite panel with a muted caption: connection icon and state on the left, "SQL · UTF-8" on the right.

### Result Grid
- Borderless data table on graphite canvas. Header in Graphite Panel with Header Ink. Rows separated by Graphite Rule, hover in Row Hover, selection in Selection, active cell outlined in Ivrea Line. A fixed 44px row-number column on the left.
- **Empty state:** one muted body line ("Run a query (⌘↩) to see results.").

### Context Menus
- **Table and view menu** (`table_menu.rs`): an identity header first, then short groups with the most used action nearest the pointer and destructive ones last. The header shows the name in Title weight, its kind in a muted caption, and a caption line with the path (`db › schema`), the connection's tag-colored dot and name, and "read-only" in saffron when it applies.
- **Rows:** a muted 16px icon, the label in Ink, and an optional right-aligned muted caption hint, such as "first 200 rows", the qualified name, or a milestone tag ("M2", "M4") on disabled future actions. Future actions stay visible and disabled rather than hidden.
- **Danger:** Truncate and Drop use vermilion ink for both icon and label, and are disabled with a "read-only" hint on read-only connections. They never run from the menu: a confirm dialog shows the connection and the exact statement in Data Mono, with a vermilion Danger OK button.
- Menu actions that produce SQL add it after the console's text and select it, so a run takes just that statement and the user's own SQL stays.

### SQL Editor
- Editor Mono on Graphite Canvas. The syntax palette overrides GPUI Kit's default dark highlight theme in `theme.rs` (`SYNTAX`) so no token is blue, following the Run-Only Blue Rule.
- **Keywords** in a lifted Ivrea green (#6DBE95). **Strings** in sand (#D8BC8C), moved off green so they stay distinct from keywords. **Numbers, constants and booleans** in dusty rose (#D69AAE). **Types** in lilac (#C3A9D9). **Functions** in pale sage (#B9CFC2). Comments stay the toolkit's muted grey.

## Do's and Don'ts

### Do:
- **Do** take every color from `theme.rs`: the GPUI Kit slots set in `edit`, or the brand constants.
- **Do** name a working zone with a solid 34px Ivrea-green band, aligned with its neighbor across the panel seam.
- **Do** draw every green line (focus, caret, active cell, active tab) in Ivrea Line.
- **Do** use violet for info, teal for success, saffron for warning, vermilion-orange for errors.
- **Do** carry the connection's tag color onto its explorer icon and console tab.
- **Do** keep docked surfaces flat and let only overlays cast a shadow.

### Don't:
- **Don't** use Savoy blue on anything but Run, and don't add a second blue control.
- **Don't** use red for brand or decoration; red only ever means danger.
- **Don't** use deep Ivrea green as a 1px stroke on graphite.
- **Don't** put toolkit-default disabled ink on a green band.
- **Don't** drift toward DBeaver/Eclipse clutter, Electron SaaS gloss, or neon terminal cosplay.
