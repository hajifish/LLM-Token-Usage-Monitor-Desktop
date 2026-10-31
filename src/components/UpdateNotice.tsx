import { useState } from 'react';
import type { UpdateStatus } from '../types';
import { useUpdate } from '../hooks/useUpdate';

/** 字节数 → 人类可读大小。 */
export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

/** 下载进度条；总大小未知时显示为不确定态。 */
export function UpdateProgress({ status }: { status: UpdateStatus }) {
  const { downloaded, total } = status;
  const knownTotal = total && total > 0 ? total : null;
  const percent =
    knownTotal === null ? null : Math.min(100, Math.round((downloaded / knownTotal) * 100));
  return (
    <div className="update-progress">
      <div className="update-progress-bar">
        <div
          className={`update-progress-fill${percent === null ? ' indeterminate' : ''}`}
          style={percent === null ? undefined : { width: `${percent}%` }}
        />
      </div>
      <span className="update-progress-text">
        {percent === null || knownTotal === null
          ? `已下载 ${formatBytes(downloaded)}`
          : `${percent}% · ${formatBytes(downloaded)} / ${formatBytes(knownTotal)}`}
      </span>
    </div>
  );
}

type UpdateApi = ReturnType<typeof useUpdate>;

/** 按更新阶段渲染对应的操作按钮。 */
export function UpdateActions({ status, api }: { status: UpdateStatus; api: UpdateApi }) {
  switch (status.phase) {
    case 'available':
      return (
        <button className="update-btn primary" onClick={api.download}>
          下载更新
        </button>
      );
    case 'downloading':
      return <span className="update-hint">正在下载，请保持应用运行…</span>;
    case 'ready':
      return (
        <button className="update-btn primary" onClick={api.install}>
          立即安装并重启
        </button>
      );
    case 'installing':
      return <span className="update-hint">正在安装更新，应用即将自动重启…</span>;
    case 'failed':
      return status.version ? (
        <button className="update-btn primary" onClick={api.download}>
          重试下载
        </button>
      ) : (
        <button className="update-btn primary" onClick={api.check}>
          重试
        </button>
      );
    case 'checking':
      return <span className="update-hint">正在检查更新…</span>;
    default:
      return (
        <button className="update-btn" onClick={api.check}>
          检查更新
        </button>
      );
  }
}

/** 主界面顶部的更新提示横幅；无可提示内容时渲染为空。 */
function UpdateNotice() {
  const api = useUpdate();
  const { status, error } = api;
  /** 用户点过「稍后」的 (阶段, 版本) 组合；阶段推进后横幅会再次出现 */
  const [acknowledged, setAcknowledged] = useState<string | null>(null);

  if (!status) return null;
  const visible = ['available', 'downloading', 'ready', 'installing'].includes(status.phase);
  if (!visible) return null;
  const key = `${status.phase}:${status.version ?? ''}`;
  if (acknowledged === key) return null;

  const title =
    status.phase === 'available'
      ? `发现新版本 v${status.version}`
      : status.phase === 'downloading'
        ? `正在下载 v${status.version}`
        : status.phase === 'ready'
          ? `新版本 v${status.version} 已下载`
          : '正在安装更新';

  return (
    <div className="update-banner" role="status">
      <div className="update-banner-main">
        <span className="update-banner-title">⬆ {title}</span>
        {status.phase === 'available' && status.notes && (
          <span className="update-banner-notes">{status.notes}</span>
        )}
        {status.phase === 'available' && !status.notes && status.date && (
          <span className="update-banner-notes">发布于 {status.date}</span>
        )}
        {status.phase === 'downloading' && <UpdateProgress status={status} />}
        {status.phase === 'ready' && (
          <span className="update-banner-notes">
            安装会替换当前应用并自动重启；配置与密钥保留在本机。macOS
            若提示输入密码，请输入登录密码以完成替换。
          </span>
        )}
        {error && <span className="update-banner-error">{error}</span>}
      </div>
      <div className="update-banner-actions">
        <UpdateActions status={status} api={api} />
        {(status.phase === 'available' || status.phase === 'ready') && (
          <button className="update-btn ghost" onClick={() => setAcknowledged(key)}>
            稍后
          </button>
        )}
      </div>
    </div>
  );
}

export default UpdateNotice;
