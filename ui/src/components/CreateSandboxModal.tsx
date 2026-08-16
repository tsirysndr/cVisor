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

const schema = z.object({
  name: z
    .string()
    .regex(/^[a-zA-Z0-9_-]*$/, "Letters, digits, _ and - only")
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
  const create = useCreateSandbox();

  const {
    register,
    handleSubmit,
    reset,
    formState: { errors },
  } = useForm<Values>({ resolver: zodResolver(schema), defaultValues: { name: "" } });

  const onSubmit = handleSubmit((values) => {
    const parsed = limitsFromForm(limitsValues);
    if ("error" in parsed) {
      setLimitsError(parsed.error);
      return;
    }
    setLimitsError(null);
    create.mutate(
      { name: values.name || undefined, limits: parsed.limits },
      {
        onSuccess: (sb) => {
          setSelected(sb.id);
          setPanel(true);
          reset();
          setLimitsValues(limitsFormDefaults());
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
            <Input
              {...register("name")}
              autoFocus
              label="Name (optional)"
              variant="bordered"
              radius="sm"
              placeholder="leave blank for a random name"
              isInvalid={!!errors.name}
              errorMessage={errors.name?.message}
            />
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
