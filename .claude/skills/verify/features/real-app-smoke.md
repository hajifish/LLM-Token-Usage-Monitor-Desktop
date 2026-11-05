# Real app launch smoke (Lane B)

## Sub-features

- Dev build launches: Vite devUrl `:1420` + Rust backend, window appears at `RunEvent::Ready` (no white-flash rectangle), tray icon appears.
- Encrypted config loads from the real path (`~/Library/Application Support/com.hajifish.llm-token-monitor/config.json`); startup writes log lines via `tauri_plugin_log` (LogDir + Stdout, Info level).
- Real provider polling runs (only for providers configured in the real config); `--minimized` keeps the window hidden.
- Debug semantics: auto-update check skipped, install refused (by design).

## How to get to it (user POV)

This is the app itself — `npm run tauri dev` from the repo root (developer POV of double-clicking the installed app).

## Driving it with the real app (Lane B)

Follow `../SKILL.md` Lane B: config guard snapshot, launch, wait for `Running \`target/debug/local-llm-token-usage-monitor\`` in `/tmp/verify-tauri-dev.log` plus `pgrep -f 'target/debug/local-llm-token-usage-monitor'`. Evidence: `screencapture -x <evidence>/screen.png` showing the window, `tail -20` of the log into `notes.md`, the `pgrep` PID. If providers are configured, the window shows real usage data and the tray menu carries metrics (tray menu contents can be photographed with a second `screencapture` while clicked manually — optional; do not script tray clicks). Teardown per SKILL.md (kill binary, kill the `npm run tauri dev` shell PID, restore config guard). Adjacent Rust check: `cargo test` in `src-tauri/`.

## Gotchas

- **Shared state**: the dev build shares the config file, tray, and real CLI credentials (Claude Code/Codex/Kimi Code dirs are read from `~`) with the user's installed app. Never drive `save_config` (settings/providers pages) here without the guard, and restore it even on failure.
- Only one instance practical: the tray icon and identifier are shared; a second instance adds a duplicate tray icon. Refuse to double-drive.
- Window capture: the window uses overlay styling — `screencapture -x` (full screen) is the reliable evidence; window-ID capture may miss it. Screen Recording permission must be granted to the terminal host, else `screencapture` yields desktop-only images.
- This feature file was authored during skill generation and executed only as far as `cargo`/process checks allow; the full GUI launch path (screenshot + tray) was not exercised in the generator run — first Lane B run should validate these steps and update this file.
