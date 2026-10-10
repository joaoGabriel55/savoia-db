# Privacy

Savoia Studio has no account and no telemetry.

**Network.** Savoia connects to the databases and SSH hosts you configure. The only other request is the update check at startup, which fetches `latest.json` from GitHub Releases. You can turn it off in **Settings › General › Updates**.

**Stored data.** Saved connections, query history and settings are kept in a SQLite file in the app's data folder. Saved passwords are kept in a separate file only your user can read:

| System | Folder |
| --- | --- |
| macOS | `~/Library/Application Support/savoia-db/` |
| Windows | `%APPDATA%\savoia-db\` |
| Linux | `~/.local/share/savoia-db/` |

**Crash reports.** When Savoia crashes, it writes a text report to `crashes/` in that folder. Nothing is sent. If you turn on **Settings › General › Privacy › Offer to report crashes**, the next start offers to open a GitHub issue with the report filled in. Read it first, since a panic message can include table names or values, then submit it or close the page. With the option off, Savoia keeps the last five reports and offers nothing.
