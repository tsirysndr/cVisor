export type ThemeName = "dark" | "light";

// HeroUI keys its theme off a `dark`/`light` class on the root element; portals
// mount on <body>, so we toggle at the document root for consistency.
export function applyTheme(theme: ThemeName) {
  const root = document.documentElement;
  root.classList.toggle("dark", theme === "dark");
  root.classList.toggle("light", theme === "light");
  root.style.colorScheme = theme;
}
