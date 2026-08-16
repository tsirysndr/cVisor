import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Input, Select, SelectItem } from "@heroui/react";
import { PrimaryButton } from "./Buttons";
import { plainTextField } from "../lib/inputProps";

// macOS-only: settings for the reusable bsdkrun microVM (`cvisor-sandbox`)
// the cvisor CLI boots to host the daemon. Persisted to ~/.cvisor/sandbox.json
// via the Tauri backend; CVISOR_SANDBOX_* env vars still override per field.
// Changes apply the next time the microVM is provisioned.

interface VmSettings {
  image?: string | null;
  tag?: string | null;
  cpus?: number | null;
  memMib?: number | null;
  disk?: string | null;
}

const TAGS = [
  { key: "ubuntu", label: "Ubuntu (default)" },
  { key: "trixie", label: "Debian trixie" },
  { key: "alpine", label: "Alpine" },
];

export function MicroVmSettings() {
  const [tag, setTag] = useState("ubuntu");
  const [image, setImage] = useState("");
  const [cpus, setCpus] = useState("");
  const [memMib, setMemMib] = useState("");
  const [disk, setDisk] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    invoke<VmSettings>("vm_settings_get")
      .then((s) => {
        setTag(s.tag || "ubuntu");
        setImage(s.image ?? "");
        setCpus(s.cpus ? String(s.cpus) : "");
        setMemMib(s.memMib ? String(s.memMib) : "");
        setDisk(s.disk ?? "");
      })
      .catch((e) => setError(String(e)));
  }, []);

  const save = async () => {
    setError(null);
    setSaved(false);
    const cpusN = cpus.trim() ? Number(cpus.trim()) : null;
    const memN = memMib.trim() ? Number(memMib.trim()) : null;
    if (cpusN !== null && (!Number.isInteger(cpusN) || cpusN <= 0)) {
      setError("vCPUs must be a positive whole number");
      return;
    }
    if (memN !== null && (!Number.isInteger(memN) || memN < 256)) {
      setError("Memory must be at least 256 MiB");
      return;
    }
    setSaving(true);
    try {
      await invoke("vm_settings_set", {
        settings: {
          tag: tag === "ubuntu" ? null : tag,
          image: image.trim() || null,
          cpus: cpusN,
          memMib: memN,
          disk: disk.trim() || null,
        },
      });
      setSaved(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="flex h-full flex-col gap-3">
      <div>
        <h3 className="text-sm font-semibold">Sandbox microVM (bsdkrun)</h3>
        <p className="text-xs text-default-500">
          The Linux VM the cvisor CLI boots on macOS to host the daemon.
          Applies the next time the microVM is provisioned; CVISOR_SANDBOX_*
          env vars override.
        </p>
      </div>
      <div className="grid grid-cols-2 gap-2">
        <Select
          label="Image tag"
          variant="bordered"
          radius="sm"
          size="sm"
          selectedKeys={[tag]}
          onSelectionChange={(k) => {
            const v = [...k][0];
            if (typeof v === "string") setTag(v);
          }}
        >
          {TAGS.map((t) => (
            <SelectItem key={t.key}>{t.label}</SelectItem>
          ))}
        </Select>
        <Input
          {...plainTextField}
          label="Image override"
          variant="bordered"
          radius="sm"
          size="sm"
          placeholder="ghcr.io/…/cvisor:custom"
          className="col-span-2"
          value={image}
          onValueChange={setImage}
        />
        <Input
          {...plainTextField}
          label="vCPUs"
          variant="bordered"
          radius="sm"
          size="sm"
          placeholder="2"
          value={cpus}
          onValueChange={setCpus}
        />
        <Input
          {...plainTextField}
          label="Memory (MiB)"
          variant="bordered"
          radius="sm"
          size="sm"
          placeholder="2048"
          value={memMib}
          onValueChange={setMemMib}
        />
        <Input
          {...plainTextField}
          label="Extra disk"
          variant="bordered"
          radius="sm"
          size="sm"
          placeholder="8G, /path/to.raw, or off"
          className="col-span-2"
          value={disk}
          onValueChange={setDisk}
        />
      </div>
      {error && <p className="text-sm text-danger">{error}</p>}
      {/* mt-auto pins the action row to the bottom of the settings section. */}
      <div className="mt-auto flex items-center gap-3">
        <PrimaryButton
          isLoading={saving}
          className="flex-1"
          onPress={() => void save()}
        >
          Save
        </PrimaryButton>
        {saved && <span className="text-xs text-success">saved</span>}
      </div>
    </div>
  );
}
