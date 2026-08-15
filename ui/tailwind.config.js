import { heroui } from "@heroui/react";

/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
    "./node_modules/@heroui/theme/dist/**/*.{js,ts,jsx,tsx}",
  ],
  darkMode: "class",
  theme: {
    extend: {
      fontFamily: {
        // Whole-UI default; terminal/output override to Agave separately.
        sans: ["'Roboto Mono Variable'", "ui-monospace", "monospace"],
        mono: ["'Agave Nerd Font'", "'Agave'", "ui-monospace", "monospace"],
      },
    },
  },
  // Charmbracelet-like dark palette (charm.sh vibe): charcoal surfaces, charm
  // purple accent, hot-pink secondary.
  plugins: [
    heroui({
      defaultTheme: "dark",
      themes: {
        dark: {
          colors: {
            background: "#171717",
            foreground: "#FAFAFA",
            focus: "#7D56F4",
            content1: "#1E1E22",
            content2: "#26262B",
            content3: "#2E2E34",
            content4: "#3A3A42",
            default: {
              100: "#26262B",
              200: "#2E2E34",
              300: "#3A3A42",
              400: "#565660",
              500: "#9CA3AF",
              foreground: "#FAFAFA",
              DEFAULT: "#2E2E34",
            },
            primary: {
              100: "#241a52",
              200: "#3a2a86",
              300: "#523bbf",
              400: "#6a4de0",
              500: "#7D56F4",
              600: "#874BFD",
              foreground: "#FFFFFF",
              DEFAULT: "#7D56F4",
            },
            secondary: {
              500: "#F25D94",
              foreground: "#FFFFFF",
              DEFAULT: "#F25D94",
            },
            success: {
              500: "#A8CC8C",
              foreground: "#0b0b0f",
              DEFAULT: "#A8CC8C",
            },
            warning: {
              500: "#DFAA5A",
              foreground: "#0b0b0f",
              DEFAULT: "#DFAA5A",
            },
            danger: {
              500: "#F25D94",
              foreground: "#FFFFFF",
              DEFAULT: "#F25D94",
            },
          },
        },
      },
    }),
  ],
};
