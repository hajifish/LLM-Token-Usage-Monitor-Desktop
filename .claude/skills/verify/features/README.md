# Feature map

One file per user-facing feature of the Token Usage app: what it is, how a user reaches it, how to drive it with the verification harness, and what end state proves it works. This directory is the repo's maintained verification source — when a feature changes, its file changes with it (`/maintain-verification-skill` runs that pass).

Harnesses (see `../SKILL.md`):

- **Lane A** — the real React app in a browser via `http://localhost:1421` (Vite `:1420` behind the shim-injection proxy; Tauri IPC answered by `mock/tauri-shim.js`, assertions via `window.__VERIFY_MOCK__`). Drives every UI feature below.
- **Lane B** — the real Tauri app (`npm run tauri dev`): real window, tray, config file, notifications. Used for Rust-side wiring; see `real-app-smoke.md`.

| Feature | File | Lane |
|---|---|---|
| Usage panel (main view) | [usage-panel.md](usage-panel.md) | A (+B reads it) |
| Providers config | [providers-config.md](providers-config.md) | A |
| Settings | [settings.md](settings.md) | A |
| Skills page | [skills-page.md](skills-page.md) | A |
| Real app launch smoke | [real-app-smoke.md](real-app-smoke.md) | B |

A proof that drives one convenient entry point is incomplete when the map lists others — check whether your change touches more than one feature file before declaring done.
