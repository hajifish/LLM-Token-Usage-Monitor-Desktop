import type { ProviderStatus, QuotaInfo, UsageSummary } from '../hooks/useUsage';

// Official brand logos downloaded from provider websites (stored in public/providers/)
const BRAND_LOGOS: Record<string, string> = {
  DeepSeek: '/providers/deepseek.png',
  Zhipu: '/providers/zhipu.png',
  'Kimi API': '/providers/kimi.png',
  OpenAI: '/providers/openai.png',
  Anthropic: '/providers/anthropic.png',
  Codex: '/providers/openai.png',
  'Claude Code': '/providers/anthropic.png',
  'Kimi Code': '/providers/kimi.png',
  Qoder: '/providers/qoder.png',
};

function BrandLogo({ name }: { name: string }) {
  const src = BRAND_LOGOS[name];
  if (src) {
    return <img src={src} alt={name} className="brand-logo-img" />;
  }
  // Fallback: colored letter badge
  const initial = name.charAt(0).toUpperCase();
  return <span className="brand-logo">{initial}</span>;
}

function CardHead({
  name,
  alias,
  badge,
}: {
  name: string;
  alias: string;
  badge?: { text: string; cls: string };
}) {
  return (
    <div className="p-card-head">
      <span className="p-card-brand">
        <BrandLogo name={name} />
        <span className="p-card-name">{alias}</span>
      </span>
      {badge && <span className={`p-card-badge ${badge.cls}`}>{badge.text}</span>}
    </div>
  );
}

// 0 = ok, 1 = yellow (low), 2 = red (empty)
function warningLevel(p: ProviderStatus): number {
  if (!p.balance) return 0;
  const v = p.balance.available_balance;
  if (p.balance.currency === 'USD') return 0;
  if (v <= 0) return 2;
  if (p.balance.currency === '%') return v < 5 ? 1 : 0;
  return v < 10 ? 1 : 0;
}

function QuotaRow({ q }: { q: QuotaInfo }) {
  const pct = Math.max(0, Math.min(100, q.remaining_percent));
  const meterClass = pct <= 0 ? 'meter-red' : pct <= 20 ? 'meter-orange' : 'meter-green';
  return (
    <div className="quota-row">
      <div className="quota-row-top">
        <span className="detail-key quota-name">
          {q.plan_label && <span className="quota-plan-tag">{q.plan_label}</span>}
          {q.name}
        </span>
        <span className="quota-pct">{pct.toFixed(0)}%</span>
      </div>
      <div className="quota-row-bottom">
        <span className={`quota-meter ${meterClass}`}>
          <span className="quota-meter-fill" style={{ width: `${pct}%` }} />
        </span>
        {q.reset_time && (
          <span className="reset-hint" title={q.reset_time}>
            {q.reset_time}
          </span>
        )}
      </div>
    </div>
  );
}

function ProviderCard({ p }: { p: ProviderStatus }) {
  if (p.error) {
    return (
      <div className="p-card p-card-error">
        <CardHead name={p.name} alias={p.alias} badge={{ text: '异常', cls: 'red' }} />
        <p className="p-card-error-msg">{p.error}</p>
      </div>
    );
  }

  const { balance, usage, quota_infos } = p;
  const isPercent = balance?.currency === '%';
  const isUsd = balance?.currency === 'USD';
  const warn = warningLevel(p);
  const warnClass = warn === 2 ? 'p-card-warn-red' : warn === 1 ? 'p-card-warn-yellow' : '';

  return (
    <div className={`p-card ${warnClass}`.trim()}>
      {/* Card header with brand logo */}
      <CardHead name={p.name} alias={p.alias} />

      {/* Main number: left-aligned, label on the same line */}
      {balance && (
        <div className="p-card-main">
          <span className={`p-card-big ${isUsd ? 'accent-blue' : 'accent-green'}`}>
            {isPercent
              ? `${balance.available_balance.toFixed(0)}%`
              : isUsd
                ? `$${balance.available_balance.toFixed(2)}`
                : `¥${balance.available_balance.toFixed(2)}`}
          </span>
          <span className="p-card-label">
            {isPercent ? '配额剩余' : isUsd ? '本月已花费' : '剩余余额'}
          </span>
        </div>
      )}

      {/* Balance breakdown: single compact line */}
      {balance && !isPercent && !isUsd && (
        <div className="p-card-balance-split">
          赠 ¥{balance.voucher_balance.toFixed(2)} · 现 ¥{balance.cash_balance.toFixed(2)}
        </div>
      )}

      {/* Details section */}
      <div className="p-card-details">
        {/* Today usage */}
        {usage && usage.total_tokens > 0 && (
          <>
            <div className="p-card-detail-row">
              <span className="detail-key">今日 Token</span>
              <span className="detail-val">{usage.total_tokens.toLocaleString()}</span>
            </div>
            {usage.today_requests != null && usage.today_requests > 0 && (
              <div className="p-card-detail-row">
                <span className="detail-key">调用次数</span>
                <span className="detail-val">{usage.today_requests.toLocaleString()}</span>
              </div>
            )}
          </>
        )}

        {/* Quota infos: name + percent, then mini meter + reset time */}
        {quota_infos &&
          quota_infos.map((q: QuotaInfo, index: number) => (
            <QuotaRow key={`${q.plan_label ?? ''}-${q.name}-${index}`} q={q} />
          ))}

        {/* Nothing to show (e.g. API key accepted but no data yet) */}
        {!balance && !(usage && usage.total_tokens > 0) && !quota_infos?.length && (
          <p className="p-card-empty-hint">暂无数据</p>
        )}
      </div>
    </div>
  );
}

interface UsagePanelProps {
  summary: UsageSummary | null;
  loading: boolean;
  refreshing: boolean;
  error: string | null;
  refresh: () => void;
}

function UsagePanel({ summary, loading, refreshing, error, refresh }: UsagePanelProps) {
  if (loading) {
    return (
      <div className="usage-panel loading">
        <div className="loading-spinner" />
        <span className="loading-text">加载中...</span>
      </div>
    );
  }

  if (error) {
    return (
      <div className="usage-panel empty">
        <div className="empty-icon">!</div>
        <p className="empty-title">获取数据失败</p>
        <p className="empty-hint">{error}</p>
        <button className="action-btn" onClick={refresh}>
          重试
        </button>
      </div>
    );
  }

  // Disabled providers are managed in Settings; keep the main view focused on active ones.
  const visibleProviders = summary ? summary.providers.filter((p) => p.enabled) : [];

  if (!summary || summary.providers.length === 0) {
    // 真正零配置才显示添加指引
    return (
      <div className="usage-panel empty">
        <div className="empty-icon">+</div>
        <p className="empty-title">暂无服务商</p>
        <p className="empty-hint">请前往设置添加 API Key</p>
      </div>
    );
  }

  if (visibleProviders.length === 0) {
    // 有配置但全部禁用：不能误导为「未配置」
    return (
      <div className="usage-panel empty">
        <div className="empty-icon">–</div>
        <p className="empty-title">所有供应商已禁用</p>
        <p className="empty-hint">可在设置中启用</p>
      </div>
    );
  }

  return (
    <div className={`usage-panel${refreshing ? ' is-refreshing' : ''}`}>
      <div className="card-grid">
        {visibleProviders.map((p, i) => (
          // alias 无自定义时回退显示名，同类型无别名双账号会重键；追加下标兜底
          <ProviderCard key={`${p.name}-${p.alias}-${i}`} p={p} />
        ))}
      </div>
    </div>
  );
}

export default UsagePanel;
