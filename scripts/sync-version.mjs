#!/usr/bin/env node
// Changesets owns the version (package.json). After `changeset version`
// bumps it, this copies it into the Rust workspace so the app, the
// installers and the release tag all agree.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const { version } = JSON.parse(readFileSync("package.json", "utf8"));
const cargo = readFileSync("Cargo.toml", "utf8");
const section = /(\[workspace\.package\][^[]*?\nversion\s*=\s*")([^"]+)(")/;
if (!section.test(cargo)) {
  throw new Error("no version under [workspace.package] in Cargo.toml");
}
writeFileSync("Cargo.toml", cargo.replace(section, `$1${version}$3`));
// Rewrites the workspace members' versions in Cargo.lock, nothing else.
execFileSync("cargo", ["update", "--workspace"], { stdio: "inherit" });
console.log(`Cargo workspace set to ${version}`);
