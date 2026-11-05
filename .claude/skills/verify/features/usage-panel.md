# Usage panel (main view)

## Sub-features

- Provider cards: brand logo, display name/alias, balance (`CNY`/`USD`), token totals (`total_tokens` with `.toLocaleString()`), today's cost/requests.
- Quota meters (providers with `quota_infos`, e.g. Claude Code): bucket name, optional plan tag, remaining %, color banding (green >20 %, orange ≤20 %, red ≤0), reset-time hint.
- Error state: provider with `error` set shows the error text instead of data.
- Header: last-updated time (`.header-time`, `HH:MM:SS` from `last_updated`), manual refresh button (title `刷新数据`), spinner state while refreshing.
- Total balance across providers (`total_balance`).
- Version tag bottom corner (`v<version>`).

## How to get to it (user POV)

Launch the app — this is the initial view. From any other view, the `← 返回` button (settings/skills) or saving/closing the providers page returns here.

## Driving it with Playwright (Lane A)

Navigate to `http://localhost:1421`; the panel loads via `get_usage` on mount. Assert on the accessibility snapshot text: `DeepSeek`, `¥`-formatted balance `42.50` (or raw `42.5`), token count `1,234,567`, quota rows `5h 限额` with `37%` and `周限额` with `71%`, the Zhipu card carrying its error text, and `v0.2.18`. Refresh round trip: read `.header-time`, click button titled `刷新数据`, wait ~1 s, assert `.header-time` advanced and `window.__VERIFY_MOCK__.refreshCount === 1`. Badge colors/low-balance warning: set `__VERIFY_MOCK__.usage.providers[0].balance.available_balance = 3` in the page, call `__VERIFY_MOCK__.fireUsageUpdated()`, snapshot — the DeepSeek card shows the low-balance badge (warning logic: `available_balance < 10` for CNY).

## Gotchas

- `last_updated` format from the backend is `YYYY-MM-DD HH:MM:SS`; the header shows only the time part. Two refreshes within the same second show identical times — assert after a second has passed, or compare full mock state instead.
- The HMR websocket error in console is expected under the proxy; not a failure of this feature.
- Lane B: this panel shows real data only if the real config has providers; empty panel with no error is valid there.
