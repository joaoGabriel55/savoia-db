#!/usr/bin/env bash
# Recompresses a .deb's payload from gzip to xz. cargo-packager always
# writes data.tar.gz; xz brings Savoia's .deb from about 21.6 MB to about
# 14 MB (the 15 MB installer budget). dpkg reads data.tar.xz natively.
#
#   scripts/repack-deb.sh path/to/package.deb
set -euo pipefail

deb=$(realpath "$1")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

cd "$work"
ar x "$deb"
test -f data.tar.gz || { echo "no data.tar.gz in $deb" >&2; exit 1; }
gunzip data.tar.gz
xz -9e -T0 data.tar
# Member order matters to dpkg: debian-binary, control, data.
rm "$deb"
ar rc "$deb" debian-binary control.tar.* data.tar.xz
echo "repacked $(basename "$deb"): $(du -h "$deb" | cut -f1)"
