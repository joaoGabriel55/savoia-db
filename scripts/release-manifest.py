#!/usr/bin/env python3
"""Writes latest.json, the manifest cargo-packager-updater reads, from the
signed installers of a release.

    release-manifest.py <version> <tag> <dir-with-assets> > latest.json

One entry per platform the updater can install: the macOS .app.tar.gz, the
Windows NSIS setup .exe and the Linux AppImage. Each needs its .sig beside it.
"""

import json
import pathlib
import sys
from datetime import datetime, timezone

REPO = "joaoGabriel55/savoia-studio"

# (suffix, updater target, format)
KINDS = [
    (".app.tar.gz", "macos", "app"),
    ("-setup.exe", "windows", "nsis"),
    (".appimage", "linux", "appimage"),
]
ARCHES = {"aarch64": "aarch64", "arm64": "aarch64", "x86_64": "x86_64", "x64": "x86_64", "amd64": "x86_64"}


def arch_of(name: str) -> str | None:
    for token, arch in ARCHES.items():
        if token in name:
            return arch
    return None


def main() -> None:
    version, tag, folder = sys.argv[1], sys.argv[2], pathlib.Path(sys.argv[3])
    platforms = {}
    for asset in sorted(folder.iterdir()):
        for suffix, target, fmt in KINDS:
            if not asset.name.lower().endswith(suffix.lower()):
                continue
            sig = asset.with_name(asset.name + ".sig")
            arch = arch_of(asset.name)
            if not sig.exists() or arch is None:
                sys.exit(f"{asset.name}: missing .sig or architecture in the name")
            platforms[f"{target}-{arch}"] = {
                "url": f"https://github.com/{REPO}/releases/download/{tag}/{asset.name}",
                "signature": sig.read_text().strip(),
                "format": fmt,
            }
    if not platforms:
        sys.exit("no updater assets found")
    json.dump(
        {
            "version": version,
            "notes": f"https://github.com/{REPO}/releases/tag/{tag}",
            "pub_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "platforms": platforms,
        },
        sys.stdout,
        indent=2,
    )


if __name__ == "__main__":
    main()
