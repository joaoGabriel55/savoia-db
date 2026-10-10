//! Auto-update from GitHub Releases through `cargo-packager-updater`. The
//! release workflow publishes `latest.json` with a minisign signature per
//! installer; the public key is baked in at build time. A build without
//! one (every dev build) never checks.

use cargo_packager_updater::semver::Version;
use cargo_packager_updater::url::Url;
use cargo_packager_updater::{Config, Update, UpdaterBuilder};

/// Set by the release workflow from the repository's updater key.
const PUBKEY: Option<&str> = option_env!("SAVOIA_UPDATER_PUBKEY");

/// The manifest of the latest release. `SAVOIA_UPDATER_ENDPOINT` replaces it
/// at build time, for testing an update end to end against a local server.
const MANIFEST: &str = match option_env!("SAVOIA_UPDATER_ENDPOINT") {
    Some(url) => url,
    None => "https://github.com/joaoGabriel55/savoia-studio/releases/latest/download/latest.json",
};

pub fn enabled() -> bool {
    PUBKEY.is_some_and(|key| !key.is_empty())
}

pub enum Check {
    /// This build can't update itself.
    Disabled,
    UpToDate,
    Available(Box<Update>),
}

/// Asks the manifest for a newer version. Blocks on the network; call it
/// off the UI thread.
pub fn check() -> Result<Check, String> {
    let Some(pubkey) = PUBKEY.filter(|key| !key.is_empty()) else {
        return Ok(Check::Disabled);
    };
    let current = Version::parse(env!("CARGO_PKG_VERSION")).map_err(|e| e.to_string())?;
    let config = Config {
        endpoints: vec![Url::parse(MANIFEST).map_err(|e| e.to_string())?],
        pubkey: pubkey.to_owned(),
        windows: None,
    };
    let update = UpdaterBuilder::new(current, config)
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .and_then(|updater| updater.check())
        .map_err(|e| e.to_string())?;
    Ok(match update {
        Some(update) => Check::Available(Box::new(update)),
        None => Check::UpToDate,
    })
}

/// Downloads, verifies the signature and installs. The caller restarts.
pub fn install(update: &Update) -> Result<(), String> {
    update.download_and_install().map_err(|e| e.to_string())
}
