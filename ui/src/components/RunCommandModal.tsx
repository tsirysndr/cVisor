import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import {
  Button,
  Input,
  Modal,
  ModalBody,
  ModalContent,
  ModalFooter,
  ModalHeader,
} from "@heroui/react";
import { useAtom, useAtomValue } from "jotai";
import { runModalOpenAtom, selectedSandboxAtom } from "../state/atoms";
import { useRun } from "../hooks/useRun";
import { OutputSkeleton } from "./Skeletons";

const schema = z.object({
  command: z.string().min(1, "Enter a command"),
  timeoutMs: z.coerce.number().int().nonnegative().optional(),
});

type Values = z.infer<typeof schema>;

// One-off `run` against the selected sandbox (or a fresh ephemeral one when
// none is selected), showing stdout/stderr/exit code.
export function RunCommandModal() {
  const [open, setOpen] = useAtom(runModalOpenAtom);
  const selected = useAtomValue(selectedSandboxAtom);
  const run = useRun();

  const {
    register,
    handleSubmit,
    formState: { errors },
  } = useForm<Values>({
    resolver: zodResolver(schema),
    defaultValues: { command: "" },
  });

  const onSubmit = handleSubmit((values) => {
    run.mutate({
      id: selected,
      command: values.command,
      timeoutMs: values.timeoutMs || undefined,
    });
  });

  return (
    <Modal
      isOpen={open}
      onOpenChange={setOpen}
      backdrop="blur"
      size="2xl"
      scrollBehavior="inside"
    >
      <ModalContent>
        <ModalHeader className="flex-col items-start gap-0">
          <span>Run command</span>
          <span className="text-xs font-normal text-default-500">
            {selected ? `in ${selected}` : "in a fresh ephemeral sandbox"}
          </span>
        </ModalHeader>
        <ModalBody className="gap-4">
          <form onSubmit={onSubmit} className="flex items-end gap-2">
            <Input
              {...register("command")}
              autoFocus
              label="Command"
              variant="bordered"
              placeholder="uname -a"
              isInvalid={!!errors.command}
              errorMessage={errors.command?.message}
              className="flex-1"
            />
            <Input
              {...register("timeoutMs")}
              label="Timeout (ms)"
              type="number"
              variant="bordered"
              placeholder="0"
              className="w-36"
              isInvalid={!!errors.timeoutMs}
              errorMessage={errors.timeoutMs?.message}
            />
            <Button type="submit" color="primary" isLoading={run.isPending}>
              Run
            </Button>
          </form>

          {run.isPending && <OutputSkeleton />}

          {run.isError && (
            <p className="text-sm text-danger">
              {(run.error as Error).message}
            </p>
          )}

          {run.data && (
            <div className="flex flex-col gap-2 font-mono text-xs">
              <div className="flex items-center gap-2">
                <span className="text-default-500">exit</span>
                <span
                  className={
                    run.data.exitCode === 0 ? "text-success" : "text-danger"
                  }
                >
                  {run.data.exitCode}
                </span>
              </div>
              {run.data.stdout && (
                <pre className="max-h-56 overflow-auto whitespace-pre-wrap rounded-md bg-content2 p-3">
                  {run.data.stdout}
                </pre>
              )}
              {run.data.stderr && (
                <pre className="max-h-56 overflow-auto whitespace-pre-wrap rounded-md bg-content2 p-3 text-danger">
                  {run.data.stderr}
                </pre>
              )}
            </div>
          )}
        </ModalBody>
        <ModalFooter>
          <Button variant="flat" onPress={() => setOpen(false)}>
            Close
          </Button>
        </ModalFooter>
      </ModalContent>
    </Modal>
  );
}
