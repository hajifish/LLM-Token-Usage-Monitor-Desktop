# 供应商配置独立页 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 供应商增删改从设置页拆出为独立管理页（主页「+」进入，保存为反向合并），设置页仅保留即时生效的全局设置。

**Architecture:** App.tsx 三视图状态路由（`usage` / `settings` / `providers`），新增 `Providers.tsx` 组件与 `providerMeta.ts` 共享常量模块；Settings.tsx 精简。纯前端改动，Rust 侧零改动。

**Tech Stack:** React 18 + TypeScript + Tauri v2（`invoke` 调用既有 `get_config` / `save_config` / `get_config_health` 命令）。

**Spec:** `docs/superpowers/specs/2026-11-04-provider-config-page-design.md`

## Global Constraints

- **无前端测试框架**（spec 已锁定）：每个任务的验证环是 `npm run build`（tsc + vite，零错误退出）+ `npm run lint` + `npm run format:check`；最终人工走查用 `npm run tauri dev`。
- 分支 `feature/provider-config-page`，不 push（合并回 develop 走人工验收 + PR 流程）。
- 数据结构不变：不修改 `src/types.ts`、不动任何 Rust 文件、不做配置迁移。
- 提交信息用中文 conventional commits（`feat:` / `refactor:` / `style:` / `docs:`），结尾加 `Co-Authored-By: Claude Code <noreply@anthropic.com>`。
- 样式全部复用 `src/styles/app.css` 现有类，本计划预期 **零 CSS 改动**；若走查发现视觉问题，修复也须只动现有类语义（不新增组件级 inline style，BANNER 横幅 inline 常量除外——那是从 Settings 原样迁入的既有代码）。
- 供应商页 header 必须是「← 返回」+「保存」（`back-btn` / `save-btn-header`）；设置页 header 只留「← 返回」。

---

### Task 1: 抽出共享常量模块 `providerMeta.ts`

**Files:**
- Create: `src/providerMeta.ts`
- Modify: `src/components/Settings.tsx`（仅头部：删除常量定义，改为 import）

**Interfaces:**
- Consumes: 无（零依赖模块，同 `src/types.ts` 风格）
- Produces: `PROVIDER_OPTIONS: { value: string; label: string }[]`、`providerLabel(name: string): string`、`KEYLESS_PROVIDERS: Set<string>`、`IS_MAC: boolean` —— Task 2 的 Providers.tsx 与精简后的 Settings.tsx 都从这里导入。

- [x] **Step 1: 创建 `src/providerMeta.ts`**

内容整块如下（注释从 Settings.tsx 原样迁出，口径提醒必须保留）：

```ts
/**
 * 供应商元数据共享模块 —— 供应商配置页与设置页共用。
 * 此文件保持零依赖，避免循环导入（同 types.ts 风格）。
 */

// 新增类型时同步 providers/mod.rs::ALL_PROVIDER_TYPES 与 tray.rs::tray_short_code
/** 服务商类型下拉与显示名（与 providers/mod.rs 的 display_name 保持同口径）。 */
export const PROVIDER_OPTIONS = [
  { value: 'OpenAI', label: 'OpenAI(API)' },
  { value: 'Codex', label: 'Codex' },
  { value: 'Anthropic', label: 'Anthropic(API)' },
  { value: 'Claude Code', label: 'Claude Code' },
  { value: 'DeepSeek', label: 'DeepSeek' },
  { value: 'Kimi API', label: 'Kimi API' },
  { value: 'Kimi Code', label: 'Kimi Code' },
  { value: 'Zhipu', label: '智谱' },
  { value: 'Qoder', label: 'Qoder' },
];

/** 无需用户配置 API Key，凭证从本机 CLI / 桌面客户端自动读取的供应商类型。
 *  与 Rust needs_api_key 名单同口径；Qoder 虽属免密默认，但其 PAT 兜底入口
 *  在供应商页渲染分支特例放行（所有平台可填、可清空）。 */
export const KEYLESS_PROVIDERS = new Set(['Codex', 'Claude Code', 'Kimi Code', 'Qoder']);

/**
 * 是否 macOS。Qoder 的本地凭据读取（客户端 auth.v1.dat + 钥匙串解密）仅 macOS 支持，
 * 其他平台该路径必然失败，需回退为 PAT 输入（#88）。用 navigator.platform / userAgent
 * 做最小侵入探测，不引入 Tauri 插件。
 */
export const IS_MAC =
  /Mac/i.test(navigator.platform || '') || /Mac OS X/i.test(navigator.userAgent);

/** 根据 provider 标识取默认显示名（下拉框 label）；未知类型兜底返回标识本身。 */
export function providerLabel(name: string): string {
  return PROVIDER_OPTIONS.find((o) => o.value === name)?.label || name;
}
```

- [x] **Step 2: Settings.tsx 改为导入**

删除 Settings.tsx 中以下三段定义（现位于文件头部 12–34 行区域）：
1. `const PROVIDER_OPTIONS = [...]`（含其上方「新增类型时同步…」注释）
2. `const KEYLESS_PROVIDERS = new Set([...])`
3. `const IS_MAC = ...`

`providerLabel` 函数定义（103–106 行）同样删除。然后在文件 import 区加入：

```ts
import { IS_MAC, KEYLESS_PROVIDERS, PROVIDER_OPTIONS, providerLabel } from '../providerMeta';
```

注意：KEYLESS_PROVIDERS / IS_MAC 此刻仍被 Settings.tsx 的供应商渲染分支使用，不可漏导入；`providerLabel` 被托盘列表与别名 placeholder 使用。

- [x] **Step 3: 构建验证**

Run: `npm run build && npm run lint && npm run format:check`
Expected: 三者全部零错误退出（纯重构，无行为变化）。

- [x] **Step 4: Commit**

```bash
git add src/providerMeta.ts src/components/Settings.tsx
git commit -m "$(cat <<'EOF'
refactor: 抽出供应商元数据共享模块 providerMeta.ts

PROVIDER_OPTIONS / KEYLESS_PROVIDERS / IS_MAC / providerLabel 自 Settings.tsx
迁出为零依赖共享模块，供供应商配置页与设置页共用；口径同步提醒注释随迁。

Co-Authored-By: Claude Code <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: 新建供应商配置页组件 `Providers.tsx`

**Files:**
- Create: `src/components/Providers.tsx`

**Interfaces:**
- Consumes: Task 1 的 `providerMeta` 四个导出；Tauri 命令 `get_config` / `save_config` / `get_config_health`（已存在，Rust 侧不动）；类型 `AppConfig` / `ConfigHealth` / `ProviderConfig`（`src/types.ts`）。
- Produces: `export default function Providers({ onBack }: { onBack: () => void })` —— Task 4 的 App.tsx 以 `<Providers onBack={() => setView('usage')} />` 挂载。本任务组件尚未被任何文件引用（App 接线在 Task 4），独立成任务是为了评审粒度：整页逻辑先行落地。

- [x] **Step 1: 创建 `src/components/Providers.tsx`**

完整文件如下。来源说明：横幅样式常量 / 健康状态拉取 / 供应商卡片渲染从 Settings.tsx **原样迁入**（保持既有注释），`handleSave` 是唯一语义改动（反向合并，spec「数据流与字段所有权」节）：

```tsx
import { useState, useEffect } from 'react';
import type { CSSProperties } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { AppConfig, ConfigHealth, ProviderConfig } from '../types';
import { IS_MAC, KEYLESS_PROVIDERS, PROVIDER_OPTIONS, providerLabel } from '../providerMeta';

interface ProvidersProps {
  onBack: () => void;
}

/** 「配置已升级为加密存储」为一次性提示，展示过后写入本地存储不再打扰。 */
const MIGRATED_NOTICE_KEY = 'llmtm.migratedNoticeShown';

/**
 * 健康提示横幅样式。
 *
 * 刻意使用 inline style：全局样式表由其他改动占用，避免产生冲突。
 */
const BANNER_BASE: CSSProperties = {
  margin: '12px 0',
  padding: '12px 14px',
  borderRadius: 8,
  fontSize: 13,
  lineHeight: 1.7,
  border: '1px solid',
  position: 'relative',
  paddingRight: 36,
};

const BANNER_TONE: Record<'danger' | 'warn' | 'info', CSSProperties> = {
  danger: {
    ...BANNER_BASE,
    background: 'rgba(255, 77, 79, 0.10)',
    borderColor: 'rgba(255, 77, 79, 0.55)',
    color: '#ffb3b3',
  },
  warn: {
    ...BANNER_BASE,
    background: 'rgba(250, 173, 20, 0.10)',
    borderColor: 'rgba(250, 173, 20, 0.55)',
    color: '#ffe0a3',
  },
  info: {
    ...BANNER_BASE,
    background: 'rgba(64, 150, 255, 0.10)',
    borderColor: 'rgba(64, 150, 255, 0.55)',
    color: '#bcd9ff',
  },
};

const BANNER_TITLE: CSSProperties = {
  display: 'block',
  fontWeight: 700,
  marginBottom: 4,
  fontSize: 13.5,
};

const BANNER_CLOSE: CSSProperties = {
  position: 'absolute',
  top: 8,
  right: 8,
  border: 'none',
  background: 'transparent',
  color: 'inherit',
  cursor: 'pointer',
  fontSize: 16,
  lineHeight: 1,
  opacity: 0.7,
  padding: '2px 6px',
};

/** 从绝对路径中取出文件名，仅用于展示（不暴露完整目录）。 */
function basename(p: string): string {
  const parts = p.split(/[\\/]/);
  return parts[parts.length - 1] || p;
}

/** 供应商配置页：供应商增删改的唯一入口，由主页「+」按钮进入（App.tsx）。 */
function Providers({ onBack }: ProvidersProps) {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [health, setHealth] = useState<ConfigHealth | null>(null);
  const [dismissed, setDismissed] = useState<Record<string, boolean>>({});
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState('');
  /** 正在确认删除的供应商索引；null 表示未在确认中 */
  const [deletingIndex, setDeletingIndex] = useState<number | null>(null);

  /** 刷新健康状态（保存后重新拉取，以便展示备份文件名）。 */
  const refreshHealth = () =>
    invoke<ConfigHealth>('get_config_health').then(setHealth).catch(console.error);

  useEffect(() => {
    let alive = true;
    // 并行拉取配置与健康状态；健康状态获取失败不阻塞本页使用
    Promise.all([
      invoke<AppConfig>('get_config'),
      invoke<ConfigHealth>('get_config_health').catch(() => null),
    ])
      .then(([cfg, h]) => {
        if (!alive) return;
        setConfig(cfg);
        setHealth(h);
        if (h?.migrated_from_plaintext) {
          if (localStorage.getItem(MIGRATED_NOTICE_KEY)) {
            // 已展示过，不再重复提示
            setDismissed((d) => ({ ...d, migrated: true }));
          } else {
            localStorage.setItem(MIGRATED_NOTICE_KEY, '1');
          }
        }
      })
      .catch(console.error);
    return () => {
      alive = false;
    };
  }, []);

  const dismiss = (key: string) => setDismissed((d) => ({ ...d, [key]: true }));

  const updateProvider = (index: number, field: keyof ProviderConfig, value: string | boolean) => {
    if (!config) return;
    const newProviders = [...config.providers];
    newProviders[index] = { ...newProviders[index], [field]: value };
    setConfig({ ...config, providers: newProviders });
  };

  const addProvider = () => {
    if (!config) return;
    const def = PROVIDER_OPTIONS[0];
    // 别名只承载用户自定义值：新增时不预填，展示时由后端回退到供应商默认名
    setConfig({
      ...config,
      providers: [
        ...config.providers,
        {
          name: def.value,
          api_key: '',
          enabled: true,
          platform_token: null,
          alias: null,
          show_in_tray: true,
        },
      ],
    });
  };

  /**
   * 切换供应商类型：别名只承载用户自定义值。
   * - 别名为空、或恰好等于旧类型的默认名（历史预填残留）→ 置空，让后端回退到新类型默认名；
   * - 用户已输入自定义别名 → 保留，不因切换类型而丢失。
   */
  const handleProviderNameChange = (index: number, newName: string) => {
    if (!config) return;
    const old = config.providers[index];
    const oldAlias = (old.alias || '').trim();
    const keepAlias = oldAlias !== '' && oldAlias !== providerLabel(old.name);
    const newProviders = [...config.providers];
    newProviders[index] = { ...old, name: newName, alias: keepAlias ? old.alias : null };
    setConfig({ ...config, providers: newProviders });
  };

  const removeProvider = (index: number) => {
    if (!config) return;
    const newProviders = config.providers.filter((_, i) => i !== index);
    setConfig({ ...config, providers: newProviders });
    setDeletingIndex(null);
  };

  const handleSave = async () => {
    if (!config) return;
    setSaving(true);
    setMessage('');
    try {
      // 反向合并（spec「数据流与字段所有权」）：providers 数组归本页所有；
      // 其余字段（refresh_interval / auto_check_update / tray_text_enabled /
      // notifications_enabled）一律以 fresh 为准落盘，避免挂载快照回滚
      // 设置页 / 托盘菜单在本页打开期间的即时生效改动。单窗口单视图下
      // 挂载快照即最新，providers[i].show_in_tray 原样带回（本页不渲染它）。
      const fresh = await invoke<AppConfig>('get_config');
      const merged: AppConfig = { ...fresh, providers: config.providers };
      await invoke('save_config', { newConfig: merged });
      setMessage('保存成功');
      // 保存可能触发「覆盖前备份」，重新拉取健康状态以展示备份文件名
      refreshHealth();
    } catch (e) {
      // 后端已将内部错误映射为中文用户可读消息，技术细节仅写日志
      setMessage(`保存失败: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  if (!config) {
    return <div className="settings loading">加载中...</div>;
  }

  const showUnreadable = !!health?.unreadable && !dismissed.unreadable;
  const showDeferred = !!health?.migration_deferred && !dismissed.deferred;
  const showMigrated = !!health?.migrated_from_plaintext && !dismissed.migrated;

  return (
    <div className="settings">
      <div className="settings-header" data-tauri-drag-region>
        <button className="back-btn" onClick={onBack}>
          ← 返回
        </button>
        <button className="save-btn-header" onClick={handleSave} disabled={saving}>
          {saving ? '保存中...' : '保存'}
        </button>
      </div>
      {message && <div className="save-toast">{message}</div>}

      <div className="settings-body">
        {/* 配置健康提示 */}
        {showUnreadable && (
          <div style={BANNER_TONE.danger} role="alert">
            <span style={BANNER_TITLE}>⚠ 检测到本机无法解密的配置文件</span>
            {health?.backup_path ? (
              <span>
                原文件已备份为 <strong>{basename(health.backup_path)}</strong>
                ，继续保存将写入新配置；如需恢复原密钥，请回到原设备或联系支持。
              </span>
            ) : (
              <span>
                原文件目前未被修改；若继续保存，会先自动将原文件备份后再写入新配置。
                如需恢复原密钥，请回到原设备或联系支持。
              </span>
            )}
            <br />
            <span>当前页面展示的是默认空配置（并非你的密钥已丢失）。</span>
            <button
              style={BANNER_CLOSE}
              onClick={() => dismiss('unreadable')}
              aria-label="关闭提示"
            >
              ×
            </button>
          </div>
        )}

        {showDeferred && (
          <div style={BANNER_TONE.warn}>
            <span style={BANNER_TITLE}>配置的加密升级已暂缓</span>
            <span>
              当前无法获取本机稳定的硬件标识，为避免将密钥绑定到不稳定的标识上，
              明文配置的加密升级已暂缓；应用功能不受影响，下次启动若标识恢复可用会自动完成升级。
            </span>
            <button style={BANNER_CLOSE} onClick={() => dismiss('deferred')} aria-label="关闭提示">
              ×
            </button>
          </div>
        )}

        {showMigrated && (
          <div style={BANNER_TONE.info}>
            <span style={BANNER_TITLE}>配置已升级为机器绑定加密存储</span>
            <span>
              配置已升级为机器绑定的加密存储（原明文已安全擦除）。
              注意：回退到旧版本将无法读取该配置，换机或重装系统后需重新录入密钥。
            </span>
            <button style={BANNER_CLOSE} onClick={() => dismiss('migrated')} aria-label="关闭提示">
              ×
            </button>
          </div>
        )}

        {/* 供应商配置（自设置页迁入，本页是唯一编辑入口） */}
        <div className="settings-section">
          {config.providers.map((p, i) => (
            <div key={`${p.name}-${p.alias || i}`} className="provider-config">
              <div className="provider-config-header">
                <select
                  value={p.name}
                  onChange={(e) => handleProviderNameChange(i, e.target.value)}
                >
                  {PROVIDER_OPTIONS.map((opt) => (
                    <option key={opt.value} value={opt.value}>
                      {opt.label}
                    </option>
                  ))}
                </select>
                <input
                  className="alias-input"
                  type="text"
                  placeholder={`别名（默认「${providerLabel(p.name)}」）`}
                  value={p.alias || ''}
                  onChange={(e) => updateProvider(i, 'alias', e.target.value)}
                />
                <label className="toggle">
                  <input
                    type="checkbox"
                    checked={p.enabled}
                    onChange={(e) => updateProvider(i, 'enabled', e.target.checked)}
                  />
                </label>
                {deletingIndex === i ? (
                  <>
                    <span className="delete-confirm-text">确定?</span>
                    <button
                      className="remove-btn remove-btn-confirm"
                      onClick={() => removeProvider(i)}
                    >
                      确定
                    </button>
                    <button className="cancel-btn" onClick={() => setDeletingIndex(null)}>
                      取消
                    </button>
                  </>
                ) : (
                  <button className="remove-btn" onClick={() => setDeletingIndex(i)}>
                    删除
                  </button>
                )}
              </div>
              <div className="provider-fields">
                {p.name === 'Qoder' ? (
                  // Qoder 双认证路径（qoder.rs）：api_key 留空 = 本机客户端凭据（主路径），
                  // 填写 PAT = 令牌兜底。所有平台都放行 PAT 输入：macOS 上客户端未登录或
                  // 钥匙串授权被拒时兜底仍可用，已存的失效 PAT 也能在此清空并保存恢复主路径
                  <>
                    <input
                      type="password"
                      placeholder={
                        IS_MAC
                          ? 'PAT 个人访问令牌（可选，留空自动读取客户端凭据）'
                          : 'PAT 个人访问令牌'
                      }
                      value={p.api_key}
                      onChange={(e) => updateProvider(i, 'api_key', e.target.value)}
                    />
                    <span className="ds-hint">
                      {IS_MAC
                        ? '留空自动读取本机 Qoder / Qoder CN 客户端登录凭据（需已登录，首次弹窗请点允许）；读取失败可改填 PAT，清空并保存即恢复自动读取'
                        : '本地凭据读取暂仅支持 macOS，请填写 PAT 作为凭据来源'}
                    </span>
                  </>
                ) : KEYLESS_PROVIDERS.has(p.name) ? (
                  <span className="ds-hint">
                    {p.name === 'Codex'
                      ? '自动从 ~/.codex/auth.json 读取凭证'
                      : p.name === 'Claude Code'
                        ? '自动从 ~/.claude/credentials.json 读取凭证'
                        : '自动从 ~/.kimi-code/config.toml 读取凭证'}
                  </span>
                ) : (
                  <input
                    type="password"
                    placeholder="API Key"
                    value={p.api_key}
                    onChange={(e) => updateProvider(i, 'api_key', e.target.value)}
                  />
                )}
                {(p.name === 'OpenAI' || p.name === 'Anthropic') && (
                  <span className="ds-hint">需组织管理员密钥（Admin Key）</span>
                )}
                {p.name === 'DeepSeek' && (
                  <>
                    <input
                      type="password"
                      placeholder="平台 Token（可选，查看今日用量）"
                      value={p.platform_token || ''}
                      onChange={(e) => updateProvider(i, 'platform_token', e.target.value)}
                    />
                    <span className="ds-hint">从 platform.deepseek.com 登录后获取</span>
                  </>
                )}
              </div>
            </div>
          ))}
          <button className="add-btn" onClick={addProvider}>
            + 添加服务商
          </button>
        </div>
      </div>
    </div>
  );
}

export default Providers;
```

- [x] **Step 2: 构建验证**

Run: `npm run build && npm run lint && npm run format:check`
Expected: 全部零错误退出。组件尚未被引用，tsc 不报未使用（exported module 无 unused 警告）。

- [x] **Step 3: Commit**

```bash
git add src/components/Providers.tsx
git commit -m "$(cat <<'EOF'
feat: 供应商配置页组件 Providers.tsx

主页「+」进入的完整管理页：健康横幅 + 供应商卡片（类型/别名/启用/删除确认/
各家 Key 字段）+ 添加按钮，header「返回 + 保存」。保存为反向合并——只写回
providers 数组，其余字段以 fresh 为准（spec 字段所有权）。App 接线在后续任务。

Co-Authored-By: Claude Code <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: 精简 Settings.tsx（移除供应商区块与保存逻辑）

**Files:**
- Modify: `src/components/Settings.tsx`（全文件改写为下述内容，约 240 行）

**Interfaces:**
- Consumes: Task 1 的 `providerLabel`（托盘供应商列表显示名）；Tauri 命令 `get_config` / `save_config` / `send_test_notification`。
- Produces: `export default function Settings({ onBack }: { onBack: () => void })` —— 对 App.tsx 的接口不变（props 签名原样），本任务不影响 App.tsx。

- [x] **Step 1: 改写 Settings.tsx**

删除：健康横幅（BANNER 常量、`basename`、`health` 状态、`refreshHealth`、`dismissed`/`dismiss`、`MIGRATED_NOTICE_KEY` 与 localStorage 逻辑）、服务商区块（渲染 + `updateProvider` / `addProvider` / `handleProviderNameChange` / `removeProvider` / `deletingIndex`）、`handleSave` / `saving` /「保存」按钮、`PROVIDER_OPTIONS` / `KEYLESS_PROVIDERS` / `IS_MAC` 导入（本页不再用）。

改写后完整文件：

```tsx
import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { AppConfig, ProviderConfig } from '../types';
import { providerLabel } from '../providerMeta';

interface SettingsProps {
  onBack: () => void;
}

function Settings({ onBack }: SettingsProps) {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [message, setMessage] = useState('');

  useEffect(() => {
    let alive = true;
    invoke<AppConfig>('get_config').then((cfg) => {
      if (alive) setConfig(cfg);
    }).catch(console.error);
    return () => {
      alive = false;
    };
  }, []);

  /** 状态栏显示区块的开关即时生效：读最新配置 → 仅改本区块字段 → 落盘。
   *  不经「保存」按钮，也不影响页面上其他未保存的编辑（如正在输入的密钥）。
   *  在途期间 checkbox 置 disabled（可见、可预期），不静默丢弃点击。 */
  const [trayBusy, setTrayBusy] = useState(false);
  /** 本页的配置行定位键：name + trim 后 alias（空回退 name）。
   *  与后端 provider_show_in_tray 的匹配键并非同口径：后端空回退 display_name
   *  （如 Zhipu→智谱），本键空回退 name——两套键空间互不相交，本键仅用于在
   *  fresh.providers 中唯一定位配置行，不与后端互比。 */
  const providerKey = (p: ProviderConfig) => {
    const a = (p.alias || '').trim();
    return `${p.name}|${a === '' ? p.name : a}`;
  };
  const applyTrayDisplay = async (patch: {
    master?: boolean;
    providerIndex?: number;
    providerName?: string;
    value: boolean;
  }) => {
    setTrayBusy(true);
    try {
      const fresh = await invoke<AppConfig>('get_config');
      if (patch.master !== undefined) {
        fresh.tray_text_enabled = patch.master;
      }
      if (patch.providerIndex !== undefined) {
        const local = config?.providers[patch.providerIndex];
        if (!local) return;
        // 按 name + trim 后 alias 在 fresh 中唯一定位目标（同类型双账号不错位）；
        // 无法唯一对齐说明配置已被外部改动，保守不写、提示重开设置页
        const key = providerKey(local);
        const targets = fresh.providers.filter((fp) => providerKey(fp) === key);
        if (targets.length !== 1) {
          // 先排查本页配置自身的身份键撞车（同类型无别名双账号）：这类配置后端落盘
          // 同样会拒绝（models::validate_provider_identities），提示补别名而非误报「配置已变化」
          const localCount = config.providers.filter((fp) => providerKey(fp) === key).length;
          setMessage(
            localCount > 1
              ? '同类型账号需填写不同别名后才能单独控制显示'
              : '配置已变化，请重新打开设置页',
          );
          return;
        }
        targets[0].show_in_tray = patch.value;
      }
      await invoke('save_config', { newConfig: fresh });
      // 仅在后端写入成功后才同步本页表单状态：只动本区块字段，保留其他未保存编辑
      setConfig((cur) => {
        if (!cur) return cur;
        const providers = cur.providers.map((p, i) =>
          i === patch.providerIndex ? { ...p, show_in_tray: patch.value } : p,
        );
        return {
          ...cur,
          tray_text_enabled: patch.master ?? cur.tray_text_enabled,
          providers,
        };
      });
    } catch (e) {
      // 失败不静默：走 toast 通道；本地状态不翻转，
      // 强制刷新以让受控 checkbox 回弹到真实值
      setMessage(`状态栏设置保存失败: ${e}`);
      setConfig((c) => (c ? { ...c } : c));
    } finally {
      setTrayBusy(false);
    }
  };

  /** 低余额通知开关即时生效：读最新配置 → 仅改本字段 → 落盘。
   *  与 applyTrayDisplay 同模式：不经「保存」按钮，后端写入成功才同步本地状态。 */
  const [notifBusy, setNotifBusy] = useState(false);
  const applyNotificationEnabled = async (value: boolean) => {
    setNotifBusy(true);
    try {
      const fresh = await invoke<AppConfig>('get_config');
      fresh.notifications_enabled = value;
      await invoke('save_config', { newConfig: fresh });
      setConfig((cur) => (cur ? { ...cur, notifications_enabled: value } : cur));
    } catch (e) {
      // 失败不静默：走 toast 通道。受控 checkbox 回弹的主因是 React 18 受控组件
      // restore 机制——react-dom 在 change 事件收尾无条件把 DOM checked 写回已提交
      // state 值（已核 react-dom 18.3 源码验证回弹 100% 成立），本地状态未翻转即回弹；
      // catch 里的 setConfig((c) => ({ ...c })) 只是冗余防御（触发一次重渲染兜底），勿误删或误信它是关键
      setMessage(`通知设置保存失败: ${e}`);
      setConfig((c) => (c ? { ...c } : c));
    } finally {
      setNotifBusy(false);
    }
  };

  /** 发送测试通知：桌面端 API 无法检测投递失败与权限拒绝（后端无条件返回成功），
   *  链路是否真正打通只能由用户亲眼看一次通知来确认。 */
  const [testNotifyBusy, setTestNotifyBusy] = useState(false);
  const sendTestNotification = async () => {
    setTestNotifyBusy(true);
    try {
      await invoke('send_test_notification');
    } catch (e) {
      // 失败不静默：走 toast 通道
      setMessage(`测试通知发送失败: ${e}`);
    } finally {
      setTestNotifyBusy(false);
    }
  };

  if (!config) {
    return <div className="settings loading">加载中...</div>;
  }

  return (
    <div className="settings">
      <div className="settings-header" data-tauri-drag-region>
        <button className="back-btn" onClick={onBack}>
          ← 返回
        </button>
      </div>
      {message && <div className="save-toast">{message}</div>}

      <div className="settings-body">
        {/* 状态栏显示 */}
        <div className="settings-section">
          <div className="tray-display-card">
            <div className="update-row">
              <span className="update-row-label">在菜单栏图标旁显示指标文字</span>
              <label className="toggle">
                <input
                  type="checkbox"
                  checked={config.tray_text_enabled}
                  disabled={trayBusy}
                  onChange={(e) =>
                    applyTrayDisplay({ master: e.target.checked, value: e.target.checked })
                  }
                />
              </label>
            </div>
            {config.tray_text_enabled && (
              <div className="tray-provider-list">
                {config.providers
                  .map((p, i) => ({ p, i }))
                  .filter(({ p }) => p.enabled)
                  .map(({ p, i }) => (
                    <label key={`${p.name}-${p.alias || i}`} className="tray-provider-row">
                      <input
                        type="checkbox"
                        checked={p.show_in_tray}
                        disabled={trayBusy}
                        onChange={(e) =>
                          applyTrayDisplay({
                            providerIndex: i,
                            providerName: p.name,
                            value: e.target.checked,
                          })
                        }
                      />
                      <span>{p.alias?.trim() || providerLabel(p.name)}</span>
                    </label>
                  ))}
              </div>
            )}
            <span className="update-hint">
              开关即时生效，无需保存。关闭总开关后菜单栏仅显示图标；也可对单个服务商关闭常显。
            </span>
          </div>
        </div>

        {/* 低余额通知 */}
        <div className="settings-section">
          <div className="tray-display-card">
            <div className="update-row">
              <span className="update-row-label">余额/配额告警恶化时弹系统通知</span>
              <div className="update-row-actions">
                <button
                  className="cancel-btn"
                  onClick={sendTestNotification}
                  disabled={testNotifyBusy}
                >
                  {testNotifyBusy ? '发送中...' : '发送测试通知'}
                </button>
                <label className="toggle">
                  <input
                    type="checkbox"
                    checked={config.notifications_enabled}
                    disabled={notifBusy}
                    onChange={(e) => applyNotificationEnabled(e.target.checked)}
                  />
                </label>
              </div>
            </div>
            <span className="update-hint">
              开关即时生效。告警恶化时提醒一次，恢复后再次恶化会再提醒；收不到请检查系统通知权限。
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}

export default Settings;
```

注意保留 `applyTrayDisplay` 的 `patch.providerName` 参数与 `providerKey` 定位逻辑原样——那是既有的双账号防错位机制，本次不重构。

- [x] **Step 2: 构建验证**

Run: `npm run build && npm run lint && npm run format:check`
Expected: 全部零错误退出。

- [x] **Step 3: Commit**

```bash
git add src/components/Settings.tsx
git commit -m "$(cat <<'EOF'
refactor: 设置页移除供应商区块与保存按钮

供应商增删改迁往独立配置页（Providers.tsx），健康横幅随之迁出；本页仅留
状态栏显示与低余额通知两个即时生效区块，header 去掉「保存」（已无保存语义）。

Co-Authored-By: Claude Code <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: App.tsx 接线——「+」按钮与 providers 视图

**Files:**
- Modify: `src/App.tsx:12`（view 状态类型）、`src/App.tsx:47-90`（header-actions 加「+」按钮）、`src/App.tsx:101-103`（视图渲染分支）

**Interfaces:**
- Consumes: Task 2 的 `export default Providers`（`<Providers onBack={() => setView('usage')} />`，返回主页）。
- Produces: 无下游消费。

- [x] **Step 1: 扩展 view 类型并导入 Providers**

```tsx
import Providers from './components/Providers';
```

```tsx
const [view, setView] = useState<'usage' | 'settings' | 'providers'>('usage');
```

- [x] **Step 2: header-actions 中刷新按钮与设置图标之间插入「+」按钮**

在 `</button>`（刷新按钮结束，现 70 行）与设置图标 `<button className="settings-icon-btn" ...>` 之间插入（复用 `settings-icon-btn` 类，spec 决策；加号 SVG 同 feather 线条风格，尺寸与设置图标一致 18×18）：

```tsx
                <button
                  className="settings-icon-btn"
                  onClick={() => setView('providers')}
                  title="供应商配置"
                >
                  <svg
                    width="18"
                    height="18"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <line x1="12" y1="5" x2="12" y2="19"></line>
                    <line x1="5" y1="12" x2="19" y2="12"></line>
                  </svg>
                </button>
```

- [x] **Step 3: 视图渲染分支**

将现三元表达式：

```tsx
        ) : (
          <Settings onBack={() => setView('usage')} />
        )}
```

改为：

```tsx
        ) : view === 'settings' ? (
          <Settings onBack={() => setView('usage')} />
        ) : (
          <Providers onBack={() => setView('usage')} />
        )}
```

`navigate` 事件监听（21–31 行）不动——后端目前从不发送该事件，保持现状。

- [x] **Step 4: 构建验证**

Run: `npm run build && npm run lint && npm run format:check`
Expected: 全部零错误退出。

- [x] **Step 5: Commit**

```bash
git add src/App.tsx
git commit -m "$(cat <<'EOF'
feat: 主页 header 加「+」按钮进入供应商配置页

view 扩展 providers 视图；「+」置于刷新与设置图标之间，复用
settings-icon-btn 样式；供应商页返回主页。设置页入口不变。

Co-Authored-By: Claude Code <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: 人工走查验收（不产生提交，发现问题则修复后重跑本任务）

**Files:** 无新增；修复时只动 Task 1–4 涉及的文件。

**Interfaces:**
- Consumes: Task 1–4 全部产出。
- Produces: 验收结论（走查清单全绿 = 可以合回 develop）。

- [x] **Step 1: 静态质量门**

Run: `npm run build && npm run lint && npm run format:check`
Expected: 全部零错误退出（Task 4 后的最终态复核）。

- [ ] **Step 2: `npm run tauri dev` 走查清单**

逐项核对（spec「测试与验收」节）：

1. 主页右上角依次为：时间 → 刷新 → **+** → 设置齿轮；「+」tooltip 为「供应商配置」。
2. 点「+」进入供应商页：header 为「← 返回」+「保存」，卡片列表与原设置页一致（类型下拉 240px + 别名 160px 同行、启用开关、删除两段确认）。
3. 供应商页内：新增供应商 → 改别名 → 切换类型（自定义别名保留、默认别名置空）→ 删除（两段确认）→「保存」→ toast「保存成功」→「← 返回」回主页。
4. 设置页：只剩状态栏显示 + 低余额通知两块；header 只有「← 返回」；托盘总开关、单供应商 show_in_tray、通知开关、测试通知全部即时生效并 toast 反馈正常。
5. 反向合并语义：在供应商页保存后，重新打开设置页，托盘/通知开关仍是保存前的值（未被挂载快照回滚）。
6. 旧配置启动（已有供应商 + 密钥）：供应商页数据完整，无迁移提示，保存后托盘菜单显示正常。
7. 「配置已升级为加密存储」一次性提示首次打开供应商页时出现（若本机配置已迁移过且 localStorage 有记录则不出现——属正常）。

- [ ] **Step 3: 走查通过则宣告完成**

全部通过 → 按 CLAUDE.md 流程停下，等待人工验收与合回 develop 的指示（本计划不含 push / PR）。
发现问题 → 修复 → 重跑 Step 1 + 相关走查项 → 单独提交 `fix: 走查修复——<内容>`。
