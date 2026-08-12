// e2e test for the Bun entry of the cvisor package. Run in a musl bun
// container with CVISOR_LIB pointing at libcvisor.so, under seccomp=unconfined.
import { Sandbox, sh } from "./src/bun";

async function eq(out: { stdout: () => Promise<string> }, expected: string, msg: string) {
  const got = await out.stdout();
  if (got !== expected) {
    throw new Error(`${msg}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(got)}`);
  }
}

const sb = new Sandbox();
await eq(sb.runCmd("echo hello from bun"), "hello from bun\n", "echo");
await eq(sb.runCmd("printf 'a\\nb\\nc\\n' | grep b"), "b\n", "pipeline");
await eq(sb.runCmd("echo x > /tmp/f && grep x /tmp/f"), "x\n", "tmp redirect");
await eq(sb.runCmd("uname -n"), "cvisor\n", "uname virtualized");
await eq(sb.runCmd("grep Name /proc/self/status"), "Name:\tcvisor-guest\n", "proc virtualized");

// Tagged-template runner, including interpolation.
await eq(sb.sh`echo templated`, "templated\n", "sb.sh template");
const word = "hi";
await eq(sh`echo ${word} there`, "hi there\n", "sh template + interpolation");

console.log("BUN_SDK_OK");
