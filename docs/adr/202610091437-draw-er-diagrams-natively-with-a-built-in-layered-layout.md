# Draw ER diagrams natively with a built-in layered layout

## Status

Proposed

## Context

Users want to see the entity-relationship diagram of a schema: its tables as boxes with their columns, and lines for the foreign keys between them, like DataGrip's and DBeaver's diagrams.

[Use GPUI Kit for a native, GPU-rendered UI](./202610091042-use-gpui-kit-for-a-native-gpu-rendered-ui.md) already says that ER diagrams are missing from the widget set and must be built on GPUI primitives. Two things are needed: a way to draw, and a way to place the boxes.

What `gpui-kit` 0.7.1 (GPUI 0.2.2) provides, checked on 2026-10-09:

- `div()` with absolute positioning renders the boxes, text and theme colors like every other view.
- `canvas()` with `PathBuilder` and `Window::paint_path` draws arbitrary strokes for the connecting lines.
- Mouse down, move, up and scroll-wheel handlers on any element, enough for panning and dragging boxes.

Options considered for placing the boxes:

- **Graphviz `dot`**, as an external binary or through bindings. It gives the best layouts, but it is a native dependency that users must install or that we must ship per OS, which the packaging plan (M5) avoids for anything but the dump tools.
- **A layout crate such as `layout-rs`.** Pure Rust, but it lays out to SVG-oriented geometry, is lightly maintained, and would be a second rendering model next to GPUI's.
- **A small built-in layered layout (chosen).** Tables are put in columns by foreign-key depth, so a referenced table sits to the left of the tables that point at it. Within a column, tables are ordered to keep each one near what it references. Tables without any foreign key go into a grid after the connected ones. It is a few hundred lines with no dependency, and it is unit-testable.

## Decision

**The ER diagram is a GPUI view in `savoia-app`. Tables are absolutely positioned `div`s, relationship lines are painted on a `canvas` beneath them, and box positions come from a built-in layered layout.**

- **Scope:** one schema at a time, opened from the schema's or a table's context menu ("Show diagram"). Opening it from a table highlights that table.
- **Data:** `describe_schema` from [Load schema objects and table details on demand](./202610091437-load-schema-objects-and-table-details-on-demand.md). The diagram doesn't query on its own.
- **Lines** join the foreign-key columns to the referenced table's key columns, with a crow's-foot end on the referencing side. Foreign keys to tables in other schemas are listed in the referencing box, not drawn.
- **Interaction:** drag the background to pan, drag a table's header to move it, and use the toolbar to reset the layout. Positions are not saved yet.
- **Where:** the diagram opens in its own closable tab next to the console.

## Consequences

- No new dependency, and the diagram uses the app's theme, fonts and colors.
- The layout is simpler than Graphviz: in dense schemas lines cross boxes. Dragging boxes is the escape hatch, and a better layout can replace the function without touching the view.
- Very large schemas (hundreds of tables) render every box. If that turns out slow, culling boxes outside the viewport is the next step.
- Follow-ups not decided here: zoom, saving positions per schema, exporting to PNG/SVG, and diagrams that span several schemas.
