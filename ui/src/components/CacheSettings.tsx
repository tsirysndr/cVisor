import { Input, Select, SelectItem } from "@heroui/react";
import { useAtom } from "jotai";
import { cacheBackendAtom, cacheFormatAtom } from "../state/atoms";
import { plainTextField } from "../lib/inputProps";

const FORMATS = [
  { key: "", label: "gzip (default)" },
  { key: "zstd", label: "zstd" },
  { key: "estargz", label: "estargz" },
  { key: "none", label: "none (tar)" },
];

// Default cache store + archive format used by every cache operation.
// Persisted locally; the S3 backend's credentials come from the daemon's
// AWS_* environment.
export function CacheSettings() {
  const [backend, setBackend] = useAtom(cacheBackendAtom);
  const [format, setFormat] = useAtom(cacheFormatAtom);

  return (
    <div className="flex flex-col gap-4">
      <div>
        <h3 className="text-sm font-semibold">Cache store</h3>
        <p className="text-xs text-default-500">
          Where cache save/restore and the Caches view read from. Leave empty
          for the daemon's local disk store.
        </p>
      </div>
      <Input
        {...plainTextField}
        label="Backend"
        variant="bordered"
        radius="sm"
        placeholder="s3://bucket/prefix (empty = disk)"
        description="S3 credentials come from the daemon's AWS_* environment"
        value={backend}
        onValueChange={setBackend}
      />
      <Select
        label="Archive format"
        variant="bordered"
        radius="sm"
        selectedKeys={[format]}
        onSelectionChange={(k) => {
          const v = [...k][0];
          setFormat(typeof v === "string" ? v : "");
        }}
      >
        {FORMATS.map((f) => (
          <SelectItem key={f.key}>{f.label}</SelectItem>
        ))}
      </Select>
    </div>
  );
}
