"use strict";

const { spawnSync } = require("node:child_process");

// The daemon is Linux-only, so only the Linux platform packages carry it. They
// are shared with @cvisor/cli (each ships both binaries) and installed as
// optional dependencies, so npm picks the one matching the host.
const PACKAGES = {
  "linux-x64": "@cvisor/cli-linux-x64",
  "linux-arm64": "@cvisor/cli-linux-arm64",
};

function binaryPath(name) {
  const host = `${process.platform}-${process.arch}`;
  const pkg = PACKAGES[host];
  if (!pkg) {
    throw new Error(
      `cvisord runs on Linux only — no binary for ${host} (supported: ${Object.keys(PACKAGES).join(", ")})`
    );
  }
  try {
    return require.resolve(`${pkg}/bin/${name}`);
  } catch {
    throw new Error(
      `${pkg} is not installed — reinstall without --no-optional: npm install -g @cvisor/daemon`
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
