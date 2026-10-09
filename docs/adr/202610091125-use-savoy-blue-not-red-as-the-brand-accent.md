# Use Savoy blue, not red, as the brand accent

## Status

Accepted

## Context

[Brand Savoia with Olivetti-style green fields and a red Run action](./202610091120-brand-savoia-with-olivetti-style-green-fields-and-a-red-run-action.md) made Valentine red (`#C8323C`) the accent, with Run as its only control. That ADR already flagged the risk: in a database client, red means danger, delete and production.

Once red was built into the app on 2026-10-09, the user rejected it for that reason and asked for white or blue instead.

Options considered:

- **Azzurro Savoia (chosen).** The House of Savoy's own blue, still worn by Italian national teams. It is the most authentic Savoy reference available and reads as a distinct accent against the Ivrea-green bands.
- **White.** A white Run on the green band reads cleanly, but white is already the color of all text on bands and graphite. It would not register as the one accent, and it would give hover and focus no hue to work with.

## Decision

Everything in the superseded ADR still holds except the accent:

- The DataGrip-style layout is unchanged.
- Connection colors still carry through the explorer, console tab and status bar.
- Dump and import stay first-class, and connection setup stays a focused form.
- Graphite ground, with Ivrea-green (`#1F5A43`) bands on the explorer header and console toolbar.
- Ivrea green fills primary buttons, and its lighter step (`#5FB088`) draws focus, caret, selection and the active cell.

The accent changes:

- **Savoy blue (`#1F64B4`, white text)** is the accent. Run is its only control, and blue never decorates.
- **Red is reserved for danger.** Brand surfaces never use it. Errors and failures use vermilion-orange.
- **Status hues:** teal for success, vermilion-orange for errors, saffron for warnings, and violet for info, which moves off blue so it can't be mistaken for the accent.
- **The app icon** carries a Savoy-blue dot.

## Consequences

- Run no longer looks like a destructive action, and a red production connection tag no longer competes with a red brand color.
- Blue is now taken by brand. Future info-level UI must use the violet info token, not a blue.
- The light theme (M5) must redefine these tokens too.
