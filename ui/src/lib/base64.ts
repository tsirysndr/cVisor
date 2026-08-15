// Binary-safe base64 helpers for the terminal stream (avoid UTF-8 mangling of
// control bytes). xterm accepts a Uint8Array in `write`.

export function b64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

export function bytesToB64(bytes: Uint8Array): string {
  let bin = "";
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
  return btoa(bin);
}

// xterm onData yields UTF-8 text; encode to bytes before base64.
export function strToB64(s: string): string {
  return bytesToB64(new TextEncoder().encode(s));
}
