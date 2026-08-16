import { useEffect, useRef, useState } from "react";
import { IconEye, IconEyeOff } from "@tabler/icons-react";
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
import { plainTextField } from "../lib/inputProps";

const schema = z.object({
  graphqlUrl: z.string().url("Enter a valid URL"),
  wsUrl: z.string(),
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
  const [showToken, setShowToken] = useState(false);
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
  // Not on desktop: the URL there is a gRPC address with no ws endpoint.
  const graphqlUrl = watch("graphqlUrl");
  useEffect(() => {
    if (!desktop && !wsEdited.current && graphqlUrl) {
      setValue("wsUrl", deriveWsUrl(graphqlUrl));
    }
  }, [graphqlUrl, setValue, desktop]);

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
    <form onSubmit={onSubmit} className="flex h-full flex-col gap-4">
      <Input {...plainTextField}
        {...register("graphqlUrl")}
        label={desktop ? "Daemon gRPC URL" : "GraphQL URL"}
        variant="bordered"
        radius="sm"
        placeholder={
          desktop ? "http://localhost:50051" : "http://localhost:8080/graphql"
        }
        isInvalid={!!errors.graphqlUrl}
        errorMessage={errors.graphqlUrl?.message}
      />
      {!desktop && (
        <Input {...plainTextField}
          {...register("wsUrl", { onChange: () => (wsEdited.current = true) })}
          label="WebSocket URL"
          variant="bordered"
          radius="sm"
          placeholder="ws://localhost:8080/graphql/ws"
          isInvalid={!!errors.wsUrl}
          errorMessage={errors.wsUrl?.message}
        />
      )}
      <Input {...plainTextField}
        {...register("token")}
        label="Token"
        type={showToken ? "text" : "password"}
        variant="bordered"
        radius="sm"
        placeholder="bearer token"
        isInvalid={!!errors.token}
        errorMessage={errors.token?.message}
        endContent={
          <button
            type="button"
            aria-label={showToken ? "Hide token" : "Show token"}
            className="text-default-400 transition hover:text-foreground"
            onClick={() => setShowToken((v) => !v)}
          >
            {showToken ? <IconEyeOff size={18} /> : <IconEye size={18} />}
          </button>
        }
      />

      {connError && <p className="text-sm text-danger">{connError}</p>}

      {/* Paired action rows split evenly so both buttons match in width;
          mt-auto pins them to the bottom of the settings section. */}
      <div className={onDisconnect ? "mt-auto grid grid-cols-2 gap-2" : "mt-auto flex"}>
        {onDisconnect && (
          <TextButton
            type="button"
            onPress={onDisconnect}
            className="text-danger data-[hover=true]:text-danger"
          >
            Disconnect
          </TextButton>
        )}
        <PrimaryButton type="submit" isLoading={isSubmitting} className="flex-1">
          {submitLabel}
        </PrimaryButton>
      </div>
    </form>
  );
}
