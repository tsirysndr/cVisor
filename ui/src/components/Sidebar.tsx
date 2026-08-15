import { Button } from "@heroui/react";
import { useAtom, useSetAtom } from "jotai";
import { IconPlus, IconTrash } from "@tabler/icons-react";
import { useFreeSandbox, useSandboxes } from "../hooks/useSandboxes";
import { createModalOpenAtom, selectedSandboxAtom } from "../state/atoms";
import { SandboxListSkeleton } from "./Skeletons";

export function Sidebar() {
  const { data: sandboxes, isLoading } = useSandboxes();
  const free = useFreeSandbox();
  const [selected, setSelected] = useAtom(selectedSandboxAtom);
  const setCreateOpen = useSetAtom(createModalOpenAtom);

  return (
    <aside className="flex w-64 shrink-0 flex-col border-r border-content3 bg-content1">
      <div className="flex items-center justify-between px-3 py-2">
        <span className="text-xs font-medium uppercase tracking-wide text-default-500">
          Sandboxes
        </span>
        <Button
          isIconOnly
          size="sm"
          variant="light"
          aria-label="Create sandbox"
          onPress={() => setCreateOpen(true)}
        >
          <IconPlus size={16} />
        </Button>
      </div>

      <div className="flex-1 overflow-y-auto px-2 pb-2">
        {isLoading ? (
          <SandboxListSkeleton />
        ) : sandboxes && sandboxes.length > 0 ? (
          <ul className="flex flex-col gap-1">
            {sandboxes.map((sb) => {
              const active = sb.id === selected;
              return (
                <li key={sb.id}>
                  <div
                    role="button"
                    tabIndex={0}
                    onClick={() => setSelected(sb.id)}
                    onKeyDown={(e) =>
                      (e.key === "Enter" || e.key === " ") && setSelected(sb.id)
                    }
                    className={`group flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-sm ${
                      active
                        ? "bg-primary/20 text-foreground"
                        : "text-default-500 hover:bg-content2 hover:text-foreground"
                    }`}
                  >
                    <span
                      className={`h-1.5 w-1.5 rounded-full ${
                        active ? "bg-primary" : "bg-default-400"
                      }`}
                    />
                    <div className="min-w-0 flex-1">
                      <div className="truncate">{sb.name}</div>
                      <div className="truncate text-[10px] text-default-400">
                        {sb.id}
                      </div>
                    </div>
                    <button
                      aria-label="Free sandbox"
                      className="opacity-0 transition group-hover:opacity-100 hover:text-danger"
                      onClick={(e) => {
                        e.stopPropagation();
                        free.mutate(sb.id, {
                          onSuccess: () => active && setSelected(null),
                        });
                      }}
                    >
                      <IconTrash size={15} />
                    </button>
                  </div>
                </li>
              );
            })}
          </ul>
        ) : (
          <p className="px-2 py-4 text-xs text-default-400">No sandboxes yet.</p>
        )}
      </div>
    </aside>
  );
}
