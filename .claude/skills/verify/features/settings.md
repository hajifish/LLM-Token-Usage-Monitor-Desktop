# Settings

## Sub-features

- 菜单栏显示指标 (tray text) master toggle + per-provider `show_in_tray` toggles — save-on-toggle (`save_config` each flip).
- 点击图标打开主页 (`tray_click_opens_window`) — save + `apply_tray_click_behavior`.
- 余额告警逐级通知 (`notifications_enabled`) — save-on-toggle; test-notification button (`send_test_notification`) proves the chain.
- Background mode (`background_mode` frozen/follow) — `apply_window_effects` + config write.
- Update section: current version, `auto_check_update` toggle, manual check (`check_update`).

## How to get to it (user POV)

Main view → gear button in the header (title `设置`) → settings page → `← 返回`.

## Driving it with Playwright (Lane A)

Navigate to `:1421`, click the button titled `设置`. Toggle `菜单栏显示指标`: assert the toggle state flips AND `window.__VERIFY_MOCK__.savedConfigs` gains an entry with `tray_text_enabled` inverted. Toggle `点击图标打开主页`: assert both a `savedConfigs` entry with `tray_click_opens_window: true` (mock state has no tray API to observe beyond the command returning). Test notification: click the test button and assert `__VERIFY_MOCK__.notifications.length` incremented from 0. Version display: `v0.2.18` visible (from `plugin:app|version`). Manual update check resolves to `已是最新版本` (mock `check_update` → `up_to_date`) and shows no download banner.

## Gotchas

- Every toggle here saves immediately — there is no save button. Assert a write per flip; a toggle that visually flips without a new `savedConfigs` entry FAILED.
- External-config-changed guard: if `get_config` returns something the page cannot reconcile, it refuses to write and shows `配置已变化，请重新打开设置页` — that message mid-drive means mock state drifted (e.g. you mutated `config` directly), not an app bug.
- In Lane B the test button fires a real macOS notification — allowed (harmless), but do not flip write-toggles there without the config guard restored afterward.
