// Human-readable byte size (e.g. 1536 -> "1.5 KB").
export function humanSize(bytes: number): string {
  if (!bytes) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
  const n = bytes / Math.pow(1024, i);
  return `${i === 0 ? n : n.toFixed(1)} ${units[i]}`;
}

// Parse a human byte size like "512", "256m", "1g", "128k" into bytes
// (mirrors the daemon's cgroup::parse_size). Returns null on invalid input.
export function parseSize(s: string): number | null {
  const m = s.trim().toLowerCase().match(/^(\d+(?:\.\d+)?)\s*([kmg]?)b?$/);
  if (!m) return null;
  const mult = { "": 1, k: 1024, m: 1024 ** 2, g: 1024 ** 3 }[m[2]]!;
  const n = Math.round(parseFloat(m[1]) * mult);
  return n > 0 ? n : null;
}

// Compact inverse of parseSize for prefilling inputs: exact k/m/g when the
// value divides evenly, raw bytes otherwise.
export function compactSize(bytes: number): string {
  for (const [unit, mult] of [
    ["g", 1024 ** 3],
    ["m", 1024 ** 2],
    ["k", 1024],
  ] as const) {
    if (bytes % mult === 0) return `${bytes / mult}${unit}`;
  }
  return String(bytes);
}
