import type { ReactNode } from "react";

// Shared frame for a main-content view: a header row (title + optional action)
// over a scrollable body.
export function ViewShell({
  title,
  action,
  children,
}: {
  title: string;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="flex h-full flex-col">
      <div className="flex h-12 shrink-0 items-center justify-between border-b border-content3 px-4">
        <h2 className="text-sm font-semibold uppercase tracking-wide text-default-500">
          {title}
        </h2>
        {action}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-4">{children}</div>
    </div>
  );
}
