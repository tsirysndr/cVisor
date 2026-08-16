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
      colors: {
        // Extra synthwave accent not covered by HeroUI's semantic slots.
        neon: {
          purple: "#B026FF",
          cyan: "#05D9E8",
          magenta: "#FF2A6D",
        },
      },
    },
  },
  // Synthwave / 80s-neon dark theme: deep near-black violet surfaces, flat
  // neon accents (no gradients), glow via solid box/text-shadow only.
  plugins: [
    heroui({
      defaultTheme: "dark",
      themes: {
        dark: {
          colors: {
            background: "#0D0221",
            foreground: "#F5F5FF",
            focus: "#FF2A6D",
            content1: "#190D2E",
            content2: "#211436",
            content3: "#2C1B45",
            content4: "#3A2558",
            divider: "#2C1B45",
            default: {
              100: "#211436",
              200: "#2C1B45",
              300: "#3A2558",
              400: "#5A4A78",
              500: "#8A7FA6",
              600: "#B5ADCC",
              foreground: "#F5F5FF",
              DEFAULT: "#2C1B45",
            },
            primary: {
              100: "#3D0A20",
              200: "#66112F",
              300: "#99183F",
              400: "#E02460",
              500: "#FF2A6D",
              600: "#FF5C8D",
              foreground: "#FFFFFF",
              DEFAULT: "#FF2A6D",
            },
            secondary: {
              100: "#03323A",
              200: "#054C58",
              300: "#067687",
              400: "#05B4C6",
              500: "#05D9E8",
              600: "#4EE7F2",
              foreground: "#0D0221",
              DEFAULT: "#05D9E8",
            },
            success: {
              500: "#05FFA1",
              foreground: "#0D0221",
              DEFAULT: "#05FFA1",
            },
            warning: {
              500: "#FFD319",
              foreground: "#0D0221",
              DEFAULT: "#FFD319",
            },
            danger: {
              500: "#FF3864",
              foreground: "#FFFFFF",
              DEFAULT: "#FF3864",
            },
          },
        },
      },
    }),
  ],
};
