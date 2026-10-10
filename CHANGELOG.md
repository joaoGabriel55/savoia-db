# savoia-studio

## 0.1.1

### Patch Changes

- [#12](https://github.com/joaoGabriel55/savoia-studio/pull/12) [`454a8c4`](https://github.com/joaoGabriel55/savoia-studio/commit/454a8c41401ca6b64a74625eb78db62a76578063) Thanks [@joaoGabriel55](https://github.com/joaoGabriel55)! - Linux packages are smaller and install on minimal systems: the `.deb` and `.rpm` drop from 21.6 MB and 16.3 MB to 14.7 MB each, and both now declare the libraries Savoia needs (xcb, xkbcommon, fontconfig, Vulkan, Wayland).

## 0.1.0

The first release of Savoia Studio, a native database client for PostgreSQL and MySQL.

- **Explore:** a lazy schema tree with counts, ER diagrams, table structure and DDL.
- **Query:** console tabs, run the statement at the caret or the whole script, schema-aware completion with join suggestions, streamed results with cancel, multiple result sets, sort/filter/copy/export, and query history.
- **Data view:** browse, filter and sort tables without SQL; add columns through foreign keys; summaries; edits held as pending changes and committed as previewed SQL in one transaction; read-only mode.
- **Dump and import:** uses your installed `pg_dump`/`psql`/`mysqldump`/`mysql` or a built-in engine; SQL scripts and CSV import; works through SSH tunnels.
- **Connections:** SSL modes, SSH tunnels with known_hosts checks, saved passwords in a user-only file.
- **Polish:** command palette (⌘⇧P), keyboard shortcuts, a Settings screen, light and dark themes that can follow the OS, signed auto-updates from GitHub Releases, and opt-in crash reports you review before sending.
