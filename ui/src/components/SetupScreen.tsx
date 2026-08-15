import { IconShieldLock } from "@tabler/icons-react";
import { ConfigForm } from "./ConfigForm";

// First-run onboarding: shown when no injected/persisted config exists.
export function SetupScreen() {
  return (
    <div className="flex min-h-screen items-center justify-center bg-background p-4">
      <div className="w-full max-w-md rounded-xl border border-content3 bg-content1 p-6 shadow-2xl">
        <div className="mb-5 flex items-center gap-2">
          <IconShieldLock size={24} className="text-primary" />
          <div>
            <h1 className="text-lg font-semibold">Connect to cVisor</h1>
            <p className="text-xs text-default-500">
              Point at a running cvisord and enter its bearer token.
            </p>
          </div>
        </div>
        <ConfigForm submitLabel="Connect" />
      </div>
    </div>
  );
}
