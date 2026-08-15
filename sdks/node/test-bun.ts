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

function assert(cond: boolean, msg: string) {
  if (!cond) throw new Error(msg);
}
// Exit codes.
assert(sb.runCmd("exit 7").exitCode === 7, "exit code 7");
assert(sb.runCmd("false").exitCode === 1, "exit code 1");
assert(sb.runCmd("true").exitCode === 0, "exit code 0");
// Atomic write-temp-then-rename.
await eq(
  sb.runCmd("echo hi > /tmp/a.part && mv /tmp/a.part /tmp/a && grep hi /tmp/a"),
  "hi\n",
  "atomic rename",
);
// Timeout: a runaway command is SIGKILLed (137).
assert(sb.runCmd("sleep 30", { timeoutMs: 300 }).exitCode === 137, "timeout kills with 137");
// Network kill switch does not crash the shell.
sb.setAllowNetwork(false);
await eq(
  sb.runCmd("(nc -w1 127.0.0.1 9 </dev/null 2>/dev/null || true); echo ok"),
  "ok\n",
  "network disabled keeps shell alive",
);

// Streaming session with an stdout callback.
{
  const sb2 = new Sandbox();
  const chunks: string[] = [];
  const code = await sb2.runStreaming("for i in 1 2 3; do echo line$i; sleep 0.1; done", {
    onStdout: (s) => chunks.push(s),
  });
  assert(code === 0, "streaming exit code");
  assert(chunks.join("") === "line1\nline2\nline3\n", `streaming output: ${JSON.stringify(chunks.join(""))}`);
}

// Interactive PTY shell.
{
  const sb3 = new Sandbox();
  let out = "";
  const shell = sb3.shell({ onOutput: (s) => (out += s) });
  shell.write("echo SHELL_OK\n");
  shell.write("test -t 1 && echo IS_TTY\n");
  shell.write("exit 4\n");
  const code = await shell.wait();
  await new Promise((r) => setTimeout(r, 100));
  assert(code === 4, `shell exit code: ${code}`);
  assert(out.includes("SHELL_OK") && out.includes("IS_TTY"), `shell output: ${JSON.stringify(out)}`);
  shell.close();
}

// File transfer in/out of the sandbox overlay.
{
  const sb4 = new Sandbox();
  sb4.writeFile("/tmp/data.txt", "seeded\n");
  await eq(sb4.runCmd("grep seeded /tmp/data.txt"), "seeded\n", "writeFile visible to run");
  sb4.runCmd("echo from-run > /tmp/out.txt");
  const back = new TextDecoder().decode(sb4.readFile("/tmp/out.txt"));
  assert(back === "from-run\n", `readFile got ${JSON.stringify(back)}`);
  sb4.setAllowListen(true); // smoke: does not throw
}

// Environment variables.
{
  const sbe = new Sandbox();
  sbe.setEnv("FOO", "bar");
  sbe.setEnv("GREETING", "hi there");
  await eq(sbe.runCmd("echo $FOO-$GREETING"), "bar-hi there\n", "setEnv");
}

console.log("BUN_SDK_OK");
