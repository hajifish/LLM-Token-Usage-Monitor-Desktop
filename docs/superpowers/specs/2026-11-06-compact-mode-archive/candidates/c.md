# 精简模式（compact mode）设计

## Problem

TokenUsage 是 720×600 的 macOS 菜单栏常驻小窗（Accessory，无 Dock），主页用 3 列卡片展示供应商。用户要求一个"精简模式"：点主页右上角新按钮进入，窗口变成贴屏幕顶部的宽矮横条（挖孔屏观感），每个供应商压成一行内的一个片段，只留图标、名字、剩余额度、重置时间，且能退出并恢复原窗口几何。

这个形状在本仓库没有先例。全仓库零运行时窗口几何代码，尺寸完全由 `tauri.conf.json:14-29` 静态声明；唯一"Rust 运行时改窗口属性"的范式是 `background.rs:16-42` 的 `apply_window_effects`。Phase A 另外钉死了四个约束：

- 配置纪律：AppConfig 在 Rust 侧加密落盘，前端"get_config 读 fresh → 只改一个字段 → save_config → invoke apply 命令 → 失败回弹"（`useFrozenBackground.ts:64-90` 是样板）。
- percent 型"剩余"有两层口径：`balance.available_balance`（provider 层挑好，5h 优先）与 `quota_infos` 全窗口平铺；托盘用 `primary_quota`（`tray.rs:204-214`，剩余最低者）但它没过 IPC。
- 托盘短码/紧凑倒计时/告警判级已在 Rust 侧成型（`tray.rs`），主页是另一套 TS 实现，两处已知漂移（grounding"告警判级双实现"）。
- 窗口 Close 是 hide 不是退出（`lib.rs:83-90`），应用一次会话可以极长；重启时窗口几何本来就不保留（总是回到 conf 值）。

## Usage (caller's view)

### 用户视角 UX 流程

1. 主页右上角按钮组（`App.tsx:57-141`）的第 4 个图标按钮从齿轮换成 feather `minimize-2`（双箭头向内收），title「精简模式」。设置不再从主页进入，托盘菜单「设置」保持可达。
2. 点击后窗口立即变成宽矮横条：宽度约为当前显示器工作区减 16 逻辑 px（下限 600），高 56 逻辑 px，水平居中贴工作区顶部（菜单栏下方 4px），置顶（alwaysOnTop）。条内左侧留 76px 红绿灯槽，接小号 logo + TokenUsage 字标，然后是所有启用供应商的 chip 单行排开（超宽横向滚动），最右一个 feather `maximize-2`（向外展开）按钮即退出。
3. chip 内容：`[品牌 logo 16px][alias][主值 粗体着色][重置倒计时 灰色小字]`。主值口径与托盘同源（percent 型取主窗口，见 Shape）；着色：红（耗尽）/橙（<5% 或 CNY<¥10）/绿，USD 不告警。悬停 chip 的 tooltip 给全部配额窗口明细。
4. 退出路径有两条，都收敛到同一个 `exitCompact`：条内 `maximize-2` 按钮；以及在精简态触发托盘「设置」（任何 navigate 到完整页面都先退出精简，因为 56px 条内放不下设置页）。退出恢复进入前的窗口尺寸和位置，关闭置顶。
5. 重启语义：`compact_mode` 随 AppConfig 持久化。再次启动时 Rust 在窗口 show 之前就套用精简几何，直接以横条形态出现，无闪烁；autostart `--minimized` 时窗口保持隐藏，几何已提前套好，之后从托盘唤起即是精简形态。

### 代码级调用点（真实代码位置）

App.tsx 渲染分支（view 状态机加一个值）：

```tsx
const [view, setView] = useState<'usage' | 'compact' | 'settings' | 'providers' | 'skills'>('usage');

// 启动：配置为真则直接进入精简视图（几何已由 Rust 在 setup 阶段套好）
useEffect(() => {
  let alive = true;
  invoke<AppConfig>('get_config').then((cfg) => {
    if (alive && cfg.compact_mode) setView('compact');
  }).catch(console.error);
  return () => { alive = false; };
}, []);

// 进入/退出：命令失败则视图不翻转（回弹），与仓库「失败 UI 回弹」纪律一致
const enterCompact = async () => {
  await invoke('set_compact_mode', { enabled: true });
  setView('compact');
};
const exitCompact = async () => {
  await invoke('set_compact_mode', { enabled: false });
  setView('usage');
};
```

按钮替换（`App.tsx:122-140` 原齿轮按钮处）：

```tsx
<button className="settings-icon-btn" onClick={enterCompact} title="精简模式">
  {/* feather minimize-2 */}
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor"
       strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <polyline points="4 14 10 14 10 20"/><polyline points="20 10 14 10 14 4"/>
    <line x1="14" y1="10" x2="21" y2="3"/><line x1="3" y1="21" x2="10" y2="14"/>
  </svg>
</button>
```

精简视图分支（与 usage/settings 并列）：

```tsx
) : view === 'compact' ? (
  <CompactBar
    summary={summary} loading={loading} refreshing={refreshing}
    error={error} refresh={refresh} onExit={exitCompact}
  />
) : view === 'settings' ? (
```

navigate 监听改造（`App.tsx:24-35`，精简态打不开完整页面，先退再切）：

```tsx
if (event.payload === 'settings') {
  void (async () => {
    await exitCompact().catch(() => {}); // 几何恢复失败也不拦住设置页
    setView('settings');
  })();
}
```

Rust 命令签名（lib.rs 注册，前端 invoke 面）：

```rust
#[tauri::command]
pub async fn set_compact_mode(
    app: AppHandle,
    window: tauri::WebviewWindow,
    config_state: tauri::State<'_, Arc<RwLock<AppConfig>>>,
    health_state: tauri::State<'_, Arc<RwLock<ConfigHealth>>>,
    compact: tauri::State<'_, Arc<CompactState>>,
    enabled: bool,
) -> Result<(), String>
```

启动套用（`lib.rs` setup 内，config 加载之后、Ready-show 之前）：

```rust
if app_config.compact_mode {
    if let Some(w) = app.get_webview_window("main") {
        if let Err(e) = compact::enter(&w, &compact_state) {
            log::error!("启动套用精简几何失败: {e}");
        }
    }
}
```

CompactBar 内部消费（唯一真正读新字段的展示代码）：

```tsx
const chips = visibleProviders.map(toChip); // toChip: ProviderStatus -> CompactChip（纯函数）
// chip 主值 = displayPercent(p) 走 primary_quota；重置 = p.primary_quota?.reset_time ?? null
```

## Shape

### TS 类型草图（src/types.ts，全部与 Rust serde 输出严格一致）

```ts
export interface QuotaInfo {
  // ……现有字段不动
  reset_at: string | null;        // 补声明（Rust models.rs:140 已有，serde default）
}

export interface ProviderStatus {
  // ……现有字段不动
  /** 主窗口（与 tray primary_quota 同源）：percent 型供应商的展示口径。 */
  primary_quota: QuotaInfo | null;
}

export interface AppConfig {
  // ……现有字段不动
  /** 精简模式：宽矮贴顶状态条形态；几何由 Rust 随配置在启动时恢复。 */
  compact_mode: boolean;
}
```

精简视图模型（`src/format.ts`，纯函数，无框架依赖，per boundary-discipline）：

```ts
export type WarnLevel = 0 | 1 | 2;
/** 告警阈值单一 TS 出处：主页与精简条共用（Rust 托盘侧另有同值实现，跨语言边界） */
export const THRESHOLD = { RED: 0, YELLOW_PCT: 5, YELLOW_CNY: 10 } as const;

/** percent 型取主窗口剩余；缺失回退 balance；USD/CNY 取 balance（与主页主数字同源） */
export function displayPercent(p: ProviderStatus): number | null;
/** 主值的字符串形态："62%" | "$12.34" | "¥42.50" | "–" */
export function formatMainValue(p: ProviderStatus): string;
/** 告警判级：percent 且有主窗口按主窗口判（与 tray quota_warning_level 同口径），其余回退 balance 规则 */
export function warningLevelOf(p: ProviderStatus): WarnLevel;

export interface CompactChip {
  name: string;          // p.alias（后端已回退，恒非空）
  brand: string;         // p.name，供 BrandLogo 查表
  value: string;         // formatMainValue 输出
  level: WarnLevel;
  reset: string | null;  // 主窗口 reset_time（后端预格式化紧凑文本）
  error: string | null;  // 供应商级错误（原样进 tooltip）
}
export function toChip(p: ProviderStatus): CompactChip;
```

### Rust 结构草图（src-tauri/src/compact.rs，新模块）

```rust
//! 精简模式窗口几何。只动几何与置顶；材质/背景归 background.rs，互不越界。

/// 进入精简前的几何基线。exit 恢复后清空。
#[derive(Debug, Clone, Copy)]
pub struct SavedGeometry {
    size: tauri::PhysicalSize<u32>,
    position: tauri::PhysicalPosition<i32>,
}

/// 托管态：Option 为空表示本会话尚未进入过精简。
#[derive(Default)]
pub struct CompactState(Mutex<Option<SavedGeometry>>);

// 常量（逻辑像素，集中在本模块一处）
const BAR_HEIGHT: f64 = 56.0;
const MARGIN_X: f64 = 8.0;
const TOP_GAP: f64 = 4.0;
const MIN_WIDTH: f64 = 600.0;

/// 纯几何数学（可单测，不碰窗口 I/O）：工作区矩形 + scale_factor -> 目标条矩形
fn band_rect(
    work: (i32, i32, u32, u32),
    scale: f64,
) -> (tauri::PhysicalPosition<i32>, tauri::PhysicalSize<u32>);

/// 幂等进入：基线仅在为空时捕获（重复进入不会把精简几何误存为基线），
/// 然后 set_size + set_position + set_always_on_top(true)。
pub fn enter(window: &tauri::WebviewWindow, state: &CompactState) -> Result<(), String>;

/// 幂等退出：恢复基线并清空槽位、set_always_on_top(false)；
/// 槽位为空（未进入过）时只确保置顶关闭，不视为错误。
pub fn exit(window: &tauri::WebviewWindow, state: &CompactState) -> Result<(), String>;

#[tauri::command]
pub async fn set_compact_mode(/* 签名见 Usage */) -> Result<(), String> {
    // 1. 幂等闸：fresh.compact_mode == enabled 直接 Ok（双击、IPC 重放安全）
    // 2. persist_config 落盘（唯一落盘咽喉，含身份校验不变）
    // 3. enabled ? enter : exit
    // 4. transform 失败 -> 尽力回滚落盘标志 -> Err（前端视图不翻转，全链路一致）
}
```

### 数据流与决策（对照任务要求的 9 个点）

**1. 状态放哪。** `AppConfig.compact_mode: bool`（`#[serde(default)]`），持久化。理由：几何必须由 Rust 在窗口 show 之前恢复才能无闪启动，而 Rust 在 setup 只能读到配置文件，读不到 React；配置是两边都能读的唯一事实源（single source of truth）。命令内部先落盘意图再动几何，失败回滚落盘（per make-operations-idempotent：双击早退 Ok、崩溃半途靠回滚收敛、重启后磁盘态即窗口态）。启动直接以精简形态打开；与 `--minimized` 正交（几何照套，窗口照藏）。

**2. 几何所有权与实现路径。** 走单条 Rust 命令 `set_compact_mode(enabled)`，照 `apply_window_effects` 模板（`window: WebviewWindow` 提取器），不走 JS 直调。理由：JS 直调要给 capabilities 永久加 `allow-set-size`/`allow-set-position`/`allow-set-always-on-top`/`allow-set-fullscreen` 等一串窗口权限；而屏幕几何、缩放换算、置顶、基线捕获这套政策本来就该住在 callee 一处（per interface depth：一个 bool 的表面，藏住落盘、几何、幂等、回滚四层能力）。capabilities 一行不改。几何参数：选窗当前所在显示器（`current_monitor()`，None 回退 `primary_monitor()`）；宽 = 工作区宽 − 2×8，下限 600；高 56；y = 工作区顶 + 4；x 水平居中。逻辑像素常量乘 `scale_factor()` 换物理像素。原几何：进入时捕获进 `CompactState`（仅空槽时捕获），退出恢复该基线并清槽；启动即精简时基线就是 conf 给的初始几何，因此不需要把 720×600 在 Rust 里再抄一份（消灭一处 hand-synced list）。

**3. 条的窗口属性。** 装饰完全不动（保持 Overlay 红绿灯），布局左侧留 76px 槽避让，规避 grounding 风险 #2（运行时 set_decorations 行为未验证）。不做 fullscreen/隐藏标题外的任何装饰实验。`alwaysOnTop(true)` 进入、退出关闭，默认给：条的价值在于干活时瞥一眼，与"挖孔屏"隐喻一致。拖动沿用 `data-tauri-drag-region`（条内空白区可拖，权限已授）。`resizable` 保持 conf 的 false（程序化 set_size 不受它限制，用户拖拽移动不受影响，实施时实测）。

**4. 行内信息模型。** 每供应商一个 chip：图标 + alias + 一个主值 + 一个重置倒计时，多配额窗口压成主窗口一个。口径决策：给 `ProviderStatus` 增加 `#[serde(default)] primary_quota: Option<QuotaInfo>`，由 scheduler 在 `poll_providers` 末尾用现有 `tray::primary_quota()` 集中填一行（不新增 IPC 命令，不把托盘私有逻辑复制进 TS——让 Rust 既有规则多一个 IPC 消费者，而非两套实现）。主值：percent 型取 `primary_quota.remaining_percent`（缺失回退 `balance.available_balance`），USD/CNY 取 balance。这与托盘数值三方同源（tray、精简条、主页主数字），顺带修复主页 % 型主数字与托盘不同源的老漂移（主页从 balance 换到 primary 口径，见 Tradeoffs）。告警着色：TS 侧 `warningLevelOf` 用共享阈值常量，percent 有主窗口时按主窗口判级，与 `tray::quota_warning_level` 同规则同阈值；USD 恒绿。重置时间：v1 用后端预格式化的 `reset_time` 紧凑文本（"12M"/"6D"，provider 层已输出），不做前端倒计时定时器；整行本来就是上次轮询的快照，数值与倒计时同龄同鲜（per subtract-before-you-add；`reset_at` 类型先补上，留作后续实时倒计时的现成挂点）。次要信息（今日 token、调用次数、余额构成、非主窗口）不进条，全量明细进 chip 的 title tooltip（零布局成本）。空态/错误态（对应 UsagePanel 四个早退分支）：loading → 条内一行「加载中…」；get_usage 失败 → 「数据获取失败」+ 行内小「重试」（复用 props.refresh）；零配置 → 「暂无供应商」；全禁用 → 「所有供应商已禁用」。三种空态条形几何不变（宽度公式与数据量解耦），空态文案旁就是退出按钮，用户唯一的出路（去完整模式修配置）不需要额外交互设计。供应商级错误（如智谱 401）：chip 显示 logo + alias + 红色「异常」，原错误文本进 tooltip。

**5.（并入 4）** 空态语义如上四条，全部有明确文案与出口。

**6. 退出交互。** 条内右侧 `maximize-2` 图标按钮 + 「精简态收到 navigate 事件先退出再切视图」规则，两个入口收敛到同一个 `exitCompact`。不再加托盘菜单项、不加 Esc 快捷键、不加双击条退出（一个显式按钮 + 一个必然路径够用，多入口是多份维护面）。设置页不加精简开关（入口/出口在 UI 上自解释，设置页开关会成为同一任务的第二条路，per two-ways 红旗）。

**7. 冻结背景层。** 精简态直接不渲染 `.background-frozen`（App 层条件 `{view !== 'compact' && background.imageUrl && …}`），条根节点自带近不透明深色 CSS 背景（`rgba(30,30,32,0.94)` + 10px 圆角）。`compact.rs` 完全不碰窗口材质，`useFrozenBackground` 一行不改：frozen 用户材质本就被移除、条靠 CSS 背景可见；follow 用户材质本就在、CSS 背景叠上去观感一致。两条路径下精简条观感统一，退出后背景图在恢复的 720×600 几何下比例天然正确（尺寸已复原，不再有拉伸错位）， grounding 风险 #1 与 #5 同时消解。

**8. 组件/模块落点。** 见下方 Module map。CSS 追加在 `app.css` 末尾一个带注释的 section（仓库唯一 CSS、扁平类名的既定组织方式，不引 CSS Modules）。tauri-shim 补 `set_compact_mode` handler（记录调用序、翻转 fixture 的 `config.compact_mode`）、fixture 补 `primary_quota`/`reset_at`/`compact_mode` 字段，并新增 `features/compact-mode.md` 用例（Lane A 断言 chip 内容与按钮往返；Lane B 实测窗口几何与置顶、重启恢复）。

**9. 明确不做。**
- 不做精简条内任何下钻/展开（看明细请退出精简，条是纯摘要）。
- 不做设置页精简开关、不做托盘「退出精简」菜单项。
- 不做前端实时倒计时定时器（用后端预格式化文本）。
- 不做条内手动刷新按钮（托盘点击已触发刷新，轮询照常）。
- 不做条的用户自定义高度/宽度/位置记忆（每次进入都重算贴顶居中）。
- 不做多行换行（超宽横向滚动，`scrollbar-width:none`）。
- 不做 fullscreen 空间悬浮（`visible_on_all_workspaces`）；全屏应用盖住条属可接受行为。
- 不做 Windows/Linux 专属几何适配（`band_rect` 数学跨平台，工作区回退常量按 macOS 菜单栏高标定，其他平台 best-effort）。
- 不改 UpdateNotice 之外的托盘行为；精简态不渲染 UpdateNotice 与版本水印（条内无空间），退出即回来。

### Module map

| 文件 | 改动 |
|---|---|
| `src-tauri/src/compact.rs` | 新增：SavedGeometry/CompactState/常量/`band_rect` 纯函数/`enter`/`exit`/`set_compact_mode` 命令 + band_rect 单测 |
| `src-tauri/src/models.rs` | AppConfig 加 `compact_mode`（serde default false，Default 补齐）；ProviderStatus 加 `primary_quota`（serde default）；补"旧配置缺字段可加载"测试 |
| `src-tauri/src/scheduler.rs` | poll_providers 末尾集中填 `primary_quota`（`tray::primary_quota(qs).cloned()` 一行循环）；三处 `ProviderStatus {` 字面量补 `primary_quota: None` |
| `src-tauri/src/lib.rs` | `mod compact;`；`app.manage(CompactState)`；setup 内按配置预套精简几何；`generate_handler!` 注册 `set_compact_mode` |
| `src-tauri/capabilities/default.json` | 不改（Rust 命令路径的意义所在） |
| `src/types.ts` | QuotaInfo 加 `reset_at`；ProviderStatus 加 `primary_quota`；AppConfig 加 `compact_mode` |
| `src/App.tsx` | view 联合类型加 `'compact'`；启动读配置定初值；`enterCompact`/`exitCompact`；齿轮按钮换 `minimize-2`；`background-frozen` 与 `version-tag` 按 `view !== 'compact'` 门控；navigate 规则；CompactBar 分支 |
| `src/components/CompactBar.tsx` | 新增：条视图（chips、四空态、退出按钮），消费 `format.ts` 与 `BrandLogo` |
| `src/components/BrandLogo.tsx` | 新增：`BRAND_LOGOS` + `BrandLogo` 从 UsagePanel 原样迁出，同波改 UsagePanel 的 import（migrate-callers-then-delete） |
| `src/format.ts` | 新增：`THRESHOLD`/`displayPercent`/`formatMainValue`/`warningLevelOf`/`toChip`；UsagePanel 改为消费同一组纯函数（主数字与告警切到主窗口口径） |
| `src/styles/app.css` | 末尾追加「精简模式」section（`.compact-bar/-brand/-chips/-chip/-chip-value.l0l1l2/-chip-reset/-empty`，约 80 行） |
| `.claude/skills/verify/mock/tauri-shim.js` | 补 `set_compact_mode` handler 与 `__VERIFY_MOCK__.compactCalls`；usage/config fixture 补三个新字段 |
| `.claude/skills/verify/features/compact-mode.md` | 新增用例文档：Lane A（chip 断言、进入/退出往返、告警着色）+ Lane B（几何/置顶/重启恢复/拖动可用） |

### 红旗自查（design-red-flags 逐条）

- Shallow module：`set_compact_mode` 表面一个 bool，落盘/几何/幂等/回滚全在 callee；CompactBar 对外只有数据 props + onExit。达标。
- Information leakage：几何政策（显示器选择、缩放换算、常量）不外泄为命令参数；TS 不复制托盘选择规则而是消费 IPC 出的 `primary_quota` 域字段。达标。
- Temporal decomposition：compact.rs 把捕获/变换/恢复三种时序聚在一个模块，因它们守护同一组决策（基线所有权）。达标。
- Split ownership：`compact_mode` 真值只在 AppConfig；基线只在 CompactState；材质所有权完全不动（仍属 background 链路）；视图态仍在 App。命令的 Ok 返回是 React 视图与 Rust 几何的汇合点。达标。
- Two ways to do one task：退出两入口收敛单函数；告警判级 TS 侧归一（主页与精简条共用 `warningLevelOf`），并顺手消灭主页/托盘数值口径漂移。达标。
- Hand-synced list：`BRAND_LOGOS` 归一；不在 Rust 重抄 720×600（惰性捕获基线）；几何常量只在 compact.rs。达标。
- Importable internals：BrandLogo/format 属有意共享的公共模块，无内部外泄路径。达标。

## Synthesis decision

（由 arena 合成阶段填写。）

## Tradeoffs accepted

- 接受精简条近乎不透明的 CSS 深色背景，换取零材质状态协调与 frozen/follow 两种配置下观感统一；比 follow 模式的实时半透明更"实"一点。
- 接受主页 % 型主数字与告警从 `balance.available_balance` 切到主窗口口径（与托盘同源），这是一个超出精简模式本身的用户可见行为修正（Claude Code 双窗口时主页数字会变），需要自己的红测与走查确认；不同源则"同一供应商在托盘和条上显示两个数"是更糟的产品态。
- 接受固定公式宽度（可能右侧留白或横向滚动）而非按内容测宽，换取无"改尺寸 → 重排 → 再测宽"反馈环，几何确定性可测。
- 接受重置倒计时随轮询快照陈旧（最长一个刷新间隔），换取无定时器、无 Rust/TS 第二份倒计时格式化实现。
- 接受 IPC payload 每供应商多一个 `primary_quota`（既有元素的克隆，量级可忽略），换取三表面（托盘/精简条/主页）一个数值口径。
- 接受告警阈值在 Rust 与 TS 各存一份（跨语言边界），以共享常量命名把 TS 侧压到一处；两份的漂移靠同值命名与用例对齐，无法结构性消除。
- 接受 alwaysOnTop 的精简条被全屏应用盖住、以及托盘 show/focus 会把焦点交给条（与今天主窗口行为一致）。
- 接受"启动套用几何失败（极端情况取不到显示器）时磁盘为 compact 而窗口常态"的降级形态（React 仍按配置渲染精简视图于常规窗口内，退出按钮可自救），为它写专门的恢复逻辑不值。
- 接受启动时 hook 与 App 各读一次 `get_config`（本就各管一事，不为此拓宽 hook API）。

## Alternatives considered

- **JS 直调 `@tauri-apps/api/window` 的 setSize/setPosition + capabilities 扩容。** 输给了 Rust 命令：把几何政策（选屏、缩放、贴顶计算）摊到前端，还得永久放宽 capabilities；接口更宽而隐藏更少，正与 interface depth 相反。基线捕获/恢复也无法在前端安全地与 Rust 启动时序协作。
- **进入精简时临时切材质（hudWindow）、退出时按配置恢复（复用 apply_window_effects）。** 输给了"条自带 CSS 背景、材质零接触"：前者迫使 useFrozenBackground 增加 compact 感知与铺图/撤材质的跨命令时序编排，换来 56px 条上一档半透明观感；失败路径还会出现"配置 frozen、材质 follow、图未铺"的中间态。选项 c 一行 hook 改动都没有。
- **精简作为 App 级覆盖层（view 不变、条 overlay 在 usage 之上）。** 输给了 `'compact'` 进 view 状态机：覆盖层会保留底下整棵 usage 树的挂载（无谓渲染），且 navigate/空态逻辑要同时处理两层；view 值让"精简是主页的一种形态"在类型上显式，切换即卸载（仓库既定模式）。
- **把告警判级/主值选择整个下沉 Rust，新增 `get_compact_rows` 命令返回 UI-ready 行。** 输给了"加一个派生字段"：新命令会造一个平行 wire 形状镜像 ProviderStatus（type 增殖），而 `primary_quota` 复用既有 payload 通道就把规则送到前端；格式化本就是前端职责（主页已有 TS 格式化），不值得为此把展示策略冻结进 Rust。
- **几何持久化进 AppConfig（保存"进入前"的几何字段）。** 输给了运行时内存基线：跨重启几何本就不保留（今天也回到 conf 值），持久化基线会引入"什么时候算过期"的新不变量；内存槽 + 惰性捕获让每个会话自洽。

## Implementation reconciliation

（实现阶段填写：接受的偏离与接受来源记录在此。）

## Open questions and risks

- 精简条默认置顶（alwaysOnTop）是产品判断，依据"挖孔屏/常驻状态条"的隐喻。要不要改成"不置顶、如普通窗口"？实现上一行常量的事，请在走查时定夺。
- 宽度取"工作区 − 16px"接近全宽。若观感太宽，备选是 `min(工作区−16, 0.75×工作区)`，请走查后定数值。
- 主页 % 型主数字切主窗口口径（托盘同源）包含在本设计里。若希望精简模式 PR 保持行为纯净，可以把它拆成前后两个 PR，但两 PR 之间主页与条口径不一致。
- 必须实测（实施时验证，对应 grounding 五条风险）：(1) `Monitor::work_area()` 在本仓库 tauri 2.x 版本的可用性（编译期即可判定；不可用则按菜单栏高常量内缩，macOS 实测取值）；(2) setup 阶段窗口尚不可见时 `current_monitor()` 是否返回 Some（None 则回退 primary，再 None 则记日志跳过、靠退出/进入自救）；(3) `resizable:false` 窗口被 set_size 后用户拖动移动是否正常；(4) 56px 高 + 76px 红绿灯槽下 traffic lights 与 chip 的间距观感，以及条背景圆角在实机的形状；(5) 冻结背景图在退出恢复后的圆角（几何已复原，理论无错位，走查确认）。
- 本设计新引入的行为风险：主页告警变敏感（主窗口口径比 balance 口径更容易触发黄档），属预期修正而非回归，用例需覆盖 grounding 里"balance 62% 但 Opus 3%"的形态。

## Next implementation step

先落 Rust 数据地基：`models.rs` 两个 serde default 字段与配套 legacy-config 测试、scheduler 的 `primary_quota` 集中填充，然后写 `compact.rs` 的 `band_rect` 纯函数与其单测（不碰窗口 I/O 即可红绿验证），再接命令与前端。
