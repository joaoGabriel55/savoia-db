# Install

Download the installer for your system from the [latest release](https://github.com/joaoGabriel55/savoia-studio/releases/latest).

| System | File |
| --- | --- |
| macOS (Apple silicon) | `savoia-studio_<version>_aarch64.dmg` |
| macOS (Intel) | `savoia-studio_<version>_x86_64.dmg` |
| Windows | `savoia-studio_<version>_x64-setup.exe`, or the `.msi` |
| Debian, Ubuntu | `.deb` |
| Fedora, openSUSE | `.rpm` |
| Any Linux | `.AppImage`. Make it executable and run it. |

## Updates

Savoia checks GitHub Releases for a new version when it starts. When it finds one, a notification offers **Install and restart**; nothing installs until you click it. Each download is verified against a signature before it installs. You can turn the startup check off in **Settings › General › Updates**, and check by hand from the command palette (**Check for updates**).

The `.deb` and `.rpm` packages don't update themselves. Install the new package over the old one.

## First launch warnings

Until the installers are code-signed, macOS and Windows warn the first time you open the app:

- **macOS:** right-click Savoia Studio in Applications, choose **Open**, then **Open** again.
- **Windows:** on the SmartScreen prompt, choose **More info**, then **Run anyway**.

## Build from source

You need Rust stable:

```sh
git clone https://github.com/joaoGabriel55/savoia-studio
cd savoia-studio
cargo run -p savoia-app --release
```

On Linux, install the system libraries first: `libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev libfontconfig-dev libx11-xcb-dev libssl-dev`.
