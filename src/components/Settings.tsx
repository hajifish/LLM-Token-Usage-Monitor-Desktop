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
    invoke<AppConfig>('get_config')
      .then((cfg) => {
        if (alive) setConfig(cfg);
      })
      .catch(console.error);
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
   *  链路是否真正打通只能由用户亲眼看一次通知来确认。成功也给 toast 反馈：
   *  让「点过按钮」这件事可见——dev 模式（tauri dev 未打包）通知归属终端/IDE
   *  宿主应用，打包后才归属本应用，能否弹横幅取决于宿主的通知权限与专注模式。 */
  const [testNotifyBusy, setTestNotifyBusy] = useState(false);
  const sendTestNotification = async () => {
    setTestNotifyBusy(true);
    try {
      await invoke('send_test_notification');
      setMessage(
        import.meta.env.DEV
          ? '测试通知已发出（dev 模式归属终端/IDE 宿主应用）；未收到请检查宿主的通知权限与专注模式'
          : '测试通知已发出；未收到请检查本应用的通知权限与专注模式',
      );
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
