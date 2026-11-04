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
            // key 必须稳定：含 alias 时每敲一个字符 key 变化 → 整卡卸载重挂 →
            // 输入框失焦、别名永远输不进去。行不重排（删除用 index 定位），索引即稳定键
            <div key={i} className="provider-config">
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
