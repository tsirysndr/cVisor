# App icons

Source of truth: `icon.svg` (charm-purple cVisor mark). The PNG/ICNS set here
was rendered from it. `icon.ico` (Windows) is **not** checked in because this
environment lacks ImageMagick — regenerate the full platform set with:

```bash
bunx --bun @tauri-apps/cli icon src-tauri/icons/icon.svg
```
