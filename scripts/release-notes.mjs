#!/usr/bin/env node
// Prints one version's section of CHANGELOG.md, for the GitHub Release notes.
//   node scripts/release-notes.mjs 0.1.1
import { readFileSync } from "node:fs";

const version = process.argv[2];
const lines = readFileSync("CHANGELOG.md", "utf8").split("\n");
const start = lines.findIndex((l) => l.trim() === `## ${version}`);
if (start < 0) {
  console.error(`no "## ${version}" section in CHANGELOG.md`);
  process.exit(1);
}
const end = lines.findIndex((l, i) => i > start && l.startsWith("## "));
console.log(lines.slice(start + 1, end < 0 ? undefined : end).join("\n").trim());
