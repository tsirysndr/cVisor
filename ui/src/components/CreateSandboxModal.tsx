import { useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import {
  Input,
  Modal,
  ModalBody,
  ModalContent,
  ModalFooter,
  ModalHeader,
  Switch,
} from "@heroui/react";
import { useAtom, useSetAtom } from "jotai";
import {
  createModalOpenAtom,
  selectedSandboxAtom,
  terminalPanelVisibleAtom,
} from "../state/atoms";
import { useCreateSandbox } from "../hooks/useSandboxes";
import {
  LimitsFields,
  limitsFormDefaults,
  limitsFromForm,
  type LimitsFormValues,
} from "./LimitsFields";
import { PrimaryButton, TextButton } from "./Buttons";
import { plainTextField } from "../lib/inputProps";

const schema = z.object({
  name: z
    .string()
    .regex(/^[a-zA-Z0-9_-]*$/, "Letters, digits, _ and - only")
    .optional(),
  repoUrl: z
    .string()
    .regex(/^[A-Za-z0-9@:/._+~%#?=&-]*$/, "Not a valid git URL")
    .optional(),
});

type Values = z.infer<typeof schema>;

export function CreateSandboxModal() {
  const [open, setOpen] = useAtom(createModalOpenAtom);
  const setSelected = useSetAtom(selectedSandboxAtom);
  const setPanel = useSetAtom(terminalPanelVisibleAtom);
  const [limitsValues, setLimitsValues] = useState<LimitsFormValues>(
    limitsFormDefaults(),
  );
  const [limitsError, setLimitsError] = useState<string | null>(null);
  // Daemon defaults: outbound network on, listening servers off.
  const [allowNetwork, setAllowNetwork] = useState(true);
  const [allowListen, setAllowListen] = useState(false);
  const create = useCreateSandbox();

  const {
    register,
    handleSubmit,
    reset,
    watch,
    formState: { errors },
  } = useForm<Values>({
    resolver: zodResolver(schema),
    defaultValues: { name: "", repoUrl: "" },
  });

  // Cloning needs egress, so a repo URL forces outbound networking on.
  const wantsRepo = !!watch("repoUrl")?.trim();

  const onSubmit = handleSubmit((values) => {
    const parsed = limitsFromForm(limitsValues);
    if ("error" in parsed) {
      setLimitsError(parsed.error);
      return;
    }
    setLimitsError(null);
    create.mutate(
      {
        name: values.name || undefined,
        repoUrl: values.repoUrl?.trim() || undefined,
        limits: parsed.limits,
        allowNetwork: allowNetwork || !!values.repoUrl?.trim(),
        allowListen,
      },
      {
        onSuccess: (sb) => {
          setSelected(sb.id);
          setPanel(true);
          reset();
          setLimitsValues(limitsFormDefaults());
          setAllowNetwork(true);
          setAllowListen(false);
          setOpen(false);
        },
      },
    );
  });

  return (
    // Plain dark backdrop (no blur), matching the command palette overlay.
    <Modal
      isOpen={open}
      onOpenChange={setOpen}
      classNames={{ backdrop: "bg-black/60" }}
    >
      <ModalContent>
        <form onSubmit={onSubmit}>
          <ModalHeader>Create sandbox</ModalHeader>
          <ModalBody className="gap-4">
            <Input {...plainTextField}
              {...register("name")}
              autoFocus
              label="Name (optional)"
              variant="bordered"
              radius="sm"
              placeholder="leave blank for a random name"
              isInvalid={!!errors.name}
              errorMessage={errors.name?.message}
            />
            <Input {...plainTextField}
              {...register("repoUrl")}
              label="Git repository (optional)"
              variant="bordered"
              radius="sm"
              placeholder="https://github.com/user/repo.git"
              description="cloned inside the sandbox on creation"
              isInvalid={!!errors.repoUrl}
              errorMessage={errors.repoUrl?.message}
            />
            <div className="flex flex-col gap-2">
              <Switch
                size="sm"
                isSelected={allowNetwork || wantsRepo}
                isDisabled={wantsRepo}
                onValueChange={setAllowNetwork}
              >
                Allow outbound network
                {wantsRepo && (
                  <span className="ml-1 text-[11px] text-default-400">
                    (required to clone)
                  </span>
                )}
              </Switch>
              <Switch
                size="sm"
                isSelected={allowListen}
                onValueChange={setAllowListen}
              >
                Allow listening servers
              </Switch>
            </div>
            <LimitsFields values={limitsValues} onChange={setLimitsValues} />
            {limitsError && (
              <p className="text-sm text-danger">{limitsError}</p>
            )}
            {create.isError && (
              <p className="text-sm text-danger">
                {(create.error as Error).message}
              </p>
            )}
          </ModalBody>
          <ModalFooter className="grid grid-cols-2 gap-2">
            <TextButton onPress={() => setOpen(false)}>Cancel</TextButton>
            <PrimaryButton type="submit" isLoading={create.isPending}>
              Create
            </PrimaryButton>
          </ModalFooter>
        </form>
      </ModalContent>
    </Modal>
  );
}
