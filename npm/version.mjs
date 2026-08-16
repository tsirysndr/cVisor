#!/usr/bin/env node
// Stamp a release version onto every npm package here: the committed versions
// are placeholders, and the wrapper's optionalDependencies must pin the exact
// version of the platform packages published alongside it.
//
//   node npm/version.mjs 0.2.0

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error(`usage: node npm/version.mjs <semver>  (got: ${version ?? "nothing"})`);
  process.exit(1);
}

const root = dirname(fileURLToPath(import.meta.url));
const packages = ["cli", "linux-x64", "linux-arm64", "darwin-arm64"];

for (const dir of packages) {
  const path = join(root, dir, "package.json");
  const pkg = JSON.parse(readFileSync(path, "utf8"));
  pkg.version = version;
  for (const name of Object.keys(pkg.optionalDependencies ?? {})) {
    pkg.optionalDependencies[name] = version;
  }
  writeFileSync(path, `${JSON.stringify(pkg, null, 2)}\n`);
  console.log(`${pkg.name} -> ${version}`);
}
