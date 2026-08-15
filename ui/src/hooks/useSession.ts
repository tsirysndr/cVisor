import { graphQLRequest } from "../graphql/client";
import {
  KILL_SESSION,
  RESIZE_SESSION,
  START_SESSION,
  WRITE_SESSION,
} from "../graphql/queries";

// Plain async wrappers over the session mutations; the Terminal drives these
// imperatively rather than through react-query.

export async function startSession(
  id: string,
  command = "",
  pty = true,
): Promise<string> {
  const d = await graphQLRequest<{ startSession: { id: string } }>(
    START_SESSION,
    { id, command, pty },
  );
  return d.startSession.id;
}

export async function writeSession(id: string, dataBase64: string) {
  await graphQLRequest(WRITE_SESSION, { id, dataBase64 });
}

export async function resizeSession(id: string, rows: number, cols: number) {
  await graphQLRequest(RESIZE_SESSION, { id, rows, cols });
}

export async function killSession(id: string) {
  await graphQLRequest(KILL_SESSION, { id });
}
