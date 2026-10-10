#!/usr/bin/env node
// Run on main once the version PR is merged and no changesets are pending:
// tags the version if it isn't tagged yet. In GitHub Actions it reports
// `tagged` and `tag` as step outputs.
import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync } from "node:fs";

const git = (...args) => execFileSync("git", args, { encoding: "utf8" }).trim();
const { version } = JSON.parse(readFileSync("package.json", "utf8"));
const tag = `v${version}`;

const exists = git("ls-remote", "--tags", "origin", `refs/tags/${tag}`) !== "";
if (exists) {
  console.log(`${tag} already exists; nothing to release.`);
} else {
  git("tag", "-a", tag, "-m", `Savoia Studio ${version}`);
  git("push", "origin", tag);
  console.log(`Tagged ${tag}`);
}
if (process.env.GITHUB_OUTPUT) {
  appendFileSync(process.env.GITHUB_OUTPUT, `tagged=${!exists}\ntag=${tag}\n`);
}
