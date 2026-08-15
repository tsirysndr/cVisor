// e2e test for the Deno entry of the cvisor package. Run in a musl deno
// container with CVISOR_LIB pointing at libcvisor.so, under seccomp=unconfined:
//   deno run --allow-ffi --allow-env --unstable-sloppy-imports test-deno.ts
// (sloppy imports because the TS sources use extensionless relative imports,
// which tsc requires; the compiled npm package needs no flag)
import { Sandbox, sh } from "./src/deno.ts";

async function eq(out: { stdout: () => Promise<string> }, expected: string, msg: string) {
  const got = await out.stdout();
  if (got !== expected) {
    throw new Error(`${msg}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(got)}`);
  }
}

const sb = new Sandbox();
await eq(sb.runCmd("echo hello from deno"), "hello from deno\n", "echo");
await eq(sb.runCmd("printf 'a\\nb\\nc\\n' | grep b"), "b\n", "pipeline");
await eq(sb.runCmd("echo x > /tmp/f && grep x /tmp/f"), "x\n", "tmp redirect");
await eq(sb.runCmd("uname -n"), "cvisor\n", "uname virtualized");
await eq(sb.runCmd("grep Name /proc/self/status"), "Name:\tcvisor-guest\n", "proc virtualized");

// Tagged-template runner, including interpolation.
await eq(sb.sh`echo templated`, "templated\n", "sb.sh template");
const word = "hi";
await eq(sh`echo ${word} there`, "hi there\n", "sh template + interpolation");

function assert(cond: boolean, msg: string) {
  if (!cond) throw new Error(msg);
}
assert(sb.runCmd("exit 7").exitCode === 7, "exit code 7");
assert(sb.runCmd("false").exitCode === 1, "exit code 1");
await eq(
  sb.runCmd("echo hi > /tmp/a.part && mv /tmp/a.part /tmp/a && grep hi /tmp/a"),
  "hi\n",
  "atomic rename",
);
assert(sb.runCmd("sleep 30", { timeoutMs: 300 }).exitCode === 137, "timeout kills with 137");

console.log("DENO_SDK_OK");
