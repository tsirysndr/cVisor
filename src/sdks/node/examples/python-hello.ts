/**
 * Running Python inside a cVisor sandbox.
 *
 * How to write a Python script to disk and execute it,
 * all within the same isolated sandbox instance.
 */
import { Sandbox } from "cvisor";

const sb = new Sandbox();

// Helper function to run a bash command and log its output streams.
async function run(cmd: string): Promise<void> {
  const output = sb.runCmd(cmd);
  const stdout = await output.stdout();
  const stderr = await output.stderr();
  console.log(`cvisor> ${cmd}`);
  if (stdout) console.log(stdout.trimEnd());
  if (stderr) console.error(stderr.trimEnd());
  console.log();
}

// Write a simple Python script to the sandbox filesystem, then run it.
await run("python3 --version");
await run("echo 'print(\"Hello from Python inside cVisor!\")' > hello.py");
await run("python3 hello.py");

// Here is a script that produces some of the Fibonacci sequence.
const fibScript = [
  "def fib(n):",
  "    a, b = 0, 1",
  "    for _ in range(n):",
  "        print(a, end=' ')",
  "        a, b = b, a + b",
  "    print()",
  "fib(10)",
].join("\n");

await run(`python3 -c "${fibScript}"`);