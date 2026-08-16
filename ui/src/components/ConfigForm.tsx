import { useEffect, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { Input } from "@heroui/react";
import { useSetAtom } from "jotai";
import {
  CONFIG_DEFAULTS,
  configAtom,
  deriveWsUrl,
  type CvisorConfig,
} from "../config";
import { isTauri } from "../transport";
import { checkConnection } from "../lib/checkConnection";
import { PrimaryButton, TextButton } from "./Buttons";

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
  // The desktop build talks gRPC over Tauri; the URL field is the daemon's gRPC
  // address and there is no separate websocket endpoint.
  const desktop = isTauri();

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
        label={desktop ? "Daemon gRPC URL" : "GraphQL URL"}
        variant="bordered"
        radius="none"
        placeholder={
          desktop ? "http://127.0.0.1:50051" : "http://localhost:8080/graphql"
        }
        isInvalid={!!errors.graphqlUrl}
        errorMessage={errors.graphqlUrl?.message}
      />
      {!desktop && (
        <Input
          {...register("wsUrl", { onChange: () => (wsEdited.current = true) })}
          label="WebSocket URL"
          variant="bordered"
          radius="none"
          placeholder="ws://localhost:8080/graphql/ws"
          isInvalid={!!errors.wsUrl}
          errorMessage={errors.wsUrl?.message}
        />
      )}
      <Input
        {...register("token")}
        label="Token"
        type="password"
        variant="bordered"
        radius="none"
        placeholder="bearer token"
        isInvalid={!!errors.token}
        errorMessage={errors.token?.message}
      />

      {connError && <p className="text-sm text-danger">{connError}</p>}

      <div className="flex items-center gap-2">
        <PrimaryButton type="submit" isLoading={isSubmitting} className="flex-1">
          {submitLabel}
        </PrimaryButton>
        {onDisconnect && (
          <TextButton
            type="button"
            onPress={onDisconnect}
            className="text-danger data-[hover=true]:text-danger"
          >
            Disconnect
          </TextButton>
        )}
      </div>
    </form>
  );
}
