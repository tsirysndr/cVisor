import React from "react";
import ReactDOM from "react-dom/client";
import { HeroUIProvider } from "@heroui/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Provider as JotaiProvider } from "jotai";
import "@fontsource-variable/roboto-mono";
import App from "./App";
import "./index.css";

const queryClient = new QueryClient({
  defaultOptions: { queries: { refetchOnWindowFocus: false } },
});

// The active theme (`dark`/`light`) is driven onto <html> by applyTheme; this
// wrapper just carries the semantic background/text tokens.
ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <JotaiProvider>
      <QueryClientProvider client={queryClient}>
        <HeroUIProvider>
          <div className="min-h-screen bg-background text-foreground">
            <App />
          </div>
        </HeroUIProvider>
      </QueryClientProvider>
    </JotaiProvider>
  </React.StrictMode>,
);
