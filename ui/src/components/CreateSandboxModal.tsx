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
  const create = useCreateSandbox();

  const {
    register,
    handleSubmit,
    reset,
    formState: { errors },
  } = useForm<Values>({ resolver: zodResolver(schema), defaultValues: { name: "" } });

  const onSubmit = handleSubmit((values) => {
    create.mutate(values.name || undefined, {
      onSuccess: (sb) => {
        setSelected(sb.id);
        setPanel(true);
        reset();
        setOpen(false);
      },
    });
  });

  return (
    <Modal isOpen={open} onOpenChange={setOpen} backdrop="blur">
      <ModalContent>
        <form onSubmit={onSubmit}>
          <ModalHeader>Create sandbox</ModalHeader>
          <ModalBody>
            <Input
              {...register("name")}
              autoFocus
              label="Name (optional)"
              variant="bordered"
              radius="none"
              placeholder="leave blank for a random name"
              isInvalid={!!errors.name}
              errorMessage={errors.name?.message}
            />
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
