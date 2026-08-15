// @ts-nocheck
// napi e2e: import the napi entry explicitly — a bare "cvisor" import under
// Bun resolves the "bun" export condition to the FFI entry instead.
import { Sandbox, sh } from "./index";

const isInteractive = Bun.argv.includes("--interactive");
const logLevelIndex = Bun.argv.indexOf("--log-level");
const logLevel =
  logLevelIndex !== -1 && logLevelIndex + 1 < Bun.argv.length
    ? Bun.argv[logLevelIndex + 1]
    : "OFF";

const sb = new Sandbox();
sb.setLogLevel(logLevel);

if (isInteractive) {
  process.stdout.write("cvisor> ");
  for await (const line of console) {
    const cmd = line.trim();
    if (!cmd) {
      process.stdout.write("use 'exit' to exit\n");
      process.stdout.write("cvisor> ");
      continue;
    }
    if (cmd === "exit") break;
    const output = sb.runCmd(cmd);
    const stdout = await output.stdout();
    const stderr = await output.stderr();
    if (stdout) process.stdout.write(stdout);
    if (stderr) process.stderr.write(`\x1b[31m${stderr}\x1b[0m`);
    process.stdout.write("cvisor> ");
  }
} else {
  // Assertions (fail the CI e2e on regression). Exercises the sh tagged
  // template, redirects, pipes, and /proc + uname virtualization.
  async function eq(out, expected: string, msg: string) {
    const got = await out.stdout();
    if (got !== expected) {
      throw new Error(`${msg}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(got)}`);
    }
  }
  await eq(sb.sh`echo hello from node`, "hello from node\n", "sh template");
  await eq(sh`printf 'a\\nb\\nc\\n' | grep b`, "b\n", "pipeline");
  await eq(sb.sh`echo x > /tmp/f && grep x /tmp/f`, "x\n", "tmp redirect");
  await eq(sb.sh`uname -n`, "cvisor\n", "uname virtualized");
  await eq(sb.sh`grep Name /proc/self/status`, "Name:\tcvisor-guest\n", "proc virtualized");

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
  console.log("NODE_SDK_OK");

  const cmds = [
    "echo 'Hello, world!'",
    "sleep 1",
    "pwd",
    "curl -s https://www.google.com",
    "python3 --version",
    "touch hello.py",
    "ls",
    "echo 'print(\"Hello, world!\")' > hello.py",
    "chmod +x hello.py",
    "python3 hello.py",
  ];

  for (const cmd of cmds) {
    const output = sb.runCmd(cmd);
    console.log("cmd:", cmd);
    console.log(
      "\n(stdout):",
      await output.stdout(),
      `\n\x1b[31m(stderr): ${await output.stderr()}\x1b[0m`,
    );
  }
}
