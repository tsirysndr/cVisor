import { Input } from "@heroui/react";
import type { Limits } from "../transport";
import { compactSize, parseSize } from "../lib/format";

// The three resource-limit inputs shared by the create and settings modals.
// All fields are free-text and optional; empty = unlimited.
export interface LimitsFormValues {
  cpuPercent: string;
  memoryMax: string;
  pidsMax: string;
}

export function limitsFormDefaults(l?: Limits): LimitsFormValues {
  return {
    cpuPercent: l?.cpuPercent ? String(l.cpuPercent) : "",
    memoryMax: l?.memoryMax ? compactSize(l.memoryMax) : "",
    pidsMax: l?.pidsMax ? String(l.pidsMax) : "",
  };
}

// Validate + convert form values into transport Limits. Returns an error
// message string on invalid input. All-empty yields {} (all limits cleared).
export function limitsFromForm(
  v: LimitsFormValues,
): { limits: Limits } | { error: string } {
  const limits: Limits = {};
  if (v.cpuPercent.trim()) {
    const n = Number(v.cpuPercent.trim());
    if (!Number.isInteger(n) || n <= 0)
      return { error: "CPU must be a positive whole percent (e.g. 50, 200)" };
    limits.cpuPercent = n;
  }
  if (v.memoryMax.trim()) {
    const bytes = parseSize(v.memoryMax);
    if (bytes === null)
      return { error: "Memory must look like 512m, 1g, or a byte count" };
    limits.memoryMax = bytes;
  }
  if (v.pidsMax.trim()) {
    const n = Number(v.pidsMax.trim());
    if (!Number.isInteger(n) || n <= 0)
      return { error: "Max processes must be a positive whole number" };
    limits.pidsMax = n;
  }
  return { limits };
}

export function LimitsFields({
  values,
  onChange,
}: {
  values: LimitsFormValues;
  onChange: (next: LimitsFormValues) => void;
}) {
  const set =
    (key: keyof LimitsFormValues) => (value: string) =>
      onChange({ ...values, [key]: value });

  return (
    <div className="grid grid-cols-3 gap-2">
      <Input
        label="CPU %"
        variant="bordered"
        radius="sm"
        size="sm"
        placeholder="100"
        description="of one core"
        value={values.cpuPercent}
        onValueChange={set("cpuPercent")}
      />
      <Input
        label="Memory"
        variant="bordered"
        radius="sm"
        size="sm"
        placeholder="512m"
        description="empty = unlimited"
        value={values.memoryMax}
        onValueChange={set("memoryMax")}
      />
      <Input
        label="Max procs"
        variant="bordered"
        radius="sm"
        size="sm"
        placeholder="256"
        description="pids.max"
        value={values.pidsMax}
        onValueChange={set("pidsMax")}
      />
    </div>
  );
}
