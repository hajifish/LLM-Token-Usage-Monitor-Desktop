# 精简模式（compact mode）设计 sketch

arena Phase B candidate。Phase A grounding：`.audit/compact-mode-grounding.md`（主 checkout）。本文只设计，不实现。

## Problem

TokenUsage 是 720×600 固定尺寸的 macOS 菜单栏常驻小窗，占屏太多。产品要一个"精简模式"：主页右上角设置按钮（`src/App.tsx:122-140`）换成精简入口；点击后窗口变成贴屏幕顶部的宽矮横条（"挖孔屏"隐喻），每供应商一行只留图标、名字、剩余额度、重置时间；必须有退出路径并恢复窗口几何。

约束（grounding 已核实）：

- 全仓库零运行时窗口几何代码；唯一先例是 `background.rs:16-42` 的 `apply_window_effects`（Rust 命令改窗口属性）。前端从未 import `@tauri-apps/api/window`；capabilities 只授 `core:window:allow-start-dragging`。
- 主页是 `App.tsx:15` 的 view 状态机条件渲染；数据由 Scheduler 轮询后经 `usage-updated` 推送，前端纯展示。
- 托盘侧已有全部"每供应商挑主值"的规则（`tray.rs`：`primary_quota` / `quota_warning_level` / `tray_value` / `countdown_compact`），但未通过 IPC 暴露；`QuotaInfo.reset_at`（models.rs:140-141）已在 wire payload 里，TS `types.ts` 未声明。
- 冻结背景层 `.background-frozen`（app.css:902-911）用 `background-size:100% 100%` 铺 720×600 截图，窗口一变比例即失真。
- 配置纪律：前端「get_config 读 fresh → 只改一个字段 → save_config → invoke apply 命令 → 失败回弹」，样板 `useFrozenBackground.setMode`（useFrozenBackground.ts:64-90，含防闪时序）。
- 历史决策（2026-10-31 紧凑化 spec）：固定紧凑尺寸、不做高度自适应。

## Usage（caller 视角）

### 用户视角 UX 流程

1. **进入**：主页顶栏原"设置"齿轮位置变成一个"精简模式"图标按钮（feather `minimize-2`，向内收拢的双箭头，title="精简模式"）。点击后：窗口原地变为贴屏幕顶部的宽矮横条（约屏幕宽 60%、高 140 逻辑 px，水平居中、贴菜单栏下方 8px），视图切换为"精简条"：左端 app logo（红绿灯避让区，沿用 76px 左距），中间供应商 chips 一行排布（可换行），右端刷新 + 退出图标按钮。条浮在所有窗口之上（alwaysOnTop）。设置入口按需求移除，仍可从托盘菜单"设置"进入。
2. **精简条上的阅读**：每个 chip = [品牌 logo] + 别名 + 主剩余值（着色）+ `·` + 重置倒计时（活跳，30s 步进；无 reset_at 时显示后端预格式化文本）。悬停 tooltip 看全量（错误全文、精确余额）。
3. **退出**：条右端 feather `maximize-2`（向外展开双箭头）按钮。点击恢复原窗口几何（720×600 + 原位置）、取消置顶、回到完整主页。精简条的两个空态（零配置/全禁用）上有"打开设置"按钮，点击同样先退出精简再切设置页。
4. **托盘交叉**：精简模式下托盘"打开主页"→ 显示的就是精简条；托盘"设置"→ 先退出精简恢复几何，再打开设置页。红绿灯关闭键照旧隐藏窗口（不退出应用），再次打开仍是精简形态（模式持久化）。
5. **重启**：精简模式是持久偏好（同 background_mode 先例）。下次启动直接以精简条形态出现（几何在窗口 show 之前套用，无闪变）；`--minimized` 自启同理——几何在 Ready 阶段对隐藏窗口套用，之后从托盘打开即是精简条。

### 代码级调用点（真实用法，类型草图由此反推）

**App.tsx 渲染分支**（现有 view 分支外再包一层 compact 门）：

```tsx
const compact = useCompactMode();
// 冻结背景图只在非精简时渲染（精简期间窗口材质接管，见 D7）
{background.imageUrl && compact.compact === false && (
  <div className="background-frozen" style={{ backgroundImage: `url(${background.imageUrl})` }} />
)}
{compact.compact !== false ? (
  compact.compact === true ? (
    <CompactBar
      summary={summary} loading={loading} refreshing={refreshing} error={error}
      refresh={refresh}
      onExit={compact.exit}
      onOpenSettings={async () => { await compact.exit(); setView('settings'); }}
    />
  ) : null /* 配置未读到（compact === null）：窗口此刻不可见，渲染空即可防闪 */
) : (
  <div className="usage-view">…现有分支：app-header / UpdateNotice / UsagePanel…</div>
)}
```

**按钮 onClick**（App.tsx:122-140 原设置按钮原地替换，类名沿用 `settings-icon-btn`）：

```tsx
<button
  className="settings-icon-btn"
  onClick={compact.enter}
  disabled={compact.busy}
  title="精简模式"
>
  {/* feather minimize-2，与相邻图标同规格 18×18 / viewBox 24 / strokeWidth 2 */}
  <svg …><polyline points="4 14 10 14 10 20"/><polyline points="20 10 14 10 14 4"/><line x1="14" y1="10" x2="21" y2="3"/><line x1="3" y1="21" x2="10" y2="14"/></svg>
</button>
```

托盘 `navigate` 监听（App.tsx:24-35）增加精简分支：

```tsx
if (event.payload === 'settings') {
  await compact.exit();      // 已是普通形态时 exit 幂等直通
  setView('settings');
}
```

**Rust 命令签名**（调用方 = 前端 hook；startup = lib.rs Ready 复用同一实现）：

```rust
#[tauri::command]
pub async fn set_compact_mode(window: tauri::WebviewWindow, enabled: bool) -> Result<(), String>
```

**hook 契约**（App 是唯一调用方）：

```ts
const { compact, busy, enter, exit } = useCompactMode();
// compact: boolean | null —— null = 启动 bootstrap 未完成（首帧门）
// enter/exit: Promise；失败时 compact 不翻转（按钮回弹），错误进 console
```

## Shape

### 数据结构（先于代码）

**核心新增 wire 类型 `CompactCell`** —— 每个供应商的"精简条投影"，Rust 侧每次轮询算好、随 `ProviderStatus` 下发。这是本设计最承重的决定：精简条需要的三条规则（主窗口挑选、货币格式化、告警判级）在 Rust 已有唯一实现（tray.rs），**全部留在 Rust 侧组合复用**，前端只渲染字符串与等级，不在 TS 复制任何一条规则（boundary-discipline：跨 IPC 边界暴露领域投影而非原始数据；laziness-protocol：规则单源）。

### TS 类型草图（src/types.ts）

```ts
export interface QuotaInfo {
  // …现有 8 字段不动…
  /** 重置时刻 ISO 8601 UTC；wire payload 已有，补声明即可驱动活倒计时 */
  reset_at: string | null;
}

/** 精简条单供应商投影 —— 与 Rust compact::CompactCell 严格一致（snake_case） */
export interface CompactCell {
  /** 主剩余值展示文本："62%" | "$12.34" | "¥88.88" | "–"（与托盘同格式） */
  value_text: string;
  /** 告警等级，与 tray::quota_warning_level 同阈值：0 平静 / 1 黄 / 2 红 */
  level: 0 | 1 | 2;
  /** 主窗口重置时刻（ISO）；活倒计时数据源 */
  reset_at: string | null;
  /** 无 reset_at 时的回退展示文本（后端预格式化，如同托盘菜单明细行） */
  reset_text: string | null;
}

export interface ProviderStatus {
  // …现有 7 字段不动…
  /** 精简条投影；null 仅出现在前端乐观合成的 pending 行（useUsage.ts:108-117） */
  compact_cell: CompactCell | null;
}

export interface AppConfig {
  // …现有 6 字段不动…
  /** 精简模式：条状窗口 + 精简视图（持久偏好，默认 false） */
  compact_mode: boolean;
}
```

### 纯函数（src/countdown.ts，新文件）

```ts
/** 与 Rust tray::countdown_compact 同格式：12M / 1H4M / 6D / 3D4H。
 *  已到期或解析失败返回 null（调用方回退 reset_text）。
 *  与 Rust 的平行复制是已接受的 tradeoff（见 Tradeoffs），由 verify 用例锚定格式。 */
export function countdownCompact(resetAtIso: string, nowMs: number): string | null;
```

### Rust 结构草图（src-tauri/src/compact.rs，新模块）

```rust
//! 精简模式：CompactCell 投影 + 窗口几何切换。几何策略全仓库唯一入口。

/// 精简条投影（纯函数，无框架依赖）——复用 tray 既有规则，不自立口径。
pub fn compact_cell(p: &ProviderStatus) -> CompactCell {
    // error 行：value_text "–"、level 0、无重置（与托盘 "–" 同 calm 语义）
    // percent 型：值取 primary_quota 的 remaining_percent（无 quota 回退 balance），
    //   level 用 quota_warning_level（本就按主窗口判级）→ 值与色同源
    // CNY/USD 型：值复用 tray_value 格式；reset_* 为 None（余额型无重置概念）
}

/// 窗口形状状态（managed state，setup 注册；同 TrayUpdateItem 托管范式）。
/// saved 仅进程内有效：精简中退出应用，下次退出精简回到 tauri.conf 默认位。
#[derive(Default)]
pub struct ShapeState(Mutex<Inner>);   // Inner { shape: Shape, saved: Option<SavedGeometry> }

/// 形状切换的唯一实现。幂等：shape 已等于目标时直接 Ok（make-operations-idempotent：
/// 重复调用 / Ready 启动与命令路径并发 / 中途失败重试都收敛到同一终态；
/// 全部 set_size/set_position 都是绝对值重算，不是增量）。
pub fn apply_shape(window: &WebviewWindow, enabled: bool) -> Result<(), String> {
    // enabled：
    //   1. current_monitor()（None 回退 primary_monitor()）取 work_area
    //   2. 捕获 outer_position + outer_size（Physical）入 saved
    //   3. set_size(LogicalSize(round(w*0.6).clamp(560, work.w-48), 140))
    //   4. set_position(LogicalPosition(work 顶居中，y = work.y + 8))
    //   5. set_always_on_top(true)
    // disabled：
    //   1. saved 有值 → set_size/set_position 原样恢复（Physical 对 Physical，无 DPI 往返）
    //      saved 无值（精简中重启）→ LogicalSize(720, 600) + 当前屏居中（conf 默认语义）
    //   2. set_always_on_top(false)
    // work_area 不可用（tauri 版本未透出）→ 回退 mon.position.y + 25*scale（菜单栏近似），
    // 该分支须在实施时以 cfg 或运行时探测落地并实测（风险 R3）
}

/// 前端入口（enter/exit 均调）。薄壳：校验目标态 → apply_shape。
#[tauri::command]
pub async fn set_compact_mode(window: WebviewWindow, enabled: bool) -> Result<(), String> {
    apply_shape(&window, enabled)
}
```

几何参数（初值，实施走查校准）：宽 = `round(work_area.width * 0.6)` 夹在 `[560, work_area.width - 48]`；高 = 140 逻辑 px（容纳两行 chips；延续"固定紧凑尺寸"历史决策，不做数据驱动自适应）；y = `work_area.y + 8`；x = work_area 水平居中。选屏 = 窗口当前所在屏（用户把它拖去哪就在哪展开），无屏可选时回退主屏。

### 4 个空态/错误态语义（精简条内）

精简条高度固定，四个早退分支全部塌缩为"居中单行文案 + 可选动作按钮"一个模板：

| 主页语义 | 精简条 | 动作 |
|---|---|---|
| loading（首次无数据） | `加载中…`（复用 loading-spinner 缩小版） | 无 |
| error（取数失败） | `获取数据失败`（单行截断，title=全文） | 内联 `重试` 按钮（复用 refresh） |
| 零配置 | `暂无服务商` | `打开设置`（先 exit 再 setView('settings')） |
| 全部禁用 | `所有供应商已禁用` | `打开设置`（同上） |

供应商级 error（chip 内）：值槽显示 `–`、tertiary 色、title = 错误全文——与托盘"出错显示 –"同 calm 语义，不加红不加告警。

### 模块地图

| 文件 | 改动 |
|---|---|
| `src/types.ts` | +`reset_at`(QuotaInfo)、+`CompactCell`、+`compact_cell`(ProviderStatus)、+`compact_mode`(AppConfig) |
| `src/countdown.ts` | **新**：`countdownCompact` 纯函数（~18 行） |
| `src/hooks/useCompactMode.ts` | **新**：bootstrap（挂载读 get_config → compact 初值）+ enter/exit + 材质时序编排（下节）；失败不翻转状态，样板同 useFrozenBackground |
| `src/components/CompactBar.tsx` | **新**：条 + CompactChip（logo 复用 BRAND_LOGOS 双路解析，从 UsagePanel 导出共享）+ 4 空态模板 + 刷新/退出按钮；30s `setInterval` now-tick 驱动活倒计时 |
| `src/App.tsx` | L122-140 设置按钮 → 精简按钮；L39-44 背景图加 `compact === false` 门；L46-151 外包 compact 渲染分支；L166 version-tag 加 `!compact` 门；navigate 监听 exit-then-settings |
| `src/styles/app.css` | 末尾追加 `/* ===== 精简模式 ===== */` 一节（~90 行）：`.compact-bar`（drag region、左 76px 红绿灯避让）、`.compact-chip*`、`.compact-empty*`；只用现有 CSS 变量，不开新文件 |
| `src/components/UsagePanel.tsx` | 仅导出 `BRAND_LOGOS`/`BrandLogo` 供 CompactBar 复用（单一 logo 解析源，不复制映射表） |
| `src-tauri/src/models.rs` | +`CompactCell` struct；`ProviderStatus` +`compact_cell: Option<CompactCell>`；`AppConfig` +`#[serde(default)] compact_mode: bool`；仿照现有 `notifications_default_tests` 补 legacy-config 缺字段测试 |
| `src-tauri/src/compact.rs` | **新**：`compact_cell` 纯函数、`ShapeState`、`apply_shape`、`set_compact_mode` 命令、几何计算私有函数；`compact_cell` 的取值/判级用单测锚定（fixture 与 tray 测试同款） |
| `src-tauri/src/scheduler.rs` | 构建 ProviderStatus 行处补 `compact_cell: Some(compact::compact_cell(&row))`（一行） |
| `src-tauri/src/lib.rs` | `mod compact`；setup `app.manage(ShapeState::default())`；invoke_handler 注册命令；Ready 阶段 show 之前读 config_state，`compact_mode` 为真时对隐藏窗口 `apply_shape(w, true)`（两条启动路径共用，--minimized 也覆盖） |
| Rust 测试 fixtures（tray.rs / scheduler.rs 测试的 `status()` 构造器） | 机械补 `compact_cell: None` 字段（编译器强制，一次性） |
| `.claude/skills/verify/mock/tauri-shim.js` | fixtures：providers 补 `compact_cell`（含 reset_at）、config 补 `compact_mode: false`；新 handler `set_compact_mode` 记录 `state.compactShapeCalls`；windowEffects 记录照旧（供时序断言） |
| `.claude/skills/verify/features/compact-mode.md` | **新** feature 文件 + README 表加行；SKILL.md 命令数 14→15、Drive 句柄表补精简条条目 |

verify 用例（Lane A）：进入→`compactShapeCalls` 长度 1、chips 与 fixture cell 一致（62% · 倒计时）、设置按钮消失、savedConfigs 末条 `compact_mode:true`、windowEffects 末条 `follow`；退出→几何恢复调用、回到三卡布局、windowEffects 末条回到 `frozen`；四空态逐个驱动；托盘 navigate settings 在精简下先退出。Lane B（真实窗口）：条形观感、材质、红绿灯避让、拖动、置顶、重启直接进条、退出几何还原（配置守卫照做）。

### 关键决策与理由（对应 orchestrator 的 9 问）

**D1 状态放 AppConfig 持久化**（`compact_mode: bool`，serde default false）。理由：模式是用户偏好而非临时视图（同 `background_mode` 先例，models.rs:87-89）；"挖孔屏"是常驻形态，重启即丢违背预期。启动关系：Ready 阶段（lib.rs:112-126）在 show 之前对隐藏窗口套用几何，普通启动与 `--minimized` 自启统一覆盖——从托盘打开时已是条形，无闪变。前端 bootstrap 读 get_config 定 compact 初值，`null` 期间渲染空（窗口此刻不可见，零成本防闪）。持久化不落几何值：几何 = 派生态（普通形态永远以 tauri.conf 为事实源），配置里只存布尔意图（single source of truth，derive instead of sync）。

**D2 几何所有权与实现路径：单一 Rust 命令** `set_compact_mode` + 私有 `apply_shape`（照 apply_window_effects 模板），不用 JS 直调。理由：JS 直调要扩 5+ 条 capabilities（set-size/set-position/set-always-on-top…每条都是安全面），且屏幕几何、原几何保存、幂等、置顶这些策略将散落前端；一个命令把它们全部藏进 callee（interface depth：小表面藏大能力）。原几何保存：进程内 `ShapeState` Mutex（TrayUpdateItem 托管范式），enter 时捕获 Physical outer_position/outer_size，exit 原样恢复；进程内不存盘——精简中退出应用，下次退出精简回到 conf 默认位（720×600 居中），语义诚实且 AppConfig 不被运行时几何污染（separate-before-serializing：用户意图进配置，瞬态进内存）。

**D3 条窗口属性：不碰装饰**。保留 Overlay 标题栏与红绿灯，条内沿用 76px 左距避让（现有 `.app-header` 同款）；拖动区 = 条背景 `data-tauri-drag-region`（chips 与按钮正常收事件）；`always_on_top(true)` 进入时开、退出时关（"挖孔屏"隐喻必须浮在应用窗口之上，否则条会被盖住失去常驻意义；不做用户开关，v1 保持一个观点）；`resizable` 保持 false（几何归应用所有）。`set_decorations(false)` 路线被明确拒绝（风险 R2 未验证且不可逆改 chrome）。

**D4 信息模型**见上文 CompactCell 与空态表。补充三点：(a) 精简值取托盘口径（percent 型按 primary_quota），与主页主数字（balance.available_balance）可能不同——主页/托盘双口径是既有漂移，本设计选择"与托盘一致"因为精简条同属 glance 表面，且不扩大也不修复主页漂移；(b) chips 展示全部 enabled 供应商（show_in_tray 是托盘专属开关，主页不筛它，精简条同主页）；(c) 重置时间 = reset_at 活倒计时（30s 步进），缺失/解析失败/已过期回退 reset_text，再缺失不显示。

**D5** 空态见上表。

**D6 退出交互**：条右端 `maximize-2` 图标按钮是唯一显式出口；两个"打开设置"空态按钮 = exit 的组合调用；托盘 navigate settings 路径同样先 exit。不加托盘菜单项、不加快捷键、不加双击手势——一个出口，读者零记忆负担（minimize-reader-load / two-ways-to-one-task 红旗防御）。

**D7 冻结背景在尺寸切换瞬间的处理：材质接管**。精简期间不渲染 `.background-frozen`（它按 100% 100% 铺 720×600 截图，条形下必然失真——规避风险 R5 而非修补），窗口回退到 hudWindow 活材质（本来就是 conf 的出厂态）。切换时序（进：save → apply('follow') 材质上 → 视图翻转图隐没 → set_compact_mode；出：save → set_compact_mode → 视图翻转图显回 → apply(background_mode) 材质还原）保证任意瞬间窗口都有不透明背景：进入时图先盖着材质、退出时图先显回再撤材质——复用 useFrozenBackground 的"图与材质永不同时缺席"防闪纪律，两条序列各以注释落在 hook 里。启动进条（配置持久化路径）无需任何材质操作：conf 出厂即 hudWindow，冻结图从未渲染。

**D8 组件/模块落点**见模块地图。CSS 沿用单文件扁平约定追加一节；图标手写 feather inline SVG（`minimize-2` 进 / `maximize-2` 出）与相邻按钮同规格；shim 与 verify 用例按模块地图所列补齐（IPC 边界变更 = 两条 lane 都要动）。

**D9 不做的事**：

- 不做屏幕选择 UI、不跨屏记忆条位置（跟随窗口所在屏）。
- 不做宽高随供应商数自适应（固定公式 + flex 换行；超过两行的账号规模 chips 压缩省略 + tooltip，明示上限而非静默丢行）——延续 2026-10-31 固定尺寸决策。
- 条内零编辑能力：增删供应商、改设置一律退出精简后再做。
- 不新增托盘菜单项、不加键盘快捷键。
- 不做条内悬停详情弹层（title tooltip 覆盖）。
- 不动主页卡片的取数口径；不修复主页/托盘既有双口径漂移。
- 不引入前端测试框架（repo 无 runner；countdownCompact 由 Lane A 断言锚定）。
- Windows/Linux 不实测贴顶几何（代码全走 tauri 跨平台 API，但只在 macOS 验证——与仓库现有 cfg 门控的实测范围一致）。
- UpdateNotice 更新横幅不进精简条（托盘菜单已有"检查更新"）。

### 红旗自查（design-red-flags）

浅模块：`set_compact_mode` 一个签名藏屏幕几何/选屏/保存恢复/幂等/置顶五件事，深；`compact_cell` 藏三条规则，深。信息泄漏：无 transport 类型外泄，CompactCell 本身就是领域投影。时间分解：compact.rs 按"精简模式"这一领域知识聚合投影与几何，不按执行顺序。pass-through：`set_compact_mode → apply_shape` 有两个调用方（命令 + Ready 启动）且命令侧承担校验，非转发。split ownership：材质的两个调用方（背景 hook / 精简 hook）共用同一命令为策略点，写方按用户交互天然串行且时序各自文档化；几何状态单 owner（ShapeState）。two ways：进/出口、倒计时、logo 解析各一条路（BrandLogo 从 UsagePanel 导出复用而非复制映射表）。importable internals：ShapeState pub 仅为 app.manage()（TrayUpdateItem 同款）。hand-synced list：倒计时格式 TS/Rust 平行是唯一双列表，已列为接受之 tradeoff 并用断言锚住。

## Synthesis decision

（留空——arena 合成阶段填写）

## Tradeoffs accepted

- 接受精简条与主页主数字双口径（精简同托盘、可能异于主页）——同一供应商两处数字不同是既存漂移的延续，精简条明确站托盘一侧。
- 接受 saved geometry 仅进程内：精简中退出应用，下次"退出精简"回到 conf 默认位而非上次的普通位。
- 接受条左端 76px 红绿灯避让占用——换取完全不碰 `set_decorations` 私有行为（R2 规避）。
- 接受 alwaysOnTop 强制开启且无开关——全屏视频等场景条会浮在上面，用户需退出精简；换 v1 的单一观点。
- 接受 countdownCompact 在 TS 的 ~18 行平行复制——换取分钟级活倒计时；格式漂移由 verify 断言与 Rust 测试共同锚定。
- 接受几何命令失败时"留在原地"（无 toast 通道，仅 console + 状态不翻转回弹）——精简条没有 toast 基建，为此铺一个不值。
- 接受进入精简会顺带触发一次立即轮询（save_config 现行为 notify_one）——无害且数据更新鲜。
- 接受两个 hook 都调用 apply_window_effects——单命令单策略点，时序各 6 行注释，比造一个"材质编排器"抽象更省读者负担。

## Alternatives considered

1. **JS 直调 window API + capabilities 扩容**。输在 interface depth：屏幕几何、保存恢复、幂等全部漏给前端，5+ 条权限面扩容；Rust 命令把同 some capability 压进一个签名。
2. **只暴露 `primary_quota` 字段、前端自算格式/判级/倒计时回退**。输在规则复制：TS 要重抄主窗口选择、货币格式化、告警判级三条 Rust 规则——正是 grounding 点名的 warningLevel 双实现之痛扩大化；CompactCell 让 Rust 组合既有 pub 函数一次成型。
3. **独立 HUD 第二窗口**。输在状态分裂：两个 webview、两棵 React 树、双份数据订阅、显隐与 `sync_tray_click_flag` 交错翻倍（托盘纪律 2 明言每条显隐路径都要同步标志）；变形主窗口保住单一 React 树、单一数据流、单一生命周期。
4. **`set_decorations(false)` 真全条**（隐藏红绿灯）。输在未验证风险上押注：Overlay + 运行时去装饰的 macOS 行为是 grounding R2，失败形态不可回退；保留装饰是零风险路径且观感原生。
5. **冻结背景图拉伸/裁切适配条形**。输在观感（R5）：100% 100% 拉伸失真、cover 裁切出怪异横带；活 hudWindow 材质是 HUD 的原生语言且复用既有命令。
6. **模式只存前端 view state 不持久化**。输在产品语义：模式是常驻偏好（background_mode 先例），每次启动重进违背"挖孔屏常驻"预期；且持久化不比 view state 多任何复杂度（一个 serde default 字段 + Ready 一行）。

## Implementation reconciliation

（留空——实施期按 architect Phase D 纪律记录已接受的偏差与接受来源）

## Open questions and risks

必须在实施中实测验证（grounding 五条未验证风险的落地处置）：

- **R1 hudWindow 材质在 140px 高横条下的观感**（圆角半径、hairline 边）。Lane B 目测；若不佳，备选是 frozen 图 cover 裁切方案（本设计已弃的 alternative 5 的弱化版）。
- **R2 Overlay 标题栏 + 红绿灯在条内的位置**。设计已规避 set_decorations，但仍需目测 76px 左距在条内不与 logo/chips 重叠。
- **R3 屏幕几何 API**：`Monitor::work_area` 在锁定的 tauri 2.x 是否可用；不可用即走 `mon.position.y + 25*scale` 回退分支。贴顶 y=8px 的实际观感、多屏下 `current_monitor` 空值率、scaleFactor 换算——全部 Lane B 实测。这是全仓库第一次屏幕几何操作，没有先例可抄。
- **R4 resizable:false 窗口被运行时 set_size 后的交互**：拖动区、置顶点击、红绿灯。预期可行（resizable 只约束用户 resize；start-dragging 权限已有），Lane B 验证。
- **R5 冻结背景切换瞬间**：设计以"图与材质永不同时缺席"时序规避，但 enter step 3→4 与 exit step 2→3 的中间态（条形视图在 720×600 窗口内、全视图在条形窗口内的一拍）需要 Lane B 目测确认无可感闪变。

设计新增、需人工/实施裁量的问题：

- 60% 宽 / 560 下限 / 140 高 / 8px 顶距四个初值是推断值（guess），实施时按 9 家 fixtures 真实走查定稿。
- 几何命令失败只 console 回弹，要不要在条上闪一个轻提示？（v1 判定不值，如用户反馈找不到原因再补。）
- 条内 chips 超过两行（约 12+ 账号）压缩省略是否可接受，还是直接把宽度上限提到 work_area-24？

## Next implementation step

先写 Rust 侧：models.rs 加 `CompactCell`/`compact_mode` 字段 + compact.rs 的 `compact_cell` 纯函数（配 fixture 单测，复用 tray 测试的构造器），跑 `cargo test` 绿——wire 投影是前端一切渲染的前提，且它不依赖任何未验证风险。
