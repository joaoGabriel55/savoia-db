# Brand Savoia with Olivetti-style green fields and a red Run action

## Status

Superseded by [Use Savoy blue, not red, as the brand accent](./202610091125-use-savoy-blue-not-red-as-the-brand-accent.md)

## Context

[Follow a DataGrip-style IDE layout with Savoia branding](./202610091043-follow-a-datagrip-style-ide-layout-with-savoia-branding.md) set the layout and made amber `#E8B84A` the brand accent on a neutral graphite palette. The result looked like JetBrains with an amber caret, with nothing tying it to the name Savoia.

On 2026-10-09 the user asked for a more distinctive identity drawn from Savoia's Italian roots: green as the primary color, red as the accent, and a quiet mood. Before the change, the amber look was recorded in `DESIGN.md` as a baseline.

Two constraints shaped the choice:

- **Green and red already mean something in a database client.** Green means run, connected and success. Red means error, failed, and is the usual tag for production. The user chose deep brand hues, with status colors moved to their own hues so brand never reads as status.
- **The layout is not up for change.** The explorer, console tabs, editor and result grid stay where power users expect them. Only the visual language changes.

Directions considered, through the Impeccable direction round (seed `c4a29ce4`):

- **Olivetti, Ivrea (chosen).** Olivetti's graphic program (Pintori, Nizzoli) and the Elea 9003, Italy's first computer, built in Piedmont, the Savoy heartland. Italian computing modernism: flat color fields name each zone of work.
- **Carta Valori.** Italian security paper (lire banknotes, the marca da bollo stamp), with each connection shown as a perforated stamp. It was the assigned roll. Not chosen: it needs custom path drawing and risks currency cosplay.
- **Milano Centrale split-flap board.** Results flip in like departures. Not chosen: per-cell animation fights scanning large grids and the frame budget.
- **DataGrip, recolored.** Today's shell with amber swapped for green and red. Not chosen: it is indistinguishable from JetBrains.

## Decision

The DataGrip-style layout from the earlier ADR stays unchanged: title bar, Database Explorer on the left, console tabs with toolbar, editor and result grid, status bar. Connection colors still carry through the explorer, console tab and status bar. Dump and import stay first-class. Connection setup stays a focused form.

The branding changes:

- **Graphite ground** with a faint green cast, instead of JetBrains neutral grey.
- **Ivrea green** (deep bottle green, `#1F5A43`) is the primary color. It fills the explorer header and the console toolbar as solid bands, the signature of the system, and primary buttons. A lighter step of the same hue draws focus rings, the caret, selection and the active grid cell.
- **Valentine red** (`#C8323C`) is the accent, and Run is its only control. Red never decorates.
- **Status colors use their own hues:** teal for connected and success, vermilion-orange for errors and failures, saffron for warnings. Neither brand color is reused for status.
- **The app icon** swaps the amber dot for Valentine red.

Tokens live in `crates/savoia-app/src/theme.rs`, which stays the only source of color. `DESIGN.md` documents the system.

This supersedes the earlier ADR. Its layout decisions are carried over unchanged above.

## Consequences

- Savoia is recognizable at a glance by its green bands and single red Run, without changing where anything is.
- Red is now the brand accent, so users who tag production red see two reds. Run's red is a filled control and the production tag is an icon tint, but watch for confusion in testing.
- Status meaning moves off green and red. Error text and icons must use the error token, never the brand red.
- The light theme (M5) must redefine the same tokens, including the green bands.
- Rejected directions stay on record here so they aren't re-proposed blindly.
