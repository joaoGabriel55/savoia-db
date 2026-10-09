---
version: 1
slug: "crates-savoia-app-src-workspace-rs"
primary_target: "crates/savoia-app/src/workspace.rs"
related_targets: ["crates/savoia-app/src/explorer.rs","crates/savoia-app/src/console.rs","crates/savoia-app/src/results.rs","crates/savoia-app/src/theme.rs"]
---

# Workspace (main IDE window)

Scope: the main window and everything docked in it (title bar, Database Explorer, console tabs, console toolbar, SQL editor, result area, status bar), plus dialogs opened from it. Mode: Operate.

Audience and job: backend developers querying and fixing data across local, staging and prod all day. Constraints: the DataGrip layout is fixed (ADR 202610091125); colors only from `theme.rs`; status hues never reuse brand green or red.

## Direction contract

THESIS: Italian computing modernism, after Olivetti in Ivrea. Flat color fields name each zone of work. It refuses the category default of a grey IDE with one accent dot.

OWN-WORLD: Graphite ground with a green cast. Deep Ivrea-green bands carry paper-white text. One Savoy-blue (azzurro Savoia) control; red is reserved for danger. A lighter step of the green draws every focus line. Status colors are teal, vermilion-orange, saffron and violet. Square bars, 6px controls, hairline rules, system sans and mono.

STORY: The developer sees the familiar IDE and knows where everything is. The green bands tell them which zone they are in. Blue means Run and nothing else; red only ever means danger. The connection tag still says which database they are touching.

FIRST VIEWPORT: Graphite title bar with the app's mark (paper cylinder on an Ivrea-green tile). Both panels share one register. Row 1 is a graphite title strip: "Database Explorer" on the left, console tabs on the right, at one height. Row 2 is one Ivrea-green tool band across both panels, broken only by the seam: explorer tools on the left; on the right, paper-colored console tools with a filled Savoy-blue Run at the far left. Below that sit the tree and the editor, then the result area, whose empty state is one muted line. Status bar in graphite.

FORM: Olivetti, Ivrea (pick card, my rank 1). Seed c4a29ce4.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
