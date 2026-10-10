---
name: Savoia Studio
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
  web-paper: "#F7F6F2"
  web-paper-panel: "#EEEDE7"
  web-paper-seam: "#D6D4CB"
  web-paper-rule: "#E0DED6"
  web-paper-ink: "#1C201D"
  web-paper-muted: "#646B66"
  web-paper-line: "#1B7650"
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
  web-price:
    fontFamily: "Archivo, system-ui, sans-serif"
    fontSize: "clamp(11rem, min(40vw, 62vh), 36rem)"
    fontWeight: 800
    lineHeight: 0.74
    letterSpacing: "-0.04em"
    fontVariation: "'wdth' 125"
  web-display:
    fontFamily: "Archivo, system-ui, sans-serif"
    fontSize: "clamp(2rem, 3.6vw, 3.4rem)"
    fontWeight: 700
    lineHeight: 1.04
    letterSpacing: "-0.025em"
    fontVariation: "'wdth' 108"
  web-headline:
    fontFamily: "Archivo, system-ui, sans-serif"
    fontSize: "clamp(2rem, 4.4vw, 3.6rem)"
    fontWeight: 700
    lineHeight: 1.02
    letterSpacing: "-0.03em"
    fontVariation: "'wdth' 112"
  web-title:
    fontFamily: "Archivo, system-ui, sans-serif"
    fontSize: "clamp(1.5rem, 2.2vw, 2rem)"
    fontWeight: 700
    lineHeight: 1.1
    letterSpacing: "-0.02em"
    fontVariation: "'wdth' 108"
  web-figure:
    fontFamily: "Archivo, system-ui, sans-serif"
    fontSize: "clamp(1.6rem, 3vw, 2.4rem)"
    fontWeight: 700
    lineHeight: 1
    letterSpacing: "-0.02em"
    fontVariation: "'wdth' 112"
  web-wordmark:
    fontFamily: "Archivo, system-ui, sans-serif"
    fontSize: "17px"
    fontWeight: 700
    lineHeight: 1
    letterSpacing: "-0.01em"
    fontVariation: "'wdth' 112"
  web-docs-h1:
    fontFamily: "Archivo, system-ui, sans-serif"
    fontSize: "clamp(32px, 5vw, 46px)"
    fontWeight: 700
    lineHeight: 1.12
    letterSpacing: "-0.03em"
    fontVariation: "'wdth' 112"
  web-docs-h2:
    fontFamily: "Archivo, system-ui, sans-serif"
    fontSize: "26px"
    fontWeight: 700
    lineHeight: 1.12
    letterSpacing: "-0.02em"
    fontVariation: "'wdth' 108"
  web-docs-h3:
    fontFamily: "Archivo, system-ui, sans-serif"
    fontSize: "20px"
    fontWeight: 650
    lineHeight: 1.12
    letterSpacing: "-0.02em"
    fontVariation: "'wdth' 104"
  web-body:
    fontFamily: "system-ui, -apple-system, Segoe UI, Roboto, Helvetica Neue, sans-serif"
    fontSize: "17px"
    fontWeight: 400
    lineHeight: 1.6
  web-docs-body:
    fontFamily: "system-ui, -apple-system, Segoe UI, Roboto, Helvetica Neue, sans-serif"
    fontSize: "17px"
    fontWeight: 400
    lineHeight: 1.65
  web-button:
    fontFamily: "system-ui, -apple-system, Segoe UI, Roboto, Helvetica Neue, sans-serif"
    fontSize: "16px"
    fontWeight: 600
    lineHeight: 1
  web-mono:
    fontFamily: "ui-monospace, SF Mono, Menlo, Consolas, Liberation Mono, monospace"
    fontSize: "0.88em"
    fontWeight: 400
rounded:
  mark: "4px"
  control: "6px"
  dialog: "8px"
  web-callout: "8px"
  web-capture: "10px"
  dot: "9999px"
spacing:
  "2": "2px"
  "4": "4px"
  "6": "6px"
  "8": "8px"
  "12": "12px"
  "16": "16px"
  web-gutter: "clamp(16px, 4vw, 56px)"
  web-section: "clamp(80px, 12vw, 160px)"
  web-max: "1240px"
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
  web-button-download:
    backgroundColor: "{colors.savoy-blue}"
    textColor: "{colors.savoy-ink}"
    typography: "{typography.web-button}"
    rounded: "{rounded.control}"
    height: "48px"
    padding: "0 20px"
  web-button-download-hover:
    backgroundColor: "{colors.savoy-blue-hover}"
  web-button-download-active:
    backgroundColor: "{colors.savoy-blue-pressed}"
  web-button-kofi:
    backgroundColor: "{colors.band-ink}"
    textColor: "{colors.web-paper-ink}"
    typography: "{typography.web-button}"
    rounded: "{rounded.control}"
    height: "48px"
    padding: "0 20px"
  web-button-kofi-on-paper:
    backgroundColor: "{colors.ivrea-green}"
    textColor: "{colors.band-ink}"
    typography: "{typography.web-button}"
    rounded: "{rounded.control}"
    height: "48px"
  web-button-kofi-on-paper-hover:
    backgroundColor: "{colors.ivrea-green-hover}"
  web-band-nav:
    backgroundColor: "{colors.ivrea-green}"
    textColor: "{colors.band-ink}"
    height: "56px"
  web-docs-menu-band:
    backgroundColor: "{colors.ivrea-green}"
    textColor: "{colors.band-ink}"
    height: "52px"
  web-paper-plate:
    backgroundColor: "{colors.web-paper}"
    textColor: "{colors.web-paper-ink}"
    padding: "28px"
  web-docs-callout:
    rounded: "{rounded.web-callout}"
    padding: "12px 18px"
  web-docs-capture:
    backgroundColor: "{colors.web-paper-panel}"
    rounded: "{rounded.web-capture}"
  web-footer-band:
    backgroundColor: "{colors.ivrea-green}"
    textColor: "{colors.band-ink}"
    height: "72px"
---

# Design System: Savoia Studio

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

### Light appearance (Olivetti paper)
The light theme swaps graphite for warm paper and keeps the brand: Ivrea bands, band ink, Savoy-blue Run and the blueprint board look the same in both. `theme.rs` holds the two palettes (`DARK`, `LIGHT`), and Settings, or the OS under *Match system*, picks one.

| Role | Dark | Light |
| --- | --- | --- |
| Canvas | #161A17 | #F7F6F2 |
| Panel | #1D211E | #EEEDE7 |
| Seam | #111412 | #D6D4CB |
| Rule | #272C28 | #E0DED6 |
| Hover / Row hover | #2A302C / #1F2420 | #E4E2DA / #F0EFE9 |
| Selection | #213A31 | #D3E6DC |
| Ink / Header / Muted | #E9E6DF / #B8BDB8 / #8D938F | #1C201D / #48504A / #646B66 |
| Line (focus, caret, active) | #5FB088 | #1B7650 |
| Teal / Vermilion / Saffron / Violet | #3DB9AE / #F2713C / #E5B54A / #9B8CE8 | #0A7870 / #B8441A / #836109 / #5E4DB8 |

On paper, the status hues and the line green are darkened to hold 4.5:1 on canvas and panel, and ink on a status fill turns white. Views take green lines from `theme.ring` rather than a constant, so they follow the appearance.

### Connection tags
User-chosen connection colors (`tag-red` through `tag-pink`) mark which database you are touching. They tint the data-source icon in the explorer, the console tab icon, and the connection form swatch. They are data, not brand: never reuse a tag value for chrome.

### Named Rules
**The Run-Only Blue Rule.** Savoy blue fills Run and nothing else in the window. The title-bar mark keeps a plain database glyph on green to keep it that way.

**The Red Means Danger Rule.** Brand surfaces never use red. Errors and failures use vermilion-orange, and no status hue reuses Ivrea green or Savoy blue. The one exception is heraldry: the app icon's Savoy shield is red with a white cross ([ADR](docs/adr/202610100830-use-the-crowned-savoy-shield-as-the-app-icon.md)), and that red never enters the UI.

**The Lifted Line Rule.** Deep Ivrea green is a fill, never a 1px stroke. Any green line uses the appearance's line green (`theme.ring`): Ivrea Line on graphite, its darker step on paper.

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

Bars, bands, strips and panels are square. Controls round to 6px, dialogs to 8px. The title-bar mark is a 16px tile at 4px. Status dots and connection swatches are full circles. The app icon is the crowned Savoy shield (white cross on red, Savoy-blue border, gold crown) on an Ivrea-green tile.

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
- Borderless data table on graphite canvas. Header in Graphite Panel with Header Ink. Rows separated by Graphite Rule, hover in Row Hover, selection in a translucent Ivrea Line wash (18%, close to Selection on the canvas; the toolkit paints it over the cell, so it must stay see-through), active cell outlined in Ivrea Line. A fixed 44px row-number column on the left.
- **Empty state:** one muted body line ("Run a query (⌘↩) to see results.").

### Context Menus
- **Table and view menu** (`table_menu.rs`): an identity header first, then short groups with the most used action nearest the pointer and destructive ones last. The header shows the name in Title weight, its kind in a muted caption, and a caption line with the path (`db › schema`), the connection's tag-colored dot and name, and "read-only" in saffron when it applies.
- **Rows:** a muted 16px icon, the label in Ink, and an optional right-aligned muted caption hint, such as "first 200 rows", the qualified name, or a milestone tag ("M2", "M4") on disabled future actions. Future actions stay visible and disabled rather than hidden.
- **Danger:** Truncate and Drop use vermilion ink for both icon and label, and are disabled with a "read-only" hint on read-only connections. They never run from the menu: a confirm dialog shows the connection and the exact statement in Data Mono, with a vermilion Danger OK button.
- Menu actions that produce SQL add it after the console's text and select it, so a run takes just that statement and the user's own SQL stays.

### SQL Editor
- Editor Mono on Graphite Canvas. The syntax palette overrides GPUI Kit's default dark highlight theme in `theme.rs` (`DARK_SYNTAX`) so no token is blue, following the Run-Only Blue Rule. The light theme has its own set (`LIGHT_SYNTAX`) with the same roles in darker inks.
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

---

## Web surfaces (site and docs)

The public site extends the same world to the web: the landing page (`docs/site/landing/index.html`, tokens inline on `:root`) and the mdBook docs (`docs/site/theme/savoia.css`, `savoia.js`). Everything above still holds for the app. This section adds what the web needs and names the one place it departs from the window. The `web-` tokens in the frontmatter belong to these surfaces only; the app never reads them.

### Overview
The landing opens on a drenched Ivrea field: a full-width green first viewport in band ink, the band nav across its top, a giant Archivo "0" as the price tag, and the real app window rising out of the field's bottom edge and cropped by it. Below it, every section sits on graphite canvas, ruled with hairlines rather than boxed. A footer band in Ivrea green closes the page. The docs keep a calm reading column on Olivetti paper or graphite, under an Ivrea menu band.

### Colors
The web uses the app's tokens for green, band ink, graphite and Savoy blue, and adds the light appearance's paper values as `web-paper` tokens (paper, panel, seam, rule, ink, muted, line). They match the app's light palette table.

- **Drenched Ivrea:** the hero and the footer are solid `ivrea-green` with `band-ink` text, `band-ink-muted` secondary text and `band-rule` hairlines, just like a tool band.
- **Graphite sections:** `graphite-canvas` ground. Body text in `ink-header`, emphasis in `ink`, captions in `ink-muted`, rules in `graphite-rule`, figure frames in `graphite-seam`. Text links and measured numbers take `ivrea-line`.
- **Paper plate:** a square `web-paper` panel set into a graphite section (the "receipt" support panel), with `web-paper-ink`, `web-paper-muted` and `web-paper-seam` row rules.
- **Docs mapping:** mdBook *light* is Olivetti paper (`web-paper` canvas, `web-paper-panel` sidebar, code and quotes, `web-paper-line` links and active item), and *coal* (relabelled "Dark") is graphite (`graphite-canvas`, `graphite-panel`, `ivrea-line` links). The rust, navy and ayu themes are hidden from the picker, and their classes fall back to the two looks. Warning borders take saffron in both.

**The Download-Only Blue Rule.** This is the web's exception to the Run-Only Blue Rule: on the site, Savoy blue fills the Download action and nothing else. It shows up twice on the landing, as the hero's OS-detected "Download for …" and as "Latest release". The docs carry no blue at all. Ko-fi is never blue: it takes a band-ink fill on green and an Ivrea fill on paper.

### Typography
**Display Font:** Archivo variable (self-hosted `archivo-wdth-wght.woff2`, weights 100 to 900, width 62 to 125%), falling back to system-ui.
**Body Font:** the system sans stack (system-ui, -apple-system, Segoe UI, Roboto, Helvetica Neue).
**Mono Font:** ui-monospace, SF Mono, Menlo, Consolas, Liberation Mono.

Archivo is always set wide: 125% for the price numeral, 112% for section heads, the wordmark and measured numbers, 104 to 108% for titles and the hero H1. Weights are 800 (the numeral), 700 (headings) and 650 (docs h3).

- **Price** (`web-price`): the single giant "0" in the hero. On narrow screens it scales with the column (about 1.05 times its width). The docs intro repeats it small (800, 64px) inside its free callout.
- **Display** (`web-display`): the landing H1, balanced, 18ch at most.
- **Headline** (`web-headline`): landing section heads.
- **Title** (`web-title`): landing step titles. The paper plate title uses 700, 22px at 112%.
- **Figure** (`web-figure`): measured values in the spec table, in Ivrea Line, with the unit in sans.
- **Docs headings** (`web-docs-h1/h2/h3`): h2 opens with a hairline rule above it.
- **Body** (`web-body`, `web-docs-body`): 17px reading text. Ledes go to 18px on the landing and 1.15 to 1.2em in the docs, with lines held to 46 to 70ch.
- **Mono** (`web-mono`): only for code, commands, file names, keycaps and the plate's amounts. Keycaps get a 1px rule with a 2px bottom edge and a 4px radius.

**The Wide Archivo Rule.** On the web, every display line is Archivo set wide. Reading text is the system sans, and mono marks only code, keys and amounts. Never set body copy in Archivo or headings in the system font.

### Layout
The landing runs on a centered column (`web-max`, with `web-gutter` on each side) and a 5:7 grid for the hero and section heads. The loop steps alternate 8:4 and 4:8. Sections are spaced `web-section` apart. The first section after the hero adds the height of the cropped app window, which hangs 30% of its width below the field. Below 960px every grid falls to one column. Below 640px the band keeps only its icons, and the tables restack as ruled grids.

The docs use an mdBook frame: a 52px menu band, a 280px sidebar that opens on an Ivrea "home" strip the same height as the band and closes with the Ko-fi support card, and a 760px reading column. Wide captures bleed 60px into the margins on each side from 1280px.

### Elevation & Depth
Web sections are flat, and the hero, sections, paper plate and bands cast no shadow. Only captures of the app lift, as if the window sat on the page.

- **Hero window** (`filter: drop-shadow(0 30px 60px rgba(8, 20, 14, 0.55)) drop-shadow(0 4px 12px rgba(8, 20, 14, 0.35))`): the app window rising from the green field, tinted toward the green.
- **Theme frame** (`filter: drop-shadow(0 18px 40px rgba(0, 0, 0, 0.45))`): the landing's dark and light screenshot switch on graphite.
- **Docs capture, paper** (`box-shadow: 0 12px 32px rgba(17, 20, 18, 0.16), 0 1px 3px rgba(17, 20, 18, 0.12)`).
- **Docs capture, graphite** (`box-shadow: 0 16px 40px rgba(0, 0, 0, 0.5), 0 1px 3px rgba(0, 0, 0, 0.4)`).

The landing's loop recordings sit in flat, square panel frames, with no shadow.

**The Only-Captures-Lift Rule.** This is the web form of Docked-Is-Flat. A shadow on the site means "this is the real app". Text, plates, cards and controls stay flat.

### Shapes
Bands, sections, the paper plate and the landing's recording frames are square, so the Square Bars Rule carries over. Controls round to 6px (`rounded.control`): buttons, band tools, the theme switch, the search field, sidebar items. Docs callouts and containers round to 8px (`web-callout`): code blocks, blockquotes, the free callout, the sidebar support card, chapter navigation. Docs captures round to 10px (`web-capture`). Keycaps, inline code and switch segments use 4px (`rounded.mark`).

### Components
- **Band nav:** a 56px green band (`web-band-nav`) with a bottom `band-rule` hairline. The wordmark is the 28px app icon (6px radius, `band-rule` ring) plus Archivo 700 at 112%. Tools are 36px, 6px-radius links in 15px medium sans with 18px line icons, and hover to `ivrea-green-hover`. A 1 by 16px `band-rule` separator sets Ko-fi apart.
- **Buttons:** 48px tall, 0 20px padding, 6px radius, 600 16px sans with a 20px icon. Each one moves 1px down when pressed. Download is Savoy blue (hover and press steps as in the app). Ko-fi on green is band ink with paper ink, hovers to white, and its cup icon is Ivrea. Ko-fi on paper is Ivrea with band ink. The ghost variant on green is an inset `band-rule` ring.
- **Struck ledger:** a list ruled in `band-rule` that names what you don't need, items in Archivo 600 17px on the left and a muted sans "none" on the right. A 2px band-ink strike draws across each item once on load, staggered by 140ms, and the items then settle to muted ink. With reduced motion the items are simply muted.
- **Recordings with captions:** the screen recordings are muted, looping, inline `<video>`s with a poster frame and a descriptive `aria-label`. They autoplay, and with reduced motion they stop and show controls. Narrow screens get a smaller cut through `media`. On the landing they sit in a square `graphite-panel` frame with a `graphite-seam` border and a 13px muted caption below a seam rule. In the docs they share the `figure.shot` frame with stills: 10px radius, seam border, capture shadow, and a 0.85em muted caption. Stills come in theme-matched pairs (only-light, only-dark).
- **Spec table:** a table ruled in `graphite-rule` with the measure in 600 sans, the value in `web-figure` Ivrea Line, and the method in muted 15px. A note under it gives the machine and links the benchmarks.
- **Ruled index:** feature rows with a 9.5rem bold term column and hairlines between rows.
- **Paper plate:** a square receipt in `web-paper`, padded 28px, with a 2px paper-ink rule above the total. Amounts are in mono with tabular figures. The full-width Ko-fi on paper closes it.
- **Docs menu band:** `#menu-bar` in Ivrea green with no bottom border, icons in `band-ink-muted` (band ink on hover, over `ivrea-green-hover`), the title in Archivo 700 at 112%, and a band-ink Ko-fi chip (32px, 6px radius).
- **Docs sidebar:** items 7 by 10px, 6px radius. The active item is `selection` (green-tinted) in 600 weight. The support card is an 8px-radius `seam`-bordered card on the page ground, with an Archivo title and a full-width Ivrea Ko-fi button.
- **Docs tables:** ruled, without zebra stripes. The header is 600 muted sans over a seam rule, and rows sit on hairlines.
- **Free callout (docs intro):** an 8px-radius Ivrea field with the small "0" price numeral and band-ink text.

### Do's and Don'ts
#### Do:
- **Do** open a web page on a drenched Ivrea field or band, and set the rest on graphite (or paper in the docs) ruled with hairlines.
- **Do** keep Savoy blue to the Download action alone on the site.
- **Do** set display lines in wide Archivo, reading text in the system sans, and mono only for code, keys and amounts.
- **Do** show the product with real captures and recordings, each with a caption, and let only those captures cast a shadow.
- **Do** map docs light to Olivetti paper and docs dark to graphite, with links in the appearance's line green.

#### Don't:
- **Don't** fill Ko-fi, links or any second control with Savoy blue, and don't add blue to the docs.
- **Don't** shadow text, plates, cards or bands, and don't round bands or sections.
- **Don't** offer mdBook's rust, navy or ayu looks.
