import { useEffect, useRef, useState } from 'react';
import type { UpdateStatus } from '../types';
import { useUpdate } from '../hooks/useUpdate';
import { parseReleaseNotes, type ReleaseNotes } from '../lib/releaseNotes';

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

/** ready 阶段的安装说明长文；横幅只留一句短提示，全文固定在弹窗 footer 上方。 */
const INSTALL_NOTE =
  '安装会替换当前应用并自动重启；配置与密钥保留在本机。macOS 若提示输入密码，请输入登录密码以完成替换。';

/** 分节 → 语义类别（条目圆点配色用）；未识别类别退蓝色 */
function sectionKind(title: string): string {
  if (title.startsWith('新功能') || title.startsWith('新增')) return 'feature';
  if (title.startsWith('修复')) return 'fix';
  if (title.startsWith('优化') || title.startsWith('改进')) return 'improve';
  return 'other';
}

/** 解析后的发版说明 → 分节 + 条目 + 脚注的排版视图。 */
function ReleaseNotesView({ parsed }: { parsed: ReleaseNotes }) {
  if (parsed.fallbackText !== null) {
    return <p className="rn-fallback">{parsed.fallbackText}</p>;
  }
  return (
    <div className="rn-body">
      {parsed.preamble !== null && <p className="rn-fallback">{parsed.preamble}</p>}
      {parsed.sections.map((section, si) => (
        <section
          key={`${si}-${section.title}`}
          className="rn-section"
          data-kind={sectionKind(section.title)}
        >
          <h4 className="rn-section-title">{section.title}</h4>
          <ul className="rn-items">
            {section.items.map((item, i) => (
              <li key={i} className="rn-item">
                <span className="rn-item-dot" aria-hidden />
                <p className="rn-item-text">
                  {item.name !== null && <strong className="rn-item-name">{item.name}：</strong>}
                  <span className="rn-item-detail">{item.detail}</span>
                </p>
              </li>
            ))}
          </ul>
        </section>
      ))}
      {parsed.footnote !== null && <p className="rn-footnote">{parsed.footnote}</p>}
    </div>
  );
}

type UpdateApi = ReturnType<typeof useUpdate>;

/** 更新内容弹窗。Esc / 背景点击 / ✕ 关闭，焦点进出自管理；安装说明固定不随正文滚动。 */
function UpdateDialog({
  status,
  api,
  onClose,
}: {
  status: UpdateStatus;
  api: UpdateApi;
  onClose: () => void;
}) {
  const cardRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const prev = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    cardRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('keydown', onKey);
      prev?.focus();
    };
  }, [onClose]);

  const parsed = status.notes !== null ? parseReleaseNotes(status.notes) : null;
  const ready = status.phase === 'ready';
  return (
    <div
      className="update-dialog-overlay"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="update-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="update-dialog-title"
        ref={cardRef}
        tabIndex={-1}
      >
        <div className="update-dialog-header">
          <div className="update-dialog-heading">
            <div className="update-dialog-kicker">发现新版本</div>
            <h3 className="update-dialog-title" id="update-dialog-title">
              v{status.version}
            </h3>
            <div className="update-dialog-meta">
              {status.date !== null && <span>发布于 {status.date.slice(0, 10)}</span>}
              <span>当前 v{status.current_version}</span>
            </div>
          </div>
          <button className="update-dialog-close" aria-label="关闭更新内容" onClick={onClose}>
            ✕
          </button>
        </div>
        <div className="update-dialog-body">
          {parsed !== null ? (
            <ReleaseNotesView parsed={parsed} />
          ) : (
            <p className="rn-fallback">本版本未附更新说明。</p>
          )}
        </div>
        {ready && (
          <div className="update-dialog-note-slot">
            <p className="update-dialog-note">{INSTALL_NOTE}</p>
          </div>
        )}
        <div className="update-dialog-footer">
          <button className="update-btn ghost" onClick={onClose}>
            稍后
          </button>
          <button
            className="update-btn primary"
            onClick={() => {
              onClose();
              if (ready) void api.install();
              else void api.download();
            }}
          >
            {ready ? '立即安装并重启' : '下载更新'}
          </button>
        </div>
      </div>
    </div>
  );
}

/** 主界面顶部的更新提示横幅；无可提示内容时渲染为空。 */
function UpdateNotice() {
  const api = useUpdate();
  const { status, error } = api;
  /** 用户点过「稍后」的 (阶段, 版本) 组合；阶段推进后横幅会再次出现 */
  const [acknowledged, setAcknowledged] = useState<string | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);

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
          ? `v${status.version} 已下载`
          : '正在安装更新';

  const hasNotes = status.notes !== null && status.notes.trim() !== '';
  /** 更新内容入口只在可读且可操作的阶段出现（available / ready） */
  const canViewNotes = hasNotes && (status.phase === 'available' || status.phase === 'ready');

  return (
    <>
      <div className="update-banner" role="status">
        <div className="update-banner-main">
          <span className="update-banner-title">
            ⬆ {title}
            {status.phase === 'available' && status.date !== null && (
              <span className="update-banner-meta">{status.date.slice(0, 10)}</span>
            )}
            {status.phase === 'ready' && (
              <span className="update-banner-meta">安装后自动重启 · 配置保留在本机</span>
            )}
          </span>
          {status.phase === 'downloading' && <UpdateProgress status={status} />}
          {error && <span className="update-banner-error">{error}</span>}
        </div>
        <div className="update-banner-actions">
          {canViewNotes && (
            <button className="update-btn ghost" onClick={() => setDialogOpen(true)}>
              更新内容
            </button>
          )}
          {status.phase === 'available' && (
            <button className="update-btn primary" onClick={api.download}>
              下载更新
            </button>
          )}
          {status.phase === 'ready' && (
            <button className="update-btn primary" onClick={api.install}>
              立即安装并重启
            </button>
          )}
          {status.phase === 'installing' && (
            <span className="update-hint">应用即将自动重启…</span>
          )}
          {(status.phase === 'available' || status.phase === 'ready') && (
            <button className="update-btn ghost" onClick={() => setAcknowledged(key)}>
              稍后
            </button>
          )}
        </div>
      </div>
      {dialogOpen && canViewNotes && (
        <UpdateDialog status={status} api={api} onClose={() => setDialogOpen(false)} />
      )}
    </>
  );
}

export default UpdateNotice;
