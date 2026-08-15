/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_CVISOR_GRAPHQL_URL?: string;
  readonly VITE_CVISOR_WS_URL?: string;
  readonly VITE_CVISOR_TOKEN?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
