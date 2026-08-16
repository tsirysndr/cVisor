// Remote-GraphQL e2e smoke for the Node/Bun/Deno package's pure-HTTP client
// (RemoteSandbox over fetch — no libcvisor). Point it at a running cvisord:
//   CVISOR_GRAPHQL_URL=http://127.0.0.1:8080/graphql CVISOR_TOKEN=... bun test-remote.ts
import { RemoteSandbox } from "./src/remote";

const url = process.env.CVISOR_GRAPHQL_URL ?? "http://127.0.0.1:8080/graphql";
const token = process.env.CVISOR_TOKEN ?? "";

function assert(cond: boolean, msg: string): void {
  if (!cond) {
    console.error(`assertion failed: ${msg}`);
    process.exit(1);
  }
}

async function main(): Promise<void> {
  const remote = new RemoteSandbox(url, token);

  const health = await remote.health();
  assert(health.ok, `health not ok: ${JSON.stringify(health)}`);

  const out = await remote.run("echo hello");
  assert(out.stdout === "hello\n", `run stdout: ${JSON.stringify(out)}`);
  assert(out.exitCode === 0, `run exit code: ${JSON.stringify(out)}`);

  const sb = await remote.createSandbox();
  assert(!!sb.id, `createSandbox returned no id: ${JSON.stringify(sb)}`);
  await remote.writeFile(sb.id, "/tmp/data.txt", "round-trip\n");
  const bytes = await remote.readFile(sb.id, "/tmp/data.txt");
  const text = new TextDecoder().decode(bytes);
  assert(text === "round-trip\n", `readFile round-trip: ${JSON.stringify(text)}`);
  await remote.freeSandbox(sb.id);

  console.log("NODE_GRAPHQL_OK");
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
