# Providers config

## Sub-features

- Add a provider: type dropdown (from `PROVIDER_OPTIONS`: `OpenAI`, `Codex`, `Anthropic`, `Claude Code`, `DeepSeek`, `Kimi API`, `Kimi Code`, `Zhipu`, `Qoder`), optional alias, API key where the type needs one (`KEYLESS_PROVIDERS` — Codex, Claude Code, Kimi Code, Qoder — hide/deny the key field except Qoder's PAT fallback).
- Edit existing provider rows: rename alias, rotate key, enable/disable.
- Remove a provider.
- Save persists via `save_config`; returning to the usage view triggers an optimistic row + refresh (`refreshAfterProviderChange`).

## How to get to it (user POV)

Main view → `+` button in the header (title `供应商配置`) → providers page → `← 返回` to go back.

## Driving it with Playwright (Lane A)

Navigate to `:1421`, click the button titled `供应商配置`. Add flow: click the add button, pick `Kimi API` from the type dropdown, type an alias and key, save. Proof is two-sided: (1) the usage view (after `← 返回`) shows the new provider row; (2) `window.__VERIFY_MOCK__.savedConfigs` gained an entry whose `providers` array contains the new `{name: 'Kimi API', api_key: …}` — the write round trip, not just the visual row. Edit flow: change an alias, save, assert `savedConfigs.at(-1)` reflects it and the card name updates after return. Remove flow: assert the provider disappears from both the UI and the last saved config.

## Gotchas

- The visible row appearing instantly is the optimistic path; the config write is the real assertion. A row that shows but wrote nothing (`savedConfigs` unchanged) is a FAILED proof.
- In the real app this page writes the encrypted config file; in Lane A it only mutates mock state. Never drive this page in Lane B during verification (shared real config).
- Keyless types: the key input's absence for `Codex`/`Claude Code`/`Kimi Code` is correct behavior, not a missing element.
