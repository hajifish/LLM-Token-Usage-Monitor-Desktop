---
name: verify
description: Drive the Token Usage desktop app (Tauri + React) the way a user does and prove changes work. Two lanes — UI lane runs the real React app in a browser via a mock Tauri IPC shim (usage panel, providers, settings, skills page); real-app lane launches the actual Tauri app for Rust/tray/window wiring. Use after any frontend or Tauri-side change, before claiming a feature works.
---

# verify — Token Usage desktop app

Repo: `LLM-Token-Usage-Monitor-Desktop` — a Tauri v2 menu-bar app (React 18 + Vite frontend, Rust backend) that monitors LLM provider token usage/balance. Frontend runs at Vite `:1420` (strictPort); Rust exposes 14 IPC commands (`get_usage`, `get_config`, `get_config_health`, `save_config`, `trigger_refresh`, `send_test_notification`, `apply_tray_click_behavior`, `apply_window_effects`, `list_skills`, `get_skill_i18n`, `get_update_status`, `check_update`, `download_update`, `install_update`).

## Two lanes — pick per change

| Change touches | Lane |
|---|---|
| `src/**` (React UI, hooks, i18n, styles) | **A — UI lane** (browser + mock IPC) |
| `src-tauri/**` (commands, providers, tray, config, updater) | **B — real-app smoke** (launch the real app) + `cargo test` |
| IPC boundary (new/renamed command) | both: add the handler to the shim (A) and exercise it in B |

Lane A is scriptable and side-effect-free — default to it. Lane B launches a real GUI window and tray on this Mac and shares the user's real config; use it only when Rust-side wiring is what changed.

## Launch

### Lane A — UI harness (default)

Two processes; start both from the repo root, each with its own log file.

```bash
# 1. Vite dev server (the app's own documented dev command) — port must be free
curl -sf http://localhost:1420 >/dev/null || (nohup npm run dev > /tmp/verify-vite.log 2>&1 & echo "vite pid $!")
# wait until ready:
for i in $(seq 1 30); do curl -sf http://localhost:1420 >/dev/null && break; sleep 1; done

# 2. Shim-injection proxy :1421 -> :1420 (mock IPC served at /__verify__/tauri-shim.js)
curl -sf http://localhost:1421 >/dev/null || (nohup node .claude/skills/verify/mock/inject-server.mjs > /tmp/verify-inject.log 2>&1 & echo "inject pid $!")
# wait until ready — this line is the readiness gate:
for i in $(seq 1 15); do curl -sf http://localhost:1421/ | grep -q __verify__ && echo READY && break; sleep 1; done
```

If `:1420` is already serving **this repo** (e.g. the user's own `npm run tauri dev`), reuse it — the proxy is read-only toward upstream. A `:1420` owned by another project means stop and report; do not kill what you did not start.

The app is now at **`http://localhost:1421`**. Reachable ≠ usable: the page is usable when a snapshot shows the `TokenUsage` title and provider cards. An HMR websocket connect error in console is expected (the proxy does not forward websockets) — ignore it.

### Lane B — real app smoke

First run compiles the whole Rust stack (minutes); `src-tauri/target/debug` usually exists so reruns are incremental.

```bash
# 0. CONFIG GUARD — the dev build reads AND writes the same config file as the
#    installed app (no data-dir override exists). Snapshot before, restore after.
#    Use a UNIQUE suffix per session: parallel sessions using the same
#    .verify-bak name have clobbered each other's snapshots (real incident).
CFG="$HOME/Library/Application Support/com.hajifish.llm-token-monitor/config.json"
[ -f "$CFG" ] && cp "$CFG" "$CFG.verify-bak-<session-id>"

# 1. Launch (GUI window + tray appear on this Mac)
nohup npm run tauri dev > /tmp/verify-tauri-dev.log 2>&1 & echo "tauri dev pid $!"
#    Bare binary also works for tray-only paths (no vite needed):
#    nohup src-tauri/target/debug/local-llm-token-usage-monitor > /tmp/verify-app.log 2>&1 &
```

Ready when the log shows cargo's `Finished`/`Running \`target/debug/local-llm-token-usage-monitor\`` line AND the process exists (`pgrep -f 'target/debug/local-llm-token-usage-monitor'`), typically 10-60 s after a warm build. The window shows the real usage panel only if providers are configured in the real config — an empty/error panel is a valid state, not a failure. Debug builds skip the auto-update check and refuse to install updates (by design).

**Tray driving (macOS)** — to click tray menu items: address the process by **unix id** (multiple same-name instances share the menu bar; never address by name), and click with **CGEvent synthesis, never AXPress** — muda does not fire handlers for `click menu item` in System Events; the AX click "succeeds" silently and nothing happens (cost a full debugging session twice). Use the helper:

```bash
osascript -l JavaScript .claude/skills/verify/scripts/tray-cgclick.jxa <pid> [item-substring]
```

It CGEvent-clicks the tray icon (opens menu), AX-reads the item's laid-out position, then CGEvent-clicks the item, retrying layout races. Reading menu structure stays plain AX (`name of every menu item of menu 1 of menu bar item 1 of menu bar 2`, by unix id).

## Doctor

Run first whenever anything looks off. Read-only.

```bash
# Lane A: proxy up AND shim actually injected AND upstream alive
curl -sf http://localhost:1421/ | grep -q __verify__ && echo "lane A: ok" || echo "lane A: proxy/shim down — see /tmp/verify-inject.log"
curl -sf http://localhost:1420 >/dev/null && echo "upstream: ok" || echo "upstream vite down — see /tmp/verify-vite.log"

# Lane B: app process + log + config guard present
pgrep -f 'target/debug/local-llm-token-usage-monitor' || echo "real app not running"
tail -5 /tmp/verify-tauri-dev.log
ls "$HOME/Library/Application Support/com.hajifish.llm-token-monitor/" | grep -c 'verify-bak' >/dev/null || echo "WARNING: no config snapshot — do not save from the app"
```

Port ownership matters: if `lsof -ti tcp:1420` lists a PID you did not start, it is the user's server — reuse (Lane A) or leave alone (Lane B needs it too, as devUrl).

## Drive

Lane A with the Playwright MCP tools (or any browser driver). Stable handles, in preference order — button `title`/`aria-label`, headings, placeholder text:

- **Usage view** (initial): title text `TokenUsage`; provider cards by `.p-card-name` text (`DeepSeek`, `Claude Code`, `智谱`); quota meters by `.quota-name` (`5h 限额`, `周限额`); header time `.header-time`; buttons by title: `刷新数据`, `Skill 清单`, `供应商配置`, `设置`; version tag `.version-tag` (`v0.2.18`).
- **Skills page**: click title `Skill 清单`; agent tabs by aria-label `Claude Code` / `Codex`; search input by placeholder `搜索名称 / 描述 / 插件…`; sections by heading text `用户目录 (n)` and `插件下载(n 个插件)`; plugin group headers are one-line rows — click to expand; back button text `← 返回`.
- **Providers page**: click title `供应商配置`; provider type dropdown options come from `PROVIDER_OPTIONS` (`OpenAI`, `Codex`, `Anthropic`, `Claude Code`, `DeepSeek`, `Kimi API`, `Kimi Code`, `Zhipu`, `Qoder`).
- **Settings**: click title `设置`; toggle rows by label text (`菜单栏显示指标`, `点击图标打开主页`, `余额告警逐级通知`); test-notification button sends a real macOS notification in Lane B, is recorded in the mock in Lane A.

Mock assertions (Lane A only): `window.__VERIFY_MOCK__` exposes the fixture state — `savedConfigs` (every `save_config` payload, in order), `notifications` (`send_test_notification` stamps), `refreshCount` (`trigger_refresh` calls), `windowEffects` (`apply_window_effects` modes), plus mutable `usage` / `config` / `skills` and `fireUsageUpdated()`. A mock command that the app did not issue resolves nothing; an unknown command rejects with `verify-mock: 未模拟的命令 "<cmd>"` — that string in console means a new IPC command needs a shim handler.

Refresh round trip: click `刷新数据` → the mock bumps `last_updated` and emits `usage-updated`; assert `.header-time` changed and `refreshCount` incremented.

## Evidence

Proofs land in `.claude/verify-evidence/<YYYYMMDD-HHMM>-<feature>/` (create it; it is not git-tracked and survives cleanup). For each proof capture, per the standards below:

- `screenshot-<view>.png` — `browser_take_screenshot` after the action, showing the resulting state.
- `snapshot-<view>.txt` — the accessibility snapshot right after (text = assertion basis).
- `console.txt` — `browser_console_messages` at the end; must contain no unexpected errors (the known-ok HMR websocket error excepted).
- `notes.md` — what was driven, what asserted, mock state diffs (`savedConfigs.length`, `refreshCount`, …).

Standards: exercise the real user path (clicks into real views — never poke component state or call commands directly except via the UI); capture the action AND the resulting state, not just the final screen; verify side effects through `__VERIFY_MOCK__` alongside what is visible (a toggle that looks flipped but wrote no `savedConfigs` entry FAILED); mocks exist only at the Tauri IPC boundary, which is a production boundary — never mock inside React. Lane B evidence: `screencapture -x <dir>/screen.png` (whole screen; window-level capture is unreliable for the overlay-styled window), `tail` of `/tmp/verify-tauri-dev.log`, and the `pgrep` output.

## Cleanup

Kill only what you started; evidence survives. Note: `nohup npm run dev & echo $!` records **npm's** PID — the vite child SURVIVES killing it (proven the hard way). The port is the reliable handle, and port-killing is safe precisely because Launch verified the port was free (therefore ours) before starting:

```bash
# Lane A — kill by port (safe: ports were verified free at launch, so they're ours)
lsof -ti tcp:1421 | xargs kill 2>/dev/null   # inject proxy
lsof -ti tcp:1420 | xargs kill 2>/dev/null   # vite + npm wrapper; SKIP if :1420 was the user's server
# verify: `lsof -ti tcp:1420 tcp:1421` empty (ignore non-listener client PIDs like WebKit XPC)
# Lane B — teardown the app, then restore the config guard
pkill -f 'target/debug/local-llm-token-usage-monitor' 2>/dev/null
# also stop the `npm run tauri dev` shell you launched (its recorded PID), which
# would otherwise rebuild-and-relaunch the app after you kill the binary
CFG="$HOME/Library/Application Support/com.hajifish.llm-token-monitor/config.json"
[ -f "$CFG.verify-bak" ] && mv "$CFG.verify-bak" "$CFG"
```

After cleanup, confirm the evidence directory still exists and screenshots open. A cleanup that eats the proof failed.

## Helpers

- `mock/inject-server.mjs` — the proxy above; `VERIFY_UPSTREAM` / `VERIFY_PORT` env-overridable. Ready gate: `curl -sf http://localhost:1421/ | grep -q __verify__`. **Reads the shim file once at startup — after editing `tauri-shim.js`, restart the proxy** (kill `:1421`, relaunch) or the page runs the stale shim.
- `mock/tauri-shim.js` — mock `__TAURI_INTERNALS__` + `__TAURI_EVENT_PLUGIN_INTERNALS__` (both needed by @tauri-apps/api v2.11) + fixtures; no-op when real Tauri is present. If the app gains a new IPC command, add its handler here or Lane A rejects with `verify-mock: 未模拟的命令`.

Adjacent checks (not this skill's lanes): `npm run lint && npm run build` for the frontend; `cargo test` in `src-tauri/` for Rust units (e.g. `notification.rs`).
