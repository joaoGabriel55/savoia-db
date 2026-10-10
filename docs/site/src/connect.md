# Connect to a database

Click **+** in the Database Explorer to add a data source. Pick PostgreSQL or MySQL and fill in the host, port, user and database. You can also paste a URL such as `postgres://user@host:5432/db` and click **Import** to fill the form. **Test** tries the connection, and **Save & Connect** saves it and connects.

<figure class="shot">
  <video poster="media/connect-url.webp" width="1200" height="1078" autoplay muted loop playsinline preload="metadata" aria-label="Recording: a postgres URL is pasted into the New data source form, Import fills host, port, user and database, a color is picked, and Test reports Connected to PostgreSQL 17.">
<source src="media/connect-url.mp4" type="video/mp4">
</video>
  <figcaption>Paste a URL, click <strong>Import</strong>, then <strong>Test</strong>.</figcaption>
</figure>

- **Password.** Tick *Save passwords* to store them in a file only your user can read, in the app's data folder. Otherwise Savoia asks for the password when you connect and forgets it when you quit.
- **SSL mode.** *Disable*, *Prefer*, *Require* or *Verify full*. Only *Verify full* also checks the server's certificate and host name against your system's trust store.
- **SSH tunnel.** Enter the bastion host, user and key or password. The first connection to an unknown host shows its key fingerprint. Trust it only if it matches the server's key; it is then added to `~/.ssh/known_hosts`.
- **Read-only.** Read-only sessions refuse writes on the server side, and the data view hides its edit controls.
- **Color.** A color tags the connection's tabs, so production stands out from local.

Double-click a data source to connect. Expand it to load its schemas; objects load as you open them.
