"use strict";

const { spawnSync } = require("node:child_process");

// One prebuilt package per supported host; npm installs only the matching one
// (they carry `os`/`cpu` fields and are optional dependencies).
const PACKAGES = {
  "linux-x64": "@cvisor/cli-linux-x64",
  "linux-arm64": "@cvisor/cli-linux-arm64",
  "darwin-arm64": "@cvisor/cli-darwin-arm64",
};

function binaryPath(name) {
  const host = `${process.platform}-${process.arch}`;
  const pkg = PACKAGES[host];
  if (!pkg) {
    throw new Error(
      `@cvisor/cli ships no binaries for ${host} (supported: ${Object.keys(PACKAGES).join(", ")})`
    );
  }
  // The daemon is Linux-only, so the darwin package doesn't carry it. On macOS
  // the CLI talks to a cvisord running inside its microVM.
  if (name === "cvisord" && process.platform !== "linux") {
    throw new Error("cvisord runs on Linux only");
  }
  try {
    return require.resolve(`${pkg}/bin/${name}`);
  } catch {
    throw new Error(
      `${pkg} is not installed — reinstall without --no-optional: npm install -g @cvisor/cli`
    );
  }
}

function run(name) {
  let binary;
  try {
    binary = binaryPath(name);
  } catch (err) {
    console.error(err.message);
    process.exit(1);
  }

  const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
  if (result.error) {
    console.error(`failed to run ${binary}: ${result.error.message}`);
    process.exit(1);
  }
  // Die the same way the child did, so shells and supervisors see the signal.
  if (result.signal) {
    process.kill(process.pid, result.signal);
    return;
  }
  process.exit(result.status ?? 1);
}

module.exports = { run };
