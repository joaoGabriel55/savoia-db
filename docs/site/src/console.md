# Write and run SQL

Each console tab is bound to one data source. Open one with **⌘T** (Ctrl+T on Windows and Linux) or the **+** on the tab bar.

- **⌘↩** runs the statement under the caret, or the selection if there is one.
- **⌘⇧↩** runs the whole script.
- Completion suggests schemas, tables and columns from the loaded catalog. After `JOIN … ON`, it suggests join conditions from foreign keys.
- A statement with several result sets shows one tab per result. Big results stream in and pause once the grid is full. Then you can scroll for more, **Load all**, **Skip rest**, or cancel.
- In the result grid, click a header to sort and use the filter box to hide rows. You can copy cells as TSV, CSV, JSON or `INSERT`, or export the whole result to CSV.
- **History** keeps every run with its time, duration and row count. Search it and reopen a query in the console.
