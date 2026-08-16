import { Button, type ButtonProps } from "@heroui/react";

// The two confirmation/dialog button styles used across the app.
//
// PrimaryButton: solid neon accent, square corners, subtle neon glow — the
// confirming action (Create, Delete, Save, Connect, Run…).
//
// TextButton: text only — no background, no border, no ring — the dismissing
// action (Cancel, Close). Muted text that lifts to the accent on hover.

export function PrimaryButton({ className = "", ...props }: ButtonProps) {
  return (
    <Button
      color="primary"
      radius="sm"
      className={`font-medium text-primary-foreground shadow-[0_0_10px_rgba(255,42,109,0.55)] ${className}`}
      {...props}
    />
  );
}

export function TextButton({ className = "", ...props }: ButtonProps) {
  return (
    <Button
      variant="light"
      radius="sm"
      disableRipple
      className={`bg-transparent px-3 text-default-500 shadow-none data-[hover=true]:bg-transparent data-[hover=true]:text-primary ${className}`}
      {...props}
    />
  );
}
