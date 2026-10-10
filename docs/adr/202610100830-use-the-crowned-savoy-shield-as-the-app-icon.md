# Use the crowned Savoy shield as the app icon

## Status

Accepted

## Context

[Use Savoy blue, not red, as the brand accent](./202610091125-use-savoy-blue-not-red-as-the-brand-accent.md) gave the app icon a database glyph with a Savoy-blue dot and kept red off every brand surface, because in a database client red means danger.

For the v0.1 release on 2026-10-10, the user asked for a logo inspired by the flag of the Kingdom of Italy: the green, white and red tricolore with the crowned Savoy shield (a white cross on red, with a blue border). Three directions were offered: the full tricolore, the shield on Ivrea green, and a shield with no red at all. The user chose the shield on Ivrea green.

## Decision

Everything in the superseded ADR still holds except the app icon:

- **The app icon** (`assets/app-icon.svg`, with PNG and `.icns` exports in `assets/icons/`) is the crowned Savoy shield on an Ivrea-green rounded square. The shield has a white cross on red with a Savoy-blue border, under a gold crown.
- **The wordmark** (`assets/logo.svg`, plus `logo-dark.svg` for dark backgrounds) sets the icon beside "Savoia Studio".
- **Red stays out of the UI.** The shield's red is heraldry inside the icon only. In-app chrome still never uses red, Run stays the window's only Savoy blue, and errors stay vermilion-orange. The title-bar mark keeps the database glyph on green.

Alternatives considered:

- **Full tricolore** behind the shield. This is the most faithful to the flag, but it puts a large red field on the brand mark, which the red-means-danger rule exists to avoid.
- **No red** (a white cross on Savoy blue). This fits the old ADR as it stands, but the shield then stops reading as the Savoy arms.

## Consequences

- The icon names the brand directly (Savoy, the House of Savoy) rather than a generic database glyph.
- Gold (`#D9A93B`) appears in the icon only. It is not a UI token.
- Small sizes lose the crown's detail. At 32 px the shield still reads.
