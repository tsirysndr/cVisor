import { Logo } from "./Logo";
import { ConfigForm } from "./ConfigForm";

// First-run onboarding: shown when no injected/persisted config exists. The
// backdrop is a drag region so the frameless macOS window can still be moved.
export function SetupScreen() {
  return (
    <div
      data-tauri-drag-region
      className="flex h-full min-h-0 flex-1 items-center justify-center bg-background p-4"
    >
      <div className="w-full max-w-md border border-content3 bg-content1 p-6 shadow-[0_0_30px_rgba(255,42,109,0.12)]">
        <div className="mb-5 flex items-center gap-3">
          <Logo size={28} />
          <div>
            <h1 className="text-lg font-semibold">Connect to cVisor</h1>
            <p className="text-xs text-default-500">
              Point at a running cvisor daemon and enter its bearer token.
            </p>
          </div>
        </div>
        <ConfigForm submitLabel="Connect" />
      </div>
    </div>
  );
}
