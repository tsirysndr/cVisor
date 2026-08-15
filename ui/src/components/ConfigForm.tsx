import { useEffect, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { Button, Input } from "@heroui/react";
import { useSetAtom } from "jotai";
import {
  CONFIG_DEFAULTS,
  configAtom,
  deriveWsUrl,
  type CvisorConfig,
} from "../config";
import { checkConnection } from "../lib/checkConnection";

const schema = z.object({
  graphqlUrl: z.string().url("Enter a valid URL"),
  wsUrl: z.string().min(1, "Required"),
  token: z.string(),
});

type Values = z.infer<typeof schema>;

export function ConfigForm({
  initial,
  onSaved,
  onDisconnect,
  submitLabel = "Connect",
}: {
  initial?: CvisorConfig;
  onSaved?: () => void;
  onDisconnect?: () => void;
  submitLabel?: string;
}) {
  const setConfig = useSetAtom(configAtom);
  const [connError, setConnError] = useState<string | null>(null);
  const wsEdited = useRef(false);

  const {
    register,
    handleSubmit,
    watch,
    setValue,
    formState: { errors, isSubmitting },
  } = useForm<Values>({
    resolver: zodResolver(schema),
    defaultValues: initial ?? CONFIG_DEFAULTS,
  });

  // Auto-derive wsUrl from graphqlUrl until the user edits wsUrl by hand.
  const graphqlUrl = watch("graphqlUrl");
  useEffect(() => {
    if (!wsEdited.current && graphqlUrl) {
      setValue("wsUrl", deriveWsUrl(graphqlUrl));
    }
  }, [graphqlUrl, setValue]);

  const onSubmit = handleSubmit(async (values) => {
    setConnError(null);
    try {
      await checkConnection(values);
    } catch (e) {
      setConnError(`Connection failed: ${(e as Error).message}`);
      return;
    }
    setConfig(values);
    onSaved?.();
  });

  return (
    <form onSubmit={onSubmit} className="flex flex-col gap-4">
      <Input
        {...register("graphqlUrl")}
        label="GraphQL URL"
        variant="bordered"
        placeholder="http://localhost:8080/graphql"
        isInvalid={!!errors.graphqlUrl}
        errorMessage={errors.graphqlUrl?.message}
      />
      <Input
        {...register("wsUrl", { onChange: () => (wsEdited.current = true) })}
        label="WebSocket URL"
        variant="bordered"
        placeholder="ws://localhost:8080/graphql/ws"
        isInvalid={!!errors.wsUrl}
        errorMessage={errors.wsUrl?.message}
      />
      <Input
        {...register("token")}
        label="Token"
        type="password"
        variant="bordered"
        placeholder="bearer token"
        isInvalid={!!errors.token}
        errorMessage={errors.token?.message}
      />

      {connError && <p className="text-sm text-danger">{connError}</p>}

      <div className="flex items-center gap-2">
        <Button
          type="submit"
          color="primary"
          isLoading={isSubmitting}
          className="flex-1"
        >
          {submitLabel}
        </Button>
        {onDisconnect && (
          <Button type="button" variant="flat" color="danger" onPress={onDisconnect}>
            Disconnect
          </Button>
        )}
      </div>
    </form>
  );
}
