// Kill browser/webview text meddling on every form field: no autofill
// dropdown, no macOS autocorrect/auto-capitalization, no spellcheck squiggles.
// Spread into each <Input> (and the cmdk input).
export const plainTextField = {
  autoComplete: "off",
  autoCorrect: "off",
  autoCapitalize: "off",
  spellCheck: "false",
} as const;
