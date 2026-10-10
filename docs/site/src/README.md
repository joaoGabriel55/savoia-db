# Savoia Studio

<p class="lead">Savoia Studio is a desktop database client for <strong>PostgreSQL</strong> and <strong>MySQL/MariaDB</strong>. It is written in Rust and draws its UI on the GPU, with no webview or JVM, so it starts fast and stays light on memory.</p>

<figure class="shot wide">
  <img class="only-dark" src="media/app-dark.webp" width="2000" height="1265" alt="The main window: the Database Explorer tree on the left, a SQL console with a standings query, and a result grid of 20 Serie A clubs.">
  <img class="only-light" src="media/app-light.webp" width="2000" height="1265" alt="The main window in the light theme: the Database Explorer tree on the left, a SQL console with a standings query, and a result grid of 20 Serie A clubs.">
  <figcaption>The main window: Database Explorer on the left, a console with its result grid on the right.</figcaption>
</figure>

<div class="free">
  <span class="zero" aria-hidden="true">0</span>
  <p><strong>Free. No trial, no license key, no paid tier, no account.</strong></p>
  <p class="sub">Open source under MIT or Apache-2.0. If it saves you time, you can <a href="https://ko-fi.com/O5I528IC3A">support it on Ko-fi</a>.</p>
</div>

## What you can do with it

- Explore schemas, tables, views and functions, with ER diagrams and DDL.
- Write SQL with schema-aware completion, run it statement by statement, and stream big results.
- Browse, filter, edit and join table data without writing SQL. Edits are reviewed as SQL before they commit.
- Dump and import whole schemas or single tables, with your installed client tools or the built-in engine.
- Connect through SSH tunnels and over TLS.

## Coming soon

Savoia speaks PostgreSQL and MySQL today. Support for more databases is on the way, free like everything else:

| Database | Status |
| --- | --- |
| <span class="engine-logo engine-postgresql" aria-hidden="true"></span>PostgreSQL | Supported |
| <span class="engine-logo engine-mysql" aria-hidden="true"></span>MySQL | Supported |
| <span class="engine-logo engine-mariadb" aria-hidden="true"></span>MariaDB | Works through a MySQL connection today; dedicated support coming |
| <span class="engine-logo engine-sqlite" aria-hidden="true"></span>SQLite | Coming soon |
| <span class="engine-logo engine-sqlserver" aria-hidden="true"></span>SQL Server | Coming soon |
| <span class="engine-logo engine-mongodb" aria-hidden="true"></span>MongoDB | Coming soon |

Want one sooner, or one that isn't listed? [Say so on GitHub](https://github.com/joaoGabriel55/savoia-studio/issues).

## Start here

<ul class="paths">
  <li><a href="install.html"><b>Install</b><span>Download for macOS, Windows or Linux.</span></a></li>
  <li><a href="connect.html"><b>Connect</b><span>Add a data source in one paste.</span></a></li>
  <li><a href="console.html"><b>Write SQL</b><span>Completion, running, results.</span></a></li>
  <li><a href="data-view.html"><b>Edit data</b><span>Change rows with a SQL preview.</span></a></li>
</ul>

The screenshots and recordings in these pages are of the real app, connected to a local PostgreSQL 17 with the Serie A sample database from the repository (`samples/`).
