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
import { useAtom, useSetAtom } from "jotai";
import { createModalOpenAtom, selectedSandboxAtom } from "../state/atoms";
import { useCreateSandbox } from "../hooks/useSandboxes";

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
              placeholder="leave blank for a random name"
              isInvalid={!!errors.name}
              errorMessage={errors.name?.message}
            />
          </ModalBody>
          <ModalFooter>
            <Button variant="flat" onPress={() => setOpen(false)}>
              Cancel
            </Button>
            <Button type="submit" color="primary" isLoading={create.isPending}>
              Create
            </Button>
          </ModalFooter>
        </form>
      </ModalContent>
    </Modal>
  );
}
