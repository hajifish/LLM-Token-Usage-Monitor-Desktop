# Skills page

## Sub-features

- Agent tabs: `Claude Code` / `Codex` (aria-labels) — each loads that agent's `list_skills` result.
- User section: `用户目录 (n)` grid of cards.
- Plugin section: `插件下载(n 个插件)` — plugin groups collapsed by default, one row per plugin (name + count), click to expand; group header shows the plugin's one-line description.
- Search box (placeholder `搜索名称 / 描述 / 插件…`) — filters across name/description/plugin; forces groups expanded while searching.
- Rescan button (title `重新扫描`), per-card detail (SKILL.md path), Chinese description display via the static zh dictionary with English fallback.

## How to get to it (user POV)

Main view → lightning bolt button in the header (title `Skill 清单`) → skills page → `← 返回`.

## Driving it with Playwright (Lane A)

Navigate to `:1421`, click the button titled `Skill 清单`. Default (claude tab): assert heading `用户目录 (2)`, cards `commit` and `deploy-check`; heading `插件下载(2 个插件)`; the `superpowers` and `pstack` group rows; groups collapsed (their skill cards e.g. `brainstorming` NOT yet visible). Click the `superpowers` group row → its cards (`brainstorming`, `test-driven-development`) appear. Search: type `tdd` — no wait, type `test` into the search box → all groups expand and only matches remain (user section may empty out). Switch to the `Codex` tab (aria-label `Codex`) → `用户目录 (1)` with `code-review`, plugin group `openapi-tools` from org `openai-curated`. Rescan: click title `重新扫描` → content reloads without error.

## Gotchas

- Mock fixtures live in `mock/tauri-shim.js` (`skills.claude` / `skills.codex`). To prove dictionary/fallback behavior, mutate `__VERIFY_MOCK__.skills.claude` in-page and rescan rather than editing fixtures.
- The claude tab reflects the scanning of `~/.claude` in the REAL app — real-machine results differ from mock fixtures; that difference is Lane B territory, not a Lane A bug.
- `list_skills` with an unknown agent rejects with the backend's error string — only reachable via direct invoke, not the UI; don't chase it through the page.
