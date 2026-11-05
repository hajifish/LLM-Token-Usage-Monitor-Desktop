// Verification scaffolding — do not ship, do not import from app code.
// Injected by inject-server.mjs BEFORE the app bundle loads. Implements the
// `window.__TAURI_INTERNALS__` surface @tauri-apps/api v2 needs (invoke +
// transformCallback), so the real React app runs in a plain browser with the
// 13 Rust commands answered from fixtures. Only installs when no real Tauri
// is present, so opening the same URL inside the real app is a no-op.
//
// Drive handle: tests read/mutate fixtures via `window.__VERIFY_MOCK__`
// (savedConfigs / notifications / refreshCount / windowEffects / usage /
// config / skills), and can fire `usage-updated` with fireUsageUpdated().
(() => {
  if (window.__TAURI_INTERNALS__) return;

  const stamp = () => {
    const d = new Date();
    const p = (n, w = 2) => String(n).padStart(w, '0');
    // Backend format "YYYY-MM-DD HH:MM:SS" (models.rs last_updated)
    return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
  };

  const usage = {
    providers: [
      {
        name: 'DeepSeek',
        alias: 'DeepSeek',
        enabled: true,
        balance: {
          provider: 'DeepSeek',
          available_balance: 42.5,
          voucher_balance: 0,
          cash_balance: 42.5,
          total_balance: 42.5,
          currency: 'CNY',
        },
        usage: {
          provider: 'DeepSeek',
          total_tokens: 1234567,
          prompt_tokens: 987654,
          completion_tokens: 246913,
          today_cost: 3.14,
          today_requests: 57,
        },
        quota_infos: null,
        error: null,
      },
      {
        name: 'Claude Code',
        alias: 'Claude Code',
        enabled: true,
        balance: null,
        usage: {
          provider: 'Claude Code',
          total_tokens: 9876543,
          prompt_tokens: 7765432,
          completion_tokens: 2111111,
          today_cost: null,
          today_requests: 233,
        },
        quota_infos: [
          {
            name: '5h 限额',
            used: 630000,
            total: 1000000,
            current_value: null,
            remaining: 370000,
            remaining_percent: 37,
            reset_time: '21:35',
            plan_label: 'Pro',
          },
          {
            name: '周限额',
            used: 11400000,
            total: 40000000,
            current_value: null,
            remaining: 28600000,
            remaining_percent: 71,
            reset_time: null,
            plan_label: null,
          },
        ],
        error: null,
      },
      {
        name: 'Zhipu',
        alias: '智谱',
        enabled: true,
        balance: null,
        usage: null,
        quota_infos: null,
        error: '请求失败: 401 Unauthorized（密钥无效）',
      },
    ],
    total_balance: 42.5,
    last_updated: stamp(),
  };

  const config = {
    providers: [
      { name: 'DeepSeek', api_key: 'sk-verify-mock-deepseek', enabled: true, platform_token: null, alias: null, show_in_tray: true },
      { name: 'Claude Code', api_key: '', enabled: true, platform_token: null, alias: null, show_in_tray: true },
      { name: 'Zhipu', api_key: 'sk-verify-mock-zhipu', enabled: true, platform_token: null, alias: null, show_in_tray: false },
    ],
    refresh_interval: 300,
    auto_check_update: true,
    tray_text_enabled: true,
    notifications_enabled: true,
    tray_click_opens_window: false,
    background_mode: 'frozen',
  };

  const skills = {
    claude: [
      { name: 'commit', description: '完成后自动提交代码、推送并创建 PR', source: 'user', plugin_name: null, plugin_org: null, plugin_description: null, path: '/Users/dev/.claude/skills/commit/SKILL.md' },
      { name: 'deploy-check', description: '发布前检查 CI 状态与产物完整性', source: 'user', plugin_name: null, plugin_org: null, plugin_description: null, path: '/Users/dev/.claude/skills/deploy-check/SKILL.md' },
      { name: 'brainstorming', description: 'You MUST use this before any creative work - creating features, building components, adding functionality, or modifying behavior.', source: 'plugin', plugin_name: 'superpowers', plugin_org: 'claude-plugins-official', plugin_description: 'A powerful plugin for Claude Code', path: '/Users/dev/.claude/plugins/cache/superpowers/brainstorming/SKILL.md' },
      { name: 'test-driven-development', description: 'Use when implementing any feature or bugfix, before writing implementation code', source: 'plugin', plugin_name: 'superpowers', plugin_org: 'claude-plugins-official', plugin_description: 'A powerful plugin for Claude Code', path: '/Users/dev/.claude/plugins/cache/superpowers/tdd/SKILL.md' },
      { name: 'setup-pstack', description: 'Configure which models pstack uses per role. Detects available models and writes the override sheet.', source: 'plugin', plugin_name: 'pstack', plugin_org: 'claude-plugins-official', plugin_description: "poteto's agent style and workflows", path: '/Users/dev/.claude/plugins/cache/pstack/setup-pstack/SKILL.md' },
      { name: 'arena', description: 'Spawn N parallel candidates at the same task, pick a base, graft the strongest parts of the losers into it.', source: 'plugin', plugin_name: 'pstack', plugin_org: 'claude-plugins-official', plugin_description: "poteto's agent style and workflows", path: '/Users/dev/.claude/plugins/cache/pstack/arena/SKILL.md' },
    ],
    codex: [
      { name: 'code-review', description: 'Review the current diff for defects', source: 'user', plugin_name: null, plugin_org: null, plugin_description: null, path: '/Users/dev/.codex/skills/code-review/SKILL.md' },
      { name: 'doc-gen', description: 'Generate API docs from the OpenAPI spec', source: 'plugin', plugin_name: 'openapi-tools', plugin_org: 'openai-curated', plugin_description: 'Curated OpenAPI helpers for Codex', path: '/Users/dev/.codex/plugins/openapi-tools/doc-gen/SKILL.md' },
    ],
  };

  const updateStatus = {
    phase: 'idle',
    current_version: '0.2.18',
    version: null,
    notes: null,
    date: null,
    downloaded: 0,
    total: null,
    message: null,
    last_checked: null,
  };

  const state = {
    usage,
    config,
    skills,
    update: updateStatus,
    version: '0.2.18',
    refreshCount: 0,
    savedConfigs: [], // every save_config payload, in order — proof of write round trips
    notifications: [], // send_test_notification timestamps
    windowEffects: [], // apply_window_effects modes
  };
  window.__VERIFY_MOCK__ = state;

  // --- event plumbing (plugin:event) ---
  const callbacks = new Map(); // transformCallback id -> fn
  const eventHandlers = new Map(); // event name -> Set<callback id>
  let cbSeq = 1;

  function emit(event, payload) {
    const ids = eventHandlers.get(event) || [];
    for (const id of ids) {
      const cb = callbacks.get(id);
      if (cb) cb({ event, id, payload });
    }
  }
  state.fireUsageUpdated = (patch) => {
    if (patch) state.usage = patch;
    state.usage.last_updated = stamp();
    emit('usage-updated', state.usage);
    return state.usage;
  };

  const handlers = {
    'plugin:app|version': () => state.version,

    'plugin:event|listen': (args) => {
      // @tauri-apps/api v2 listen() resolves the bare callback id as `eventId`
      const id = args.handler;
      if (!eventHandlers.has(args.event)) eventHandlers.set(args.event, new Set());
      eventHandlers.get(args.event).add(id);
      return id;
    },
    'plugin:event|unlisten': (args) => {
      const ids = eventHandlers.get(args.event);
      if (ids) ids.delete(args.eventId ?? args.handler);
      return null;
    },

    get_usage: () => state.usage,
    get_config: () => state.config,
    get_config_health: () => ({
      unreadable: false,
      backup_path: null,
      migration_deferred: false,
      migrated_from_plaintext: false,
    }),
    save_config: (args) => {
      // The one command whose side effect matters for proofs: record + adopt.
      state.config = args.newConfig;
      state.savedConfigs.push(JSON.parse(JSON.stringify(args.newConfig)));
      return null;
    },
    trigger_refresh: async () => {
      state.refreshCount += 1;
      await new Promise((r) => setTimeout(r, 250)); // feel like a real poll
      state.fireUsageUpdated();
      return null;
    },
    send_test_notification: () => {
      state.notifications.push(stamp());
      return null;
    },
    apply_tray_click_behavior: () => null,
    apply_window_effects: (args) => {
      state.windowEffects.push(args.mode);
      return null;
    },
    list_skills: (args) => {
      const list = state.skills[args.agent];
      if (!list) return Promise.reject(`未知的 agent 参数:${args.agent}(仅支持 claude / codex)`);
      return list;
    },

    // Updater: debug 构建语义 — 自动检查跳过、安装被拒（与真实 debug 行为同口径）
    'updater:get_update_status': () => state.update,
    'updater:check_update': async () => {
      state.update = { ...state.update, phase: 'up_to_date', message: '已是最新版本', last_checked: stamp() };
      return state.update;
    },
    'updater:download_update': () => Promise.reject('debug 构建跳过更新下载'),
    'updater:install_update': () => Promise.reject('debug 构建拒绝安装更新'),
  };
  // Command names arrive un-prefixed for app commands; updater commands in
  // this app are invoked as bare names from useUpdate.ts — accept both spellings.
  handlers['get_update_status'] = handlers['updater:get_update_status'];
  handlers['check_update'] = handlers['updater:check_update'];
  handlers['download_update'] = handlers['updater:download_update'];
  handlers['install_update'] = handlers['updater:install_update'];

  window.__TAURI_INTERNALS__ = {
    invoke: (cmd, args = {}, options) => {
      const h = handlers[cmd];
      if (!h) {
        return Promise.reject(
          `verify-mock: 未模拟的命令 "${cmd}" — 若是新增 IPC 命令，请在 .claude/skills/verify/mock/tauri-shim.js 补充 handler`,
        );
      }
      try {
        return Promise.resolve(h(args, options));
      } catch (e) {
        return Promise.reject(String(e));
      }
    },
    transformCallback: (cb) => {
      const id = cbSeq++;
      callbacks.set(id, cb);
      return id;
    },
    metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
    plugins: {},
  };
  // @tauri-apps/api v2.11 event.ts _unlisten() calls this directly (not via invoke)
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: (event, eventId) => {
      const ids = eventHandlers.get(event);
      if (ids) ids.delete(eventId);
    },
  };
})();
