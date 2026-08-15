import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// base "./" keeps asset URLs relative so the build works whether it is served
// statically from any path or embedded in the Tauri desktop shell.
export default defineConfig({
  base: "./",
  plugins: [react()],
  // Tauri expects a fixed dev port and quiet, non-clearing output.
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  envPrefix: ["VITE_", "TAURI_"],
});
