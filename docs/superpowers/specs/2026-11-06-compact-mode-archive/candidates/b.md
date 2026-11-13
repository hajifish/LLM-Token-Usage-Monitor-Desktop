# 精简模式（Compact Mode）设计

- 日期：2026-11-06
- 状态：architect Phase B 候选设计（synthesis 前稿）
- 范围：仅设计。本文不含已实施代码。

## Problem

主页 720×600 占屏太大。用户要一个"极度精简"形态：供应商尽量一行、窗口左右拉宽、自动贴屏幕最上方（"挖孔屏扩大"观感），只突出供应商图标名字、剩余额度、额度重置时间，其余信息缩小；必须有退出路径并恢复窗口几何；设置入口被替换后仍需可达（托盘菜单已有"设置"）。

Phase A 摸底给出的硬约束（设计必须绕开或显式处置）：

1. **全仓库零运行时窗口几何代码**。尺寸只由 `tauri.conf.json:14-29` 静态声明；唯一运行时窗口属性变更是 `background.rs:16-42` 的 `apply_window_effects`（Rust 命令模板）。前端从未引用 `@tauri-apps/api/window`。
2. **capabilities 最小化**（`capabilities/default.json` 只有 core:default + event + start-dragging + log）。JS 直调 setSize/setPosition 需要扩权限；走 Rust 命令不需要。
3. **配置纪律**：AppConfig 在 Rust 侧（`models.rs:67-90`），落盘唯一通道 `commands::persist_config`（`commands.rs:67-94`）；前端纪律是"get_config 读 fresh → 只改一个字段 → save_config → invoke apply → 失败 UI 回弹"（样板 `useFrozenBackground.ts:64-90`）。Rust 侧直写配置的先例是 `scheduler.rs:433-456` 的 `set_refresh_interval`（读 clone → 改一字段 → persist_config）。
4. **窗口生命周期**：setup（`lib.rs:57-82`）→ 窗口 visible:false → `RunEvent::Ready`（`lib.rs:109-127`）无 `--minimized` 才 show+focus；CloseRequested = hide。autostart 固定传 `--minimized`。
5. **冻结背景层**（`app.css:902-911`）`background-size:100% 100%`，窗口一旦变宽矮，拉伸即畸变。
6. **告警判级双实现前科**：主页 `warningLevel`（`UsagePanel.tsx:47-54`，只看 balance）与 Rust `quota_warning_level`（`tray.rs:102-116`，percent 型按主配额）曾因口径分叉漏告警（`tray.rs:882-895` 的回归测试即事故记录）。新展示面不得引入第三份漂移口径。
7. **`reset_at` 类型缺口**：Rust `QuotaInfo` 有（`models.rs:138-141`，ISO 8601 UTC），TS `types.ts:16-25` 未声明。补类型即可做实时倒计时，零后端改动。
8. **2026-10-31 spec 既定**："固定紧凑尺寸、不做高度自适应"（`docs/superpowers/specs/2026-10-31-main-window-compact-design.md`）；"禁用供应商主页不显示"。
9. **单 CSS 文件**（`app.css`，1101 行扁平全局）、无图标库（手写 feather 风 inline SVG）、无 JS 测试 runner（verify 走浏览器 shim 驱动）。

本设计是对这些约束的第一次"运行时几何"实践，仓库无先例可抄，`apply_window_effects` 是唯一模板。

## 用户视角 UX 流程

**进入**。正常模式主页右上角，原"设置"齿轮位置换成"精简模式"按钮（feather `minimize-2` 图标：两角向内收缩的一对箭头，与退出用的 `maximize-2` 成对，见 Feather Icons 图集；title 提示"精简模式"）。点击后窗口瞬间（无动画，macOS set_size 即时生效）变为：宽约为当前屏幕可用区 80%、高 64 逻辑像素的横条，水平居中、顶边贴住菜单栏下沿，悬浮在所有窗口之上。条内一行供应商芯片。

**芯片**。每家启用供应商一枚：品牌 logo + 名字 + 主值（大号、按余量紧张度着色：绿/黄/红，美元花费型蓝色）+ 小字标签（"配额剩余"/"剩余余额"/"本月已花费"）+ 重置倒计时（小字，"3D4H" 格式，每 30 秒走动）。主值与菜单栏托盘标题同源同值（同为 primary_quota 口径），屏幕上下两处不会出现两个数字。异常供应商芯片显示"异常"（红）；无数据显示"–"。供应商多时名字先省略号截断，主值永不截断。

**条的右端**。数据时间小字（时:分:秒，同正常模式顶栏口径）+ 展开按钮（feather `maximize-2`）。

**拖动**。整条空白区域是拖动区（沿用 `data-tauri-drag-region` 与 `core:window:allow-start-dragging` 既有权限）。红绿灯保留在条左端（Overlay 标题栏不动，左侧 76px 让位，同现有 `.app-header` 手法），关闭红点仍是"隐藏窗口"的熟悉路径。

**退出**。两条路：(1) 点条的展开按钮，窗口几何复原为进入前的大小位置；(2) 托盘菜单"设置"，自动退出精简并以正常窗口打开设置页（这是齿轮被替换后的兜底路径，也是"卡在精简形态"时的安全出口）。

**记忆**。模式写入配置，重启（含开机自启后从托盘打开）直接以精简形态出现，无闪变。

**异常形态**。加载中（居中"加载中..."）、获取失败（居中"获取数据失败，点击重试"，点击触发刷新）、零配置（"暂无服务商"）、全部禁用（"所有供应商已禁用"）。展开按钮在所有形态下始终可见。更新横幅（UpdateNotice）与右下角版本号在精简形态下不渲染。

## Usage（caller's view）

README 一句话：`useCompactMode()` 给你 `{ compact, enter, exit }`；`compact === true` 时渲染 `<CompactBar/>` 代替 usage 视图；几何、置顶、持久化全部藏在一次 `invoke('set_compact_mode')` 里，前端不碰窗口 API。

### 调用点 1：App.tsx 渲染分支与按钮替换

```tsx
// src/App.tsx（改动区：L14-18 状态、L24-35 navigate 监听、L122-140 按钮替换、L45-167 渲染树）
function App() {
  const [view, setView] = useState<'usage' | 'settings' | 'providers' | 'skills'>('usage');
  const { summary, loading, refreshing, error, refresh, refreshAfterProviderChange } = useUsage();
  const background = useFrozenBackground();
  const compactMode = useCompactMode();          // ← 新增
  const [appVersion, setAppVersion] = useState<string | null>(null);

  // navigate 监听（L24-35）增设退出：托盘「设置」是齿轮被替换后的唯一设置入口，
  // 必须以正常窗口打开设置页
  useEffect(() => {
    const unlisten = listen<string>('navigate', (event) => {
      if (event.payload === 'settings') {
        if (compactMode.compact) void compactMode.exit();   // 不 await：几何跳变与视图切换并发，容一帧过渡缝
        setView('settings');
      } else if (event.payload === 'usage') {
        setView('usage');
      }
    });
    return () => { unlisten.then((fn) => fn()); };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- compactMode.exit 引用稳定
  }, [compactMode.compact]);

  const compact = compactMode.compact;

  return (
    <ErrorBoundary>
      {background.imageUrl && <div className="background-frozen" style={{ backgroundImage: `url(${background.imageUrl})` }} />}
      {/* compact === null（启动形态未决）首帧空渲染：Rust 已按配置定好几何，
          此处渲染错形态视图会有一帧"正常视图挤进横条"的畸变 */}
      {compact === null ? null : compact ? (
        <CompactBar
          summary={summary}
          loading={loading}
          refreshing={refreshing}
          error={error}
          refresh={refresh}
          onExit={compactMode.exit}
        />
      ) : (
        <div className="app">
          {view === 'usage' ? (
            <div className="usage-view">
              <div className="app-header" data-tauri-drag-region>
                {/* …header-time / 刷新 / Skills / 供应商 原样… */}
                {/* 原 L122-140 齿轮按钮整块替换为： */}
                <button className="settings-icon-btn" onClick={compactMode.enter} title="精简模式">
                  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                       strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    {/* feather minimize-2：两角向内收缩，与退出按钮 maximize-2 成对 */}
                    <polyline points="4 14 10 14 10 20"></polyline>
                    <polyline points="20 10 14 10 14 4"></polyline>
                    <line x1="14" y1="10" x2="21" y2="3"></line>
                    <line x1="3" y1="21" x2="10" y2="14"></line>
                  </svg>
                </button>
              </div>
              <UpdateNotice />
              <UsagePanel summary={summary} loading={loading} refreshing={refreshing} error={error} refresh={refresh} />
            </div>
          ) : view === 'settings' ? (
            <Settings onBack={() => setView('usage')} background={background} />
          ) : /* providers / skills 原样 */}
          {appVersion && !compact && <div className="version-tag">v{appVersion}</div>}
        </div>
      )}
    </ErrorBoundary>
  );
}
```

要点：`compact === true` 时不渲染 `.app` 外壳、UpdateNotice、version-tag；精简形态下 `view` 恒为 `'usage'`（进入设置/供应商页必先退出，不变式见 Shape）。

### 调用点 2：CompactBar（新组件）

```tsx
// src/components/CompactBar.tsx
import { useState, useEffect } from 'react';
import BrandLogo from './BrandLogo';
import { compactChips, resetCountdownText } from '../compactModel';
import type { UsageSummary } from '../types';

interface CompactBarProps {
  summary: UsageSummary | null;
  loading: boolean;
  refreshing: boolean;
  error: string | null;
  refresh: () => void;
  onExit: () => void;
}

export default function CompactBar({ summary, loading, refreshing, error, refresh, onExit }: CompactBarProps) {
  // 倒计时分钟级精度：30s 走一格足够，避免秒级重渲
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const t = setInterval(() => setNow(new Date()), 30_000);
    return () => clearInterval(t);
  }, []);

  const chips = summary ? compactChips(summary) : [];

  return (
    <div className={`compact-bar${refreshing ? ' is-refreshing' : ''}`} data-tauri-drag-region>
      {loading ? (
        <span className="compact-status">加载中...</span>
      ) : error ? (
        <button className="compact-status is-action" onClick={refresh}>获取数据失败，点击重试</button>
      ) : chips.length === 0 ? (
        <span className="compact-status">
          {summary && summary.providers.length > 0 ? '所有供应商已禁用' : '暂无服务商'}
        </span>
      ) : (
        chips.map((c) => (
          <div key={c.key} className={`compact-chip${c.tension > 0 ? ` warn-${c.tension}` : ''}`}>
            <BrandLogo name={c.name} />
            <span className="compact-name">{c.alias}</span>
            <span className={`compact-value${c.unit === 'usd' ? ' accent-blue' : ''}`}>
              {c.value ?? (c.error ? '异常' : '–')}
            </span>
            <span className="compact-label">{c.label}</span>
            {(c.resetAt || c.resetFallback) && (
              <span className="compact-reset">{resetCountdownText(c.resetAt, c.resetFallback, now)}</span>
            )}
          </div>
        ))
      )}
      <span className="compact-flex-spacer" />
      {summary?.last_updated && (
        <span className="compact-time">{summary.last_updated.split(' ')[1] ?? summary.last_updated}</span>
      )}
      <button className="compact-exit-btn" onClick={onExit} title="退出精简模式">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor"
             strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          {/* feather maximize-2：与进入按钮 minimize-2 成对 */}
          <polyline points="15 3 21 3 21 9"></polyline>
          <polyline points="9 21 3 21 3 15"></polyline>
          <line x1="21" y1="3" x2="14" y2="10"></line>
          <line x1="3" y1="21" x2="10" y2="14"></line>
        </svg>
      </button>
    </div>
  );
}
```

### 调用点 3：useCompactMode（新 hook）

```ts
// src/hooks/useCompactMode.ts
import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { AppConfig } from '../types';

export interface CompactMode {
  /** null = 启动形态未决（get_config 未返回）。此时 App 不渲染任何视图。 */
  compact: boolean | null;
  enter: () => void;
  exit: () => void;
}

export function useCompactMode(): CompactMode {
  const [compact, setCompact] = useState<boolean | null>(null);

  // 启动：与 Rust setup 阶段读同一份配置（Rust 已按它定好窗口几何），
  // 前端只决定首帧渲染哪个视图。读失败按正常形态兜底。
  useEffect(() => {
    let alive = true;
    invoke<AppConfig>('get_config')
      .then((c) => { if (alive) setCompact(c.compact_mode); })
      .catch(() => { if (alive) setCompact(false); });
    return () => { alive = false; };
  }, []);

  // 乐观翻转（内容先变，几何随 invoke 到位，容一帧缝）；
  // 失败回滚：UI 复原 + 反向 invoke 对齐后端（命令幂等，分析见设计 Shape 节）
  const apply = useCallback(async (next: boolean) => {
    setCompact(next);
    try {
      await invoke('set_compact_mode', { enabled: next });
    } catch (e) {
      console.error(`精简模式切换失败（${next}），回滚:`, e);
      setCompact(!next);
      invoke('set_compact_mode', { enabled: !next }).catch(() => { /* 双败：下次点击幂等收敛 */ });
    }
  }, []);

  const enter = useCallback(() => { void apply(true); }, [apply]);
  const exit = useCallback(() => { void apply(false); }, [apply]);
  return { compact, enter, exit };
}
```

### 调用点 4：Rust 命令（前端唯一新增 IPC）

```ts
invoke('set_compact_mode', { enabled: true });   // 进入
invoke('set_compact_mode', { enabled: false });  // 退出（幂等：重复调用无副作用）
```

## Shape

### 决策表（任务要求的 9 项显式决策）

| # | 决策点 | 决策 | 理由 |
| --- | --- | --- | --- |
| 1 | 状态放哪 | **AppConfig 持久化** `compact_mode: bool`（serde default false）；启动时 Rust 在 setup 阶段（窗口尚 hidden）直接以精简几何创建，无闪变；`--minimized` 自启不受影响（几何先行，托盘唤起时即横条） | 齿轮被替换说明这是常用主形态而非偶发玩法；Rust 侧先行应用同时解决了"JS 晚于 Ready show 导致的错形闪现"（若由前端 mount 后再 invoke，必有一帧正常几何可见） |
| 2 | 几何所有权与路径 | **单一 Rust 命令** `set_compact_mode(enabled)`（照 `apply_window_effects` 模板，落在专属模块 `compact.rs`）；不扩 capabilities；几何参数：宽 = 当前显示器 work_area 宽 × 0.8（下限 560 逻辑 px），高 = 64 逻辑 px，水平居中于可用区，y = work_area 顶（macOS 菜单栏下沿由 `Monitor::work_area()` 给出，已验证 tauri 2.11.6 存在）；显示器取 `current_monitor()`，None 回退 `primary_monitor()`；原几何 = 进入时捕获 `outer_position + inner_size`（本会话内存），退出恢复；无存档时回退"setup 时观察到的创建尺寸 + 当前可用区居中"（创建尺寸源自 tauri.conf 运行时观察，非手写常量） | 窗口几何只有一个写者（per 单一所有权）；monitor/work_area 是原生信息，属 Rust；JS 直调需扩权且解决不了启动时序 |
| 3 | 横条窗口属性 | 红绿灯/Overlay 标题栏**保留不动**（左侧 76px 让位，同 `.app-header`），不调 `set_decorations`（规避未验证风险 2）；整条为拖动区（data-tauri-drag-region + 子元素 pointer-events:none，按钮除外）；`set_always_on_top(true/false)` 随进入/退出切换 | HUD 要悬浮；红绿灯保留兼得原生关闭路径与零风险 |
| 4 | 精简行信息模型 | 每供应商一枚芯片：logo（复用 BRAND_LOGOS）+ alias + **一个主值** + 微标签 + 重置倒计时。主值口径：percent 型取 `ProviderStatus.primary_quota.remaining_percent`（新字段，Rust 侧填充，与托盘 `aggregate_value` 同源），其余取 `balance.available_balance`；多配额窗口只显示主窗口（primary_quota），其余窗口留在正常模式；告警着色与**显示值同源**（percent 按主窗口 remaining_percent，USD 恒不告警，阈值 ≤0 红 / <5 黄（percent）/ <10 黄（CNY），即 `quota_warning_level` 语义）；重置时间用 `reset_at`（ISO）前端每 30s 实时倒计时，`reset_at` 缺失/不可解析回退 `reset_time` 预格式化文本，已过期不显示（不回退陈旧文本，对齐 `tray.rs:192-200` 语义） | 芯片主值与托盘标题同源，屏幕两处不出现两个数；选择逻辑只在 Rust 一处（见决策 4 的替代方案） |
| 5 | 空态/错误态 | 4 态全部保留语义、压缩呈现：loading 居中"加载中..."；error 居中"获取数据失败，点击重试"（可点，触发 refresh）；零配置"暂无服务商"（正常模式空态文案同步改：设置按钮没了，指引改为"点击右上角 ＋ 添加供应商"）；全禁用"所有供应商已禁用"。芯片级：error → 值位显示"异常"（红），无数据 → "–"。展开按钮恒在 | 正常模式 UsagePanel 4 早退分支（`UsagePanel.tsx:169-214`）语义等价映射 |
| 6 | 退出交互 | (a) 条右端展开按钮（maximize-2）；(b) 托盘"设置"navigate 自动退出。不做 Esc 快捷键、不做托盘"退出精简"菜单项（动态菜单项违反托盘一次性装配纪律） | 两条路够了；条恒置顶可点，不存在够不到退出的卡死态 |
| 7 | 冻结背景层 | 精简形态下 CSS 覆盖 `background-size: cover; background-position: center top`（外层加 `.app-compact` 类），**不动材质**（不反向调 apply_window_effects） | 拉伸（100% 100%）在 15:1 横条上灾难性畸变；cover 裁剪保住"透出桌面"意图；材质启停保持 useFrozenBackground 单一所有者（避免第二个材质调用者造成状态分叉） |
| 8 | 组件/模块落点 | 新增 `compact.rs` / `useCompactMode.ts` / `CompactBar.tsx` / `compactModel.ts` / 抽出 `BrandLogo.tsx`；CSS 追加"精简模式"一节进 `app.css`（单文件惯例）；shim 补 `set_compact_mode` handler + fixture 字段 + fireNavigate 助手；verify 新增 compact-mode 用例 | 模块地图见下 |
| 9 | 明确不做 | 窗口位置跨启动持久化（全应用本就不持久化位置）；隐藏红绿灯；高度自适应（延续 2026-10-31 spec 决策，横条固定 64px 是新决策不与之冲突）；多行换行（严格一行，名字先截断）；托盘菜单改动；主页面卡片布局改动；统一主页 warningLevel 与新口径（另开后续项）；精简形态下的设置页内嵌 |

### 数据结构

**TS 类型草图**（`src/types.ts` 增量；字段名与 Rust serde 严格一致）：

```ts
export interface QuotaInfo {
  // …现有字段不动…
  reset_time: string | null;
  /** 重置时刻（ISO 8601 UTC），实时倒计时换算用；缺失回退 reset_time 文本。 */
  reset_at: string | null;        // ← 新增（wire 上已有，Rust models.rs:138-141）
  plan_label: string | null;
}

export interface ProviderStatus {
  // …现有字段不动…
  /** 订阅型主窗口（Rust tray::primary_quota 选出：剩余%最低，差<0.5 并列取重置更近）。
   *  percent 型供应商才有；与托盘标题/菜单主值同源。 */
  primary_quota: QuotaInfo | null;   // ← 新增（#[serde(default)]，旧载荷兼容）
}

export interface AppConfig {
  // …现有字段不动…
  /** 精简模式：主页以贴顶横条呈现（HUD 形态）。启动时按此值定窗口几何。 */
  compact_mode: boolean;          // ← 新增（serde default false）
}
```

**精简行模型**（`src/compactModel.ts`，零依赖纯函数模块，与 types.ts/providerMeta.ts 同风格）：

```ts
import type { ProviderStatus, QuotaInfo, UsageSummary } from './types';

export type Tension = 0 | 1 | 2;   // 0 平静 / 1 黄（低）/ 2 红（空）

/** 一枚供应商芯片的全部展示事实。value 与 tension 必须同源（事故教训 tray.rs:882-895）。 */
export interface CompactChip {
  key: string;                     // `${name}-${alias}-${i}`（同 UsagePanel 键口径）
  name: string;                    // 类型名，BrandLogo 解析键
  alias: string;                   // 显示名（后端回退恒非空）
  value: string | null;            // '62%' | '¥42.50' | '$12.34' | null(无数据)
  unit: 'percent' | 'cny' | 'usd'; // 决定着色族（usd 蓝点缀）
  label: string;                   // 配额剩余 | 剩余余额 | 本月已花费
  tension: Tension;                // 按 value 的数值判级，不按 balance
  resetAt: string | null;          // primary_quota.reset_at（ISO），实时倒计时
  resetFallback: string | null;    // primary_quota.reset_time 文本兜底
  error: string | null;
}

/** summary → 芯片列表（过滤 enabled；percent 取 primary_quota，其余取 balance）。纯函数。 */
export function compactChips(summary: UsageSummary): CompactChip[] {
  // not implemented: 映射 + 过滤；tension 判级见决策 4；value 格式化对齐托盘 tray_value
}

/** 重置倒计时文本。策略与 Rust quota_countdown_compact（tray.rs:192-200）一致：
 *  resetAt 可解析且未过期 → '12M'/'1H4M'/'6D'/'3D4H'；已过期 → null（不回退陈旧文本）；
 *  resetAt 缺失/不可解析 → fallback 文本。纯函数。 */
export function resetCountdownText(
  resetAt: string | null,
  fallback: string | null,
  now: Date,
): string | null {
  // not implemented: 分钟/小时/天整单位省略低位（与 Rust countdown_compact 同格式，
  // 格式一致性行为由 verify 用例以固定 reset_at 锁定）
}
```

**Rust 结构/命令草图**（`src-tauri/src/compact.rs`，模块定位注释照 `background.rs` 风格：本模块只负责精简形态的窗口几何这一件事）：

```rust
//! 精简模式窗口几何：进入 = 宽矮横条贴可用区顶端 + 置顶；退出 = 恢复进入前几何。
//! 几何变换只走本模块一条路（进入/退出/启动应用），避免多处改几何导致状态分叉。

use std::sync::Mutex;
use tauri::{LogicalSize, PhysicalPosition, PhysicalSize, WebviewWindow};

/// 横条高度（逻辑 px）。CSS 侧 .compact-bar 用 height:100vh 自适应，无跨语言常量同步。
const BAR_HEIGHT: f64 = 64.0;
/// 横条宽 = 可用区宽 × 比例，下限保底（极小屏也不至于挤成一团）。
const BAR_WIDTH_RATIO: f64 = 0.8;
const BAR_MIN_WIDTH: f64 = 560.0;

/// 进入前捕获的正常几何（物理像素；restore = set_size + set_position 原样写回）。
pub struct SavedGeometry {
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
}

/// 精简形态会话态。config.compact_mode 是"当前是否精简"的唯一事实源，
/// 本结构只保管恢复所需的几何，不保管 active 标志（避免第二份状态）。
pub struct CompactState {
    /// Some = 本会话从正常形态进入过，退出时恢复；None = 启动即精简，退出回默认居中。
    /// 进入路径仅在 None 时写入：失败回滚后的重进不得用横条几何覆盖有效存档（收敛分析见设计文档）。
    saved: Mutex<Option<SavedGeometry>>,
    /// setup 阶段观察到的窗口创建尺寸 = 正常形态默认尺寸。
    /// 源头是 tauri.conf.json（单一事实源），运行时观察取得，无手写 720×600 常量。
    default_size: PhysicalSize<u32>,
}

/// 横条目标几何（纯函数，单测锚点）：宽 = 可用区宽×RATIO（≥MIN，逻辑 px），
/// 高 = BAR_HEIGHT 逻辑 px；水平居中于可用区；y = 可用区顶（菜单栏下沿含在 work_area 里）。
/// 位置用物理 px 计算（混合 DPI 多屏下逻辑全局坐标有歧义，物理坐标无歧义）；
/// 尺寸用逻辑 px（感知一致：任何 scale 下横条观感相同）。
fn bar_geometry(
    wa_pos: PhysicalPosition<i32>,
    wa_size: PhysicalSize<u32>,
    scale: f64,
) -> (LogicalSize<f64>, PhysicalPosition<i32>) {
    // not implemented:
    // wa_w_logical = wa_size.width / scale
    // bar_w = max(wa_w_logical * BAR_WIDTH_RATIO, BAR_MIN_WIDTH)
    // x = wa_pos.x + round((wa_size.width - bar_w * scale) / 2)
    // y = wa_pos.y
}

/// 默认几何（纯函数）：无存档时的退出目标 = 创建尺寸在当前可用区居中。
fn centered_default(
    default_size: PhysicalSize<u32>,
    wa_pos: PhysicalPosition<i32>,
    wa_size: PhysicalSize<u32>,
) -> PhysicalPosition<i32> {
    // not implemented
}

/// setup 阶段装配（lib.rs 调用，窗口尚 visible:false）：
/// 捕获创建尺寸 → manage(CompactState) → compact_mode 为真则直接应用横条几何
/// （不写 saved：没有"进入前"几何可存）。先几何后 Ready show，全链路无闪变。
pub fn setup(app: &tauri::AppHandle, compact_mode: bool) -> Result<(), String> {
    // not implemented
}

/// 应用横条形态：current_monitor（回退 primary_monitor）→ bar_geometry →
/// set_always_on_top(true) → set_size → set_position。
fn apply_bar(window: &WebviewWindow) -> Result<(), String> {
    // not implemented
}

/// 恢复正常形态：saved 有则原样写回，无则 default_size + 居中；set_always_on_top(false)。
fn restore_normal(window: &WebviewWindow, state: &CompactState) -> Result<(), String> {
    // not implemented
}

/// 进入/退出精简模式（前端唯一新增 IPC）。一次调用完成几何 + 持久化，
/// 幂等（config 已是目标态 → Ok 原样返回；重复点击不会用横条几何覆盖 saved）。
///
/// 顺序：进入 =（saved 为 None 时）捕获几何 → persist(true) → apply_bar；
///       退出 = persist(false) → restore_normal → 清空 saved。
/// 任一步失败的收敛性见设计文档"失败与幂等分析"。
#[tauri::command]
pub async fn set_compact_mode(
    window: WebviewWindow,
    app: tauri::AppHandle,
    config_state: tauri::State<'_, std::sync::Arc<tokio::sync::RwLock<crate::models::AppConfig>>>,
    health_state: tauri::State<'_, std::sync::Arc<tokio::sync::RwLock<crate::config::ConfigHealth>>>,
    compact_state: tauri::State<'_, CompactState>,
    enabled: bool,
) -> Result<(), String> {
    // not implemented:
    // if config_state.read().await.compact_mode == enabled { return Ok(()); }
    // if enabled && saved.is_none() { saved = capture(window)?; }
    // let mut cfg = config_state.read().await.clone();
    // cfg.compact_mode = enabled;
    // crate::commands::persist_config(&app, &config_state, &health_state, cfg).await?;  // 唯一落盘通道
    // if enabled { apply_bar(&window)?; } else { restore_normal(&window, &compact_state)?; }
}
```

lib.rs 装配（`lib.rs:57-82` setup 内、create_tray 之后）：

```rust
mod compact;                                     // + 顶部 mod 声明
// setup 内：
compact::setup(app.handle(), app_config.compact_mode)?;
// invoke_handler（lib.rs:91-106）追加：
compact::set_compact_mode,
```

**ProviderStatus.primary_quota 填充**（`scheduler.rs`，summary 组装处，单一位置）：

```rust
// poll_providers 组装完 provider_statuses 后统一回填（与 display_alias 回退同层）：
// percent 型且 quota_infos 非空 → primary_quota = tray::primary_quota(qs).cloned()
// 选择逻辑保持 tray.rs 一处定义，托盘/通知/精简条三方共用（消费点：tray.rs:203、
// notification.rs aggregate_value、前端 CompactBar）
```

**models.rs 增量**：

```rust
// AppConfig 增（L67-90 区域）：
/// 精简模式：主页以贴顶横条（HUD）呈现；启动时按此值应用窗口几何。
#[serde(default)]
pub compact_mode: bool,

// ProviderStatus 增（L146-156 区域）：
/// 订阅型主窗口（tray::primary_quota 选出）；percent 型供应商才有。
/// 与托盘标题/菜单主值同源，展示层（精简条）直接消费，不再各自选窗。
#[serde(default)]
pub primary_quota: Option<QuotaInfo>,
```

### 几何参数与时序（决策 2 的展开）

- **宽度**：`work_area 宽 × 0.8`。1512 逻辑宽的 MacBook 14" 得约 1210px，6 枚芯片（每枚约 170-190px）单行放得下；"左右适当拉大"与"挖孔屏扩大"的居中留边观感由此而来。下限 560 逻辑 px 兜极小屏。
- **高度**：64 逻辑 px。容得下 20px logo、17px 主值、12px 名字与拖动热区，同屏不遮内容区。
- **贴顶**：`y = work_area.position.y`。macOS 菜单栏高度（含刘海屏差异）由 `Monitor::work_area()` 天然扣除，**不需要**硬编码 24/25pt（该 API 在锁定版本 tauri 2.11.6 中存在，实测路径 `tauri::Monitor::work_area() -> &PhysicalRect<i32, u32>`，字段 `position`/`size` 公开，证据为本地 cargo registry 源码 `tauri-2.11.6/src/window/mod.rs:62-97`）。
- **选屏**：窗口当前所在屏（`current_monitor()`），拖到哪块屏进入就以哪块屏为基准；取不到回退主屏；都取不到报错给前端回滚。
- **原几何**：进入时 `outer_position()` + `inner_size()`（物理 px）存内存。不跨启动持久化（应用本就不持久化窗口位置；退出无存档时回"创建尺寸 + 当前可用区居中"）。

**启动时序**（决策 1 的展开）：

```
setup: config 加载 → compact::setup(cfg.compact_mode)
       ├─ 捕获创建尺寸（= 默认正常尺寸，源自 tauri.conf）
       ├─ compact_mode=true → apply_bar（窗口 visible:false，几何先行）
       └─ manage(CompactState)
Ready: should_show_window → show + focus（此刻几何已正确）
前端:  mount → get_config → compact 已知 → 首帧渲染对应视图（null 门槛先渲染空）
```

autostart `--minimized`：Ready 不 show；几何已在 setup 定好；托盘"打开主页"唤起即横条。前端 get_config 与 show 无竞态（mode 在隐藏期已解析）。

**失败与幂等分析**（make-operations-idempotent 的落实）。命令单步失败点逐一收敛：

| 失败点 | 状态 | 前端回滚 `invoke(!next)` 后 | 再点击目标按钮 |
| --- | --- | --- | --- |
| 进入时 persist 失败 | 几何未动、config=false、saved 可能已写入（值=正常几何，无害） | 幂等守卫直接 Ok，无操作 | 正常重进 |
| 进入时 apply_bar 失败 | config=true、窗口仍正常、saved=正常几何 | 走退出路径：恢复几何（已是正常，无观感变化）+ persist(false)，全对齐 | 正常重进 |
| 退出时 persist 失败 | 几何未动、config=true、saved 保留 | 幂等守卫 Ok，无操作 | 幂等 Ok（config 已 true），UI 正常 |
| 退出时 restore 失败 | config=false、窗口仍横条、saved=正常几何 | 走进入路径：**saved 为 Some 则跳过捕获**（防横条几何覆盖存档）→ apply_bar（已是横条，无变化）+ persist(true)，全对齐 | 点展开 → 退出路径恢复 |
| 双败 / 中途崩溃 | 上表某行的残留态 | 收敛同列 | 收敛同列 |

关键守卫两条：进入路径仅在 `saved.is_none()` 时捕获几何；幂等守卫以 config 为准。两者合起来使任何单点失败后的"再点一次"必然收敛（per make-operations-idempotent）。

**共享状态检查**（per separate-before-serializing-shared-state）：窗口几何的写者只有 compact.rs 一个（conf 静态声明是创建时的一次性输入，非运行时写者）。config 写者新增 set_compact_mode，但走既有唯一落盘通道 `persist_config`，与 `set_refresh_interval`（`scheduler.rs:433-456`）同一读-改-写模式，并发暴露面与现状相同，不扩大。前端 `compact` 状态是纯展示副本，失败回弹纪律与设置页各开关一致。

### 接口深度评估

公共面：1 个 Rust 命令（bool → Result）、1 个 hook（3 成员）、1 个组件（props 与 UsagePanel 同族 + onExit）、1 个纯函数模块、2 个增量字段（config 1 + payload 1）+ 1 个类型补齐（reset_at）。藏在其后：显示器/work_area 数学、菜单栏偏移、几何存档与恢复、置顶切换、持久化时序、失败收敛、主配额选择、双币种语义、倒计时格式策略、冻结背景裁剪。调用者学 5 个名字就能用，无需懂任何内部（deep module，非深调用链：从按钮到窗口跳变两跳）。无 wire 类型上公共面（hook 出 boolean，组件进 domain 类型）。

## Synthesis decision

（arena synthesis 填写，此处留空）

## Tradeoffs accepted

- **倒计时格式 TS/Rust 双实现**，换取横条内 30s 实时走动（后端 180s 轮询快照做不到）。格式（12M/1H4M/6D/3D4H）由 Rust 测试锁定，TS 侧行为由 verify 用例以固定 `reset_at` 断言文本锁定。接受两处维护义务。
- **保留红绿灯（左侧 76px 死区）**，换取完全规避 `set_decorations` 在 Overlay + hudWindow 下的未验证行为（grounding 风险 2）。视觉上损失约 9% 条宽。
- **冻结背景 cover 裁剪而非材质反向切换**，换取背景模块（useFrozenBackground + apply_window_effects）保持单一所有者。裁的是一张静态快照的上下边，可能露出轻微"条带感"，列实测项。
- **过渡的一帧缝**（内容先翻、几何随 invoke 到位，或反之），换取乐观 UI 与简单时序。macOS set_size 无动画，缝为一帧，接受。
- **不持久化窗口位置**，与全应用行为一致；启动即精简的用户退出后落在"当前屏居中"而非上次正常位置。
- **ProviderStatus 增字段**（payload 变大一点）而非新 IPC 命令，换取主配额选择逻辑全仓库一处定义。
- **正常模式零配置文案改口**（"前往设置" → "右上角 ＋"）：齿轮没了，诚实指路比维持旧文案重要。

## Alternatives considered

- **JS 直调窗口 API + capabilities 扩容**（`getCurrent().setSize/setPosition/setAlwaysOnTop`）：输在启动时序无解（JS 晚于 Ready show，持久化形态必闪一帧正常几何）、monitor/work_area 数学要进 TS、capabilities 扩大攻击面、几何出现第二个写者。接口更浅（权限表换一行配置），藏的能力更少。
- **纯前端会话态（不持久化）**：少一个配置字段与 setup 分支，但常用主形态每次启动要重点一次，且"启动即精简无闪变"仍需 Rust 应用几何，等于复杂度没省、体验变差。输在 experience-first。
- **第二窗口做 HUD**（新增小窗常驻 + 主窗隐藏）：形态上更"真 notch"，但仓库是单窗架构（conf、tray click flag、CloseRequested=hide、navigate、背景材质应用全部绑 main 窗口），第二窗要复制整套生命周期接线，托盘纪律每条显隐路径都要同步。同样的能力藏在现有窗口的模式切换后面，公共面小得多。输在 interface depth 与改动面。
- **前端读-改-写 save_config + apply 命令**（照 background_mode/tray_click 的既有样板）：两步非原子，失败要 UI 侧再发一次反向 save；前端为 1 个 bool 穿梭整份配置。单命令把几何存档、恢复、幂等、持久化全藏进 callee，且有 `set_refresh_interval` 的 Rust 侧直写先例。输在原子性与深度。
- **TS 侧复现 primary_quota 选择算法**：与 tray.rs 手工同步算法，正是 warningLevel 双实现漏告警事故（tray.rs:882-895）的同型风险。输在 hand-synced 逻辑漂移。

## Implementation reconciliation

（实施期间填写，此处留空）

## Open questions and risks

**产品口味题（已按默认推进，可一句话推翻）**：

1. 宽 80% / 高 64px / 居中贴顶，这三个数是首版猜测值，实机走查后微调是否可接受？
2. 横条悬浮置顶（always_on_top）是 HUD 直觉，但会盖住全屏内容，接受吗？（去掉是一行。）
3. 精简形态跨重启记忆已按"记住"设计；若期望"每次启动回正常"，改动是一个 serde 字段 + 删 setup 分支。

**实施中必须实测的项**（grounding 5 条风险的处置映射）：

1. **hudWindow 材质在宽矮横条的观感**（含系统圆角在 64px 高窗的表现）：未规避，列 real-app 走查首项。若材质在极端横条比下异常，退路是横条形态下临时切 frozen 背景 cover（本文决策 7 的 CSS 已就位）。
2. **Overlay + set_decorations 行为**：已规避（不调 set_decorations）。若视觉评审坚持藏红绿灯，须先原型验证再改。
3. **屏幕几何 API**：`work_area` 存在性已核（tauri 2.11.6 源码，前引）；**未验证**的是混合 DPI 双屏下物理坐标换算与拖到副屏进入的实际落位，须 real-app 双屏实测（无第二块屏则记录为已知限制）。
4. **resizable:false + 运行时 set_size**：macOS 程序化 setFrame 不受用户缩放闸限制（预期可行），仍列 real-app 验证；同时验证横条仍可拖动（drag region 与 resizable 无关，预期可行）。
5. **冻结背景层切换瞬间**：cover 裁剪方案已落 CSS，实测条带感是否可接受（见 tradeoffs）。

新增风险：**always_on_top 与 Accessory 策略、Spaces 切换的交互**未实测；**过渡一帧缝**在低性能机器上可能可见；**`PhysicalRect` 若未来 tauri 版本变动**（字段公开性）有编译期护栏（编译错误即知，无静默风险）。

## Next implementation step

先写 `compact.rs` 的 `bar_geometry`/`centered_default` 纯函数与 cargo 单测（几何核心先行，可独立验证），再接命令与 lib.rs 装配做一次 real-app 手动几何验证，通过后才动前端。

## 模块地图

| 文件 | 改动 |
| --- | --- |
| `src-tauri/src/compact.rs` | **新增**。CompactState/SavedGeometry、bar_geometry（纯函数+单测）、setup、apply_bar/restore_normal、`set_compact_mode` 命令 |
| `src-tauri/src/lib.rs` | `mod compact`；setup 内 `compact::setup(...)`（L57-82 区，create_tray 后）；invoke_handler 追加命令（L91-106） |
| `src-tauri/src/models.rs` | AppConfig 增 `compact_mode`（serde default，L67-90 区）；ProviderStatus 增 `primary_quota`（serde default，L146-156 区） |
| `src-tauri/src/scheduler.rs` | summary 组装处回填 `primary_quota`（poll_providers，L539 起；选择函数复用 `tray::primary_quota` L204-214，不动其定义） |
| `src-tauri/src/commands.rs` | 不改（`persist_config` 已 pub(crate)，compact.rs 直接复用） |
| `src/types.ts` | QuotaInfo 增 `reset_at`（L16-25）；ProviderStatus 增 `primary_quota`；AppConfig 增 `compact_mode` |
| `src/hooks/useCompactMode.ts` | **新增**。启动读配置定首帧形态；enter/exit 乐观翻转 + 失败回滚 |
| `src/components/CompactBar.tsx` | **新增**。横条视图（芯片行 / 4 空态 / 时间 / 展开按钮 / 30s 倒计时 ticker） |
| `src/compactModel.ts` | **新增**。CompactChip、compactChips、resetCountdownText（纯函数，零依赖） |
| `src/components/BrandLogo.tsx` | **新增**（从 `UsagePanel.tsx:4-24` 抽出 BRAND_LOGOS + BrandLogo；UsagePanel 改 import，防精简条够进面板内部） |
| `src/components/UsagePanel.tsx` | 删本地 BRAND_LOGOS/BrandLogo 改 import；零配置空态文案改"点击右上角 ＋ 添加供应商"（L194-203，齿轮已不存在） |
| `src/App.tsx` | L122-140 齿轮按钮替换为精简按钮（minimize-2）；接 useCompactMode；渲染分支（compact null 门槛 / CompactBar / 正常树）；navigate 'settings' 先退出精简（L24-35）；compact 下不渲染 UpdateNotice/version-tag |
| `src/styles/app.css` | 追加"精简模式"一节：`.compact-bar/-chip/-value/-name/-label/-reset/-status/-time/-exit-btn/-flex-spacer`、warn 着色、`.app-compact .background-frozen` cover 覆盖；复用既有 `.settings-icon-btn`、`.accent-blue`、`.brand-logo-img` |
| `.claude/skills/verify/mock/tauri-shim.js` | handlers 增 `set_compact_mode`（记 `state.compactCalls` + 改 mock config）；config fixture 增 `compact_mode:false`；usage fixture 的 quota 增 `reset_at`（固定未来时刻）与 ProviderStatus 级 `primary_quota`；增 `fireNavigate(payload)` 助手 |
| `.claude/skills/verify/features/compact-mode.md` | **新增**。浏览器道：齿轮缺席/新按钮在、进入后横条渲染、芯片值与 fixture 对齐（含固定 reset_at 的倒计时文本）、4 空态、展开退出回正常视图、fireNavigate('settings') 退出；real-app 道：真机几何/贴顶/拖动/退出恢复/双屏 |
