// e2e test for the cVisor Deno SDK. Run in a musl deno container with
// CVISOR_LIB pointing at libcvisor.so, under seccomp=unconfined:
//   deno run --allow-ffi --allow-env --unstable-ffi test.ts
import { Sandbox, sh } from "./mod.ts";

function eq(actual: string, expected: string, msg: string) {
  if (actual !== expected) {
    throw new Error(`${msg}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

const sb = new Sandbox();
eq(sb.run("echo hello from deno").stdout, "hello from deno\n", "echo");
eq(sb.run("printf 'a\\nb\\nc\\n' | grep b").stdout, "b\n", "pipeline");
eq(sb.run("echo x > /tmp/f && grep x /tmp/f").stdout, "x\n", "tmp redirect");
eq(sb.run("uname -n").stdout, "cvisor\n", "uname virtualized");
eq(sb.run("grep Name /proc/self/status").stdout, "Name:\tcvisor-guest\n", "proc virtualized");

// Tagged-template runner, including interpolation.
eq(sb.sh`echo templated`.stdout, "templated\n", "sb.sh template");
const word = "hi";
eq(sh`echo ${word} there`.stdout, "hi there\n", "sh template + interpolation");

console.log("DENO_SDK_OK");
