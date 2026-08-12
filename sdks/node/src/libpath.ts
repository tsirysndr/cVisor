import { arch } from "node:os";

/**
 * Locate libcvisor.so for the FFI backends (Bun, Deno): the CVISOR_LIB
 * environment variable wins, otherwise resolve the copy bundled in the
 * matching @cvisor/linux-* platform package.
 *
 * The platform-package branch needs CommonJS `require` (available in the
 * compiled dist and under Bun); when the sources run directly as ESM without
 * it (e.g. `deno run src/deno.ts` in this repo), CVISOR_LIB is required.
 */
export function libraryPath(): string {
  const override = process.env.CVISOR_LIB;
  if (override) return override;
  if (typeof require === "undefined") {
    throw new Error("cvisor: set CVISOR_LIB to the path of libcvisor.so");
  }
  const { familySync, MUSL } = require("detect-libc");
  const libc = familySync() === MUSL ? "musl" : "gnu";
  return require.resolve(`@cvisor/linux-${arch()}-${libc}/libcvisor.so`);
}
