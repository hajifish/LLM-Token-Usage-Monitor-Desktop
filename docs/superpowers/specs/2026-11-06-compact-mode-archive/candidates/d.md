# 精简模式（Compact Mode）设计

> architect Phase B runner 候选稿。共享输入：`.audit/compact-mode-grounding.md`（Phase A 合成）。
> 本稿为设计产出，未改任何源码。路径行号基于 main 检出 aa4d1eb。

## Problem

主页 720×600 三列卡片占了半块屏，但用户大部分时间只想瞟一眼"每家还剩多少、什么时候重置"。需求是把主页换成一条贴屏幕顶端的窄横条（"挖孔屏"隐喻），突出供应商图标+名字、剩余额度、重置时间。

这个仓库让该形状不显然的约束（来自 grounding）：

- **全仓库零运行时窗口几何代码**。尺寸/位置完全由 `tauri.conf.json:14-29` 静态声明；精简模式是第一次运行时 set_size/set_position，没有现成范式。可参照的模板只有 `background.rs:36-42` 的 `apply_window_effects`（Rust 命令改窗口属性 + 前端"读 fresh 配置→改一字段→落盘→调命令→失败回弹"纪律）。
- 窗口是 Accessory 菜单栏应用：CloseRequested = hide（`lib.rs:83-90`），`--minimized` 自启保持隐藏（`lib.rs:109-127`），托盘"打开主页/设置"随时 show 窗口（`tray.rs:489-505`）。精简形态必须在这三条显隐路径下都自洽。
- 告警判级已经是双实现（主页 `UsagePanel.warningLevel` vs Rust `tray::quota_warning_level`，grounding 明确点名的 drift），新表面不能再长出第三份阈值表。
- `primary_quota`（托盘"哪个配额窗口是主窗口"的挑选策略）只活在 `tray.rs:204-214`，未过 IPC。
- `.background-frozen`（`app.css:902-911`）是 `100% 100%` 拉伸的整窗静态图，窗口一变尺寸立即错形。
- capabilities 只授 `core:default` + event + `start-dragging`（`capabilities/default.json`）——JS 直调窗口 API 需要扩权。

## Usage（caller 视角，先于类型）

### 用户视角 UX 流程

1. 主页右上角原"设置"齿轮按钮的位置换成一个 `minimize-2`（feather，双箭头向内收拢）图标按钮，title"精简模式"。点击后：窗口瞬间变为贴屏幕顶端居中的一条宽横条（宽随启用供应商数、高 72 逻辑像素），主页内容替换为一条"精简条"，图标落盘为开启，下次启动直接以精简形态打开。
2. 精简条上每个供应商一枚"芯片"：品牌 logo（22px）+ 别名 + 主数字（剩余额度/配额剩余，告警色）+ 重置倒计时（"1H24M"式，每 30s 自走）。出错供应商显示红色"异常"角标。条右端依次是数据时间、手动刷新、`maximize-2` 退出按钮。
3. 条保持 alwaysOnTop 悬浮在其他窗口之上；条身空白区可拖动（现有 `start-dragging` 权限）。
4. 退出路径有三条，走同一个 `exit()`：点 `maximize-2`；点任意芯片（"看详情"意图）；托盘"设置"等导航事件到达时先退出再切视图。退出后窗口几何精确恢复进入前的物理位置/尺寸，主页照旧。
5. 配置持久化：精简中直接关窗（=hide）、退出应用、重开，托盘"打开主页"出现的就是精简条；autostart `--minimized` 静默驻留后再打开同理。
6. 设置入口消失的补偿：托盘菜单"设置"照旧可达（`tray.rs:412`）。设置页不新增精简模式开关（避免第二个写入口）。

### 前端调用点（真实代码）

```tsx
// src/App.tsx —— 调用点 1：入口按钮替换现设置按钮（L122-140 原位）
const compactState = useCompactMode();
// …
<button
  className="settings-icon-btn"
  onClick={() => compactState.enter()}
  disabled={compactState.busy}
  title="精简模式"
>
  {/* feather minimize-2：polyline 4 14 10 14 10 20 / 20 10 14 10 14 4，
      line 14 10 → 21 3 与 3 21 → 10 14（18×18, viewBox 24, stroke=currentColor,
      strokeWidth 2 —— 与现有按钮 SVG 同规格） */}
</button>

// src/App.tsx —— 调用点 2：渲染分支（L46 处插一层）
{compactState.compact ? (
  <CompactBar
    summary={summary} loading={loading} refreshing={refreshing} error={error}
    refresh={refresh} busy={compactState.busy}
    onExit={() => { compactState.exit(); setView('usage'); }}
  />
) : (
  /* 现有 usage/settings/providers/skills 四分支原样不动 */
)}
// 同一分支里冻结背景层条件化为：{!compactState.compact && background.imageUrl && …}（L39-44）

// src/App.tsx —— 调用点 3：托盘 navigate 守卫（L24-35 监听器内）
listen<string>('navigate', async (event) => {
  if (event.payload === 'settings' || event.payload === 'usage') {
    if (compactState.compact) await compactState.exit();  // 任何视图切换前先恢复几何
    setView(event.payload);
  }
});

// src/hooks/useCompactMode.ts —— 消费面（README 式）
const { compact, busy, enter, exit } = useCompactMode();
// enter(): 读 fresh 配置 → compact_mode=true → save_config → invoke('enter_compact_mode')
//          → 才翻转本地 compact。任一步失败：不翻转，且 best-effort 回写 compact_mode=false。
// exit():  镜像。两命令皆幂等，重复调用收敛同一终态。
```

### Rust 命令签名（真实调用点）

```rust
// src-tauri/src/compact.rs（新模块，照 background.rs 命令模板）
#[tauri::command]
pub async fn enter_compact_mode(
    window: tauri::WebviewWindow,
    config_state: tauri::State<'_, Arc<RwLock<AppConfig>>>,
    backup: tauri::State<'_, GeometryBackup>,
) -> Result<(), String>;

#[tauri::command]
pub async fn exit_compact_mode(
    window: tauri::WebviewWindow,
    backup: tauri::State<'_, GeometryBackup>,
) -> Result<(), String>;

// 共享内核（非命令）：Ready 启动路径与命令体共用，保证"配置为真 ⇒ 几何为精简"
pub(crate) fn apply_compact_geometry(window: &tauri::WebviewWindow, enabled_count: usize) -> Result<(), String>;
pub(crate) fn restore_normal_geometry(window: &tauri::WebviewWindow, backup: &GeometryBackup) -> Result<(), String>;
```

`lib.rs` 改两处：`invoke_handler`（L91-106）注册两个命令；`RunEvent::Ready`（L112-125）在 show 与否**之前**执行——`if config.compact_mode { apply_compact_geometry(&window, enabled) }`。hidden 窗口同样应用几何：几何是窗口属性，与可见性正交，托盘任意 show 路径出现的即是精简形态。

## Shape（数据结构先行）

### 线上模型增量（单一事实源在 Rust，TS 严格镜像）

```rust
// src-tauri/src/models.rs —— 三处 #[serde(default)] 增量，旧盘配置零迁移
pub struct AppConfig { …, pub compact_mode: bool }                        // 默认 false
pub struct ProviderStatus {
    …,
    pub primary_quota: Option<QuotaInfo>,   // 主窗口（策略复用 tray::primary_quota）
    pub warning_level: u8,                  // 0/1/2（策略复用 tray::quota_warning_level）
}
// QuotaInfo.reset_at（models.rs:140-141）已在 wire 上，仅 TS types.ts:16-25 补声明。
```

```ts
// src/types.ts 增量（与 Rust serde 输出严格一致，文件头纪律）
interface QuotaInfo     { …existing…; reset_at: string | null; }
interface ProviderStatus{ …existing…; primary_quota: QuotaInfo | null; warning_level: number; }
interface AppConfig     { …existing…; compact_mode: boolean; }
```

派生只写一处（scheduler.rs `poll_providers`，`provider_statuses` 组装完成之后、L638 构造 `UsageSummary` 之前）：

```rust
for p in &mut provider_statuses {
    p.primary_quota = p.quota_infos.as_ref().and_then(|qs| crate::tray::primary_quota(qs)).cloned();
    p.warning_level = crate::tray::quota_warning_level(p);
}
```

这一个咽喉点同时覆盖 `get_usage` 读、`usage-updated` 事件、托盘菜单与通知四条下游——不存在"事件有、get_usage 没有"的分叉。策略零新增：两个字段都是对既有 `tray.rs` 纯函数的复用（model-the-domain：把"哪个窗口是主窗口""该多 alarmed"编码为数据，而不是让每个表面各答一遍）。

### 精简条信息模型（每枚芯片）

| 槽位 | 取值口径 | 来源 |
|---|---|---|
| logo + 名字 | `BrandLogo(p.name)` + `p.alias` | 复用 `UsagePanel.tsx:16-24`（加 `export`） |
| 主数字 | `p.balance` 为 percent 型 ⇒ `p.primary_quota.remaining_percent` 格式化为 `N%`；否则 `formatBalanceValue(p.balance)`（¥/$/格式从 `UsagePanel.tsx:104-117` 提取为导出函数，两处共用防漂移）。无 balance ⇒ 主数字槽显示"–"（与托盘 `tray_value` 同语义） | wire 字段 |
| 告警着色 | `p.warning_level`：0 无 / 1 黄（边框+数字橙）/ 2 红。阈值表只在 Rust 一处（quota_warning_level），TS 侧只做 0/1/2 → class 映射 | wire 字段 |
| 重置倒计时 | `p.primary_quota.reset_at`（ISO）⇒ TS 纯函数 `countdownText(reset_at, now, reset_time)` 实时换算"1H24M"式（30s tick 重算）；`reset_at` 缺失回退 `reset_time` 预格式化文本；已过期**隐藏**不回退陈旧文本（与 Rust `quota_countdown_compact` 语义一致）。非 percent 供应商无倒计时槽 | wire + 视图 |
| 出错态 | `p.error` ⇒ 红"异常"角标，主数字槽显示"–" | 现有字段 |

主数字与菜单栏托盘同源（aggregate_value 口径），与主页大数字在"多配额窗口且 5h 不是最紧"的少数场景会不一致——这是**既已存在**的托盘 vs 主页分歧（claude_code.rs 取 5h 优先，tray 取最低），精简条选择加入"最常驻表面一致"的一侧，理由见 Tradeoffs。

次要信息压到条右端一小簇：`last_updated` 时:分:秒（11px）+ 14px 刷新按钮 + 退出按钮。没有其他内容（subtract-before-you-add：不塞 today tokens、plan_label、余额拆分）。

### 窗口几何：所有权与算法（compact.rs 单一所有者）

```rust
pub struct GeometryBackup(std::sync::Mutex<Option<GeometrySnapshot>>);  // app.manage 托管
#[derive(Clone, Copy)]
struct GeometrySnapshot { x: i32, y: i32, width: u32, height: u32 }     // 全部物理像素
```

- **选屏**：`window.current_monitor()`（窗口当前所在屏，绝不跳屏）；None（隐藏窗口/未定位过）回退 `primary_monitor()`；再 None 报错返回。
- **贴顶**：用 monitor 的 `work_area`（工作区顶 = 菜单栏下沿，自动吃掉 notch/菜单栏高度；API 可用性列入实测项 R3）。y = work_area.top，x = work_area 水平居中。
- **尺寸**：高固定 72 逻辑像素；宽 = `clamp(96 + 230 × n, 560, ⌊0.92 × work_area 宽⌋)`，n = **配置里 enabled 供应商数**（不是 summary 行数——Ready 时 summary 尚空，且配置数与轮询后可见行数同源同值；providers/settings 页都先退精简才可改配置，故 n 在一次精简会话内稳定，几何只在进入/启动时算两次，无连续 re-fit）。逻辑值 × `scale_factor` 取整为物理值，`set_size/set_position` 传 Physical——跨 DPI 无浮点往返。
- **备份与恢复**：`GeometrySnapshot` 仅在"正常→精简"这条**边**上写入（已有 Some 则不覆盖——重复 enter 保留最早的原几何）；`exit` 用 `take()` 消费；消费到 None（崩溃恢复/重启后退出）回退 conf 默认 720×600 并在当前屏居中。备份只在内存：重启后失去原几何是已接受代价（见 Tradeoffs），换来不加配置字段、避开换屏/拔屏后的陈旧坐标（make-operations-idempotent：每步从现存状态推导，重复执行收敛）。
- **置顶不变量**：`set_always_on_top(true)` 只发生在 `apply_compact_geometry`，`set_always_on_top(false)` 只发生在 `restore_normal_geometry`——"置顶 ⇔ 精简几何"由单一所有者编码，无人能只改一半（encode-lessons-in-structure）。
- **capabilities 零扩容**：全部窗口操作走 Rust 命令，不加任何 `core:window:allow-*` 权限。

### 精简条窗口属性

- **不动 decorations**：保持 Overlay 标题栏与红绿灯（grounding 风险 R2 整体绕开，零 `set_decorations` 调用）。条身左侧留 `--compact-left-inset: 76px`（与 `.app-header` 同值，独立 token 便于实测校准红绿灯在 72px 高窗口里的落点）。
- **拖动**：条身空白区 `data-tauri-drag-region`（权限已有）；芯片、按钮拦截点击。拖离顶端不禁止——退出时恢复备份几何。
- **表面**：精简时前端卸载 `.background-frozen` 图层，芯片条自绘不透明深色胶囊 `rgba(24,24,26,0.92)` + 16px 圆角（窗口本身 transparent:true，胶囊即"挖孔"）。**不调 `apply_window_effects`、不动材质**：hud 材质在宽矮条上的观感（R1）不再是本设计的依赖项，材质启停仍归 `useFrozenBackground` 独占（两个 actor 各写各的窗口属性子集：材质归 background.rs，几何/置顶归 compact.rs，无共享写——separate-before-serializing）。尺寸切换瞬间的错形帧处理：接受 1 帧（≤16ms）瞬态，方向为"图先盖着 → 切几何 → React 同一 commit 换 DOM 卸图"（进入）/ "恢复几何 → 同一 commit 换回 DOM 与图"（退出），列入实测项 R5。

### 前端状态与组件

```ts
// src/hooks/useCompactMode.ts —— 唯一写者（App 持有实例；Settings 页不开第二个入口）
export function useCompactMode(): {
  compact: boolean;          // 渲染分支唯一开关
  busy: boolean;             // 在途禁点
  enter(): Promise<void>;
  exit(): Promise<void>;
}
// 启动 effect：get_config 发现 compact_mode=true → 直接置 compact=true（几何 Rust 已在 Ready
// 应用，前端零 IPC），模式同 useFrozenBackground.ts:42-62 的启动采纳样板。
// enter/exit 遵循配置纪律：get fresh → 只改 compact_mode → save_config → invoke 命令 → 翻转；
// invoke 失败 → best-effort 回写 false → 抛错（busy 释放，按钮可用，状态未翻转=回弹）。
```

```tsx
// src/components/CompactBar.tsx —— 唯一新组件
interface CompactBarProps {
  summary: UsageSummary | null;
  loading: boolean;
  refreshing: boolean;
  error: string | null;
  refresh: () => void;
  busy: boolean;
  onExit: () => void;
}
```

四个空态/错误态（与主页四分支一一对应，全部装在固定条内）：

1. **loading**（首载无数据）：条中一行 `加载中…` 弱文本。宽度按配置 n 已定，不闪跳。
2. **error**（get_usage 失败）：`数据获取失败`（红色弱文本）+ 条右端刷新按钮可用（refresh 链路复用，成功即自愈）。
3. **零配置**：`暂无供应商` 文本，整条可点 ⇒ `onExit()` 回主页由既有指引接手（单一代价最低的出口，不新造导航语义）。
4. **全禁用**：`所有供应商已禁用` 文本，同上可点退出。

退出交互只有一条代码路径 `exit()`，三个触发器（maximize-2 按钮 / 芯片点击 / navigate 守卫）——"一个任务一种做法"。

### 接口深度判断

新增公共面 = 2 个 Rust 命令 + 1 个 hook（2 方法）+ 1 个组件（7 props，全是从 App 已有状态直传的数据）。藏在面后的：显示器几何与 scale factor 换算、备份生命周期、置顶不变量、主窗口挑选策略、告警阈值、四个空态、30s 倒计时。调用方（App.tsx）净增约 15 行。一个 agent 从任何单文件看，改芯片口径只动 compact.rs/scheduler.rs，改条外观只动 CompactBar.tsx/app.css 尾部——"看起来对的改动在整仓库都是对的"。

### 明确不做的事（scope 边界）

- 不做窗口尺寸随供应商增删的运行中自适应重算（只在进入/启动时算）。
- 不做多行换行（超宽芯片名省略号降级，坚持单行）。
- 不做精简条与主页间的形变动画（瞬时切换）。
- 不做 Esc 键退出、不做双击退出（三个触发器足够）。
- 不在设置页加精简模式开关（避免第二个写入口）。
- 不把主页 UsagePanel 迁移到 `primary_quota`/`warning_level`（新字段纯增量，主页迁移是后续独立小 PR）。
- 不加入全屏空间（NSWindow collectionBehavior 未被 Tauri 2 WebviewWindow 暴露；全屏应用可能盖住条，记为已知限制，不做 objc 裸调）。
- 不新增任何 npm/Cargo 依赖（图标为 inline feather 风格 SVG，同现有按钮）。
- 不为 Windows/Linux 调几何行为（work_area/菜单栏语义按 macOS 实测；其他平台继承尽力而为）。

## Synthesis decision

（arena 合成阶段填写——本文件为候选稿。）

## Tradeoffs accepted

- 我们接受**重启丢失"原几何"备份**（内存备份 + 退出回退 720×600 默认），换取不加配置字段与陈旧坐标免疫。重启后退出精简的用户会回到默认位置，属可感知但低频。
- 我们接受**精简条主数字与主页大数字在多配额窗口场景的不一致**，换取与菜单栏托盘（最常驻表面）三方一致。该分歧今天已存在于托盘 vs 主页之间，本设计只是选边，未新增分歧源。
- 我们接受**尺寸切换的 1 帧错形/胶囊尺寸瞬态**，换取不做窗口动画 API 与跨 IPC 时序编排。真机上若观感刺眼，备选方案是进入时先卸图再切（把帧让给材质态），一行顺序对调。
- 我们接受**76px 红绿灯死区**占掉约 1/3 枚芯片位，换取零 decorations 运行时操作（R2 整体绕开）。若实测观感不可接受，调 `--compact-left-inset` 或后续立项隐藏红绿灯，不属于本设计。
- 我们接受**30s 倒计时粒度**（分钟级显示足够），换取不引入秒级 re-render。
- 我们接受 `resizable:false` 语义扩展为"程序可改、用户不可拖拽缩放"——这正是想要的（set_size 不受 resizable 影响是 Tauri 常规行为，仍列 R4 实测）。
- 看似遗漏实则刻意：`warning_level` 让主页与新表面出现"同类信息两种来源"——它是消灭第三份 TS 阈值表的代价，主页迁移后该字段成为唯一事实源。

## Alternatives considered

1. **JS 直调 `@tauri-apps/api/window`（setSize/setPosition/setAlwaysOnTop）+ capabilities 扩容**。输在接口深度：几何策略（选屏、work_area、scale、备份）被迫整体搬到 TS，capabilities 新增 4-5 条 allow 权限扩大安全面，且启动路径（Ready 时前端还没跑）仍需一份 Rust 实现——同一策略两份实现、两个语言。本设计把策略收敛在 compact.rs 一个语言一处。
2. **独立第二 WebviewWindow 作为精简条**（常规窗口照旧，条是另一个窗口）。输在生命周期面：新窗口的 show/hide/focus/托盘 sync_tray_click_flag、capabilities windows 匹配、close 语义都要为它重新走一遍，且两个窗口同时存在时"主页 vs 条"的状态同步（同一份数据两处渲染）制造 split ownership。单窗口双形态保持仓库"唯一主窗口"前提，几何切换只是属性变更。
3. **纯前端方案**（CSS transform: scale + position:fixed 伪造贴顶条，不动窗口）。输在根本不满足需求：窗口矩形不变，"挖孔屏"只是视觉错觉，遮挡与命中区仍是 720×600，且 transparent 窗口缩放会糊化文字。此路否决是约束性的（需求明确要求窗口变宽贴顶），不是权衡。
4. **不加 wire 字段，TS 端复刻 primary挑选 + 阈值表**。输在 boundary-discipline 与既有教训：quota 挑选含并列 tie-break（<0.5 取重置更近）这类策略复制到 TS 即第三份告警口径（grounding 已点名双实现 drift）。两个 serde-default 字段换三处对齐（主数字/倒计时配对/着色），是这个仓库里最便宜的正确做法。
5. **精简条复用 hudWindow 材质**（enter/exit 内编排 apply_window_effects）。输在时序耦合：材质启停、冻结图卸载、几何切换三个异步步骤跨 IPC 编排四态，且 R1（hud 在宽矮条的观感）从"绕开"变"依赖"。自绘胶囊让材质域与几何域彻底解耦。

## Red-flag 自检（design-red-flags.md）

- Shallow module：useCompactMode 两个方法掩盖整条配置纪律与 IPC 编排；compact.rs 两个命令掩盖显示器数学与备份生命周期——均为深模块。无 options 暴露内部阶段（命令无参数泄漏策略）。
- Information leakage：`primary_quota`/`warning_level` 是域级派生事实而非传输形状（UsageSummary 本就是 UI 共享模型，`total_balance` 先例）；格式化（¥/$/%符号、倒计时文案）留在视图层。`GeometrySnapshot` 不出 Rust 模块。
- Temporal decomposition：compact.rs 按所有权（几何知识）分组，命令/启动/恢复三个时机的代码住在一起，正确。
- Pass-through：`apply_compact_geometry` 被 Ready 与命令体共用，是策略复用不是转发。
- Split ownership：compact_mode 全仓库一个写者（useCompactMode 的纪律路径）；GeometryBackup 一个写者（compact.rs）；材质一个写者（background.rs）。精简状态没有前后端双写——前端写配置（意图），Rust 写窗口（效果），各自独立收敛。
- Two ways to do one task：退出只有 `exit()`；几何只有 compact.rs；格式化主数字共用 `formatBalanceValue`。navigate 的 'skills' drift（tray 已 emit 但前端不认）是现存问题，本设计不在守卫里顺手扩大 payload 白名单（scope 外）。
- Hand-synced list：types.ts ↔ models.rs 镜像是仓库既有纪律（文件头注释即守卫）；0/1/2 → class 映射只有 CompactBar 一处。

## Open questions and risks（实施中必须实测验证）

 grounding 五条风险的归宿：

- **R1 hudWindow 宽矮观感**：已从依赖降级为无关（自绘胶囊）。需实测：胶囊 rgba(24,24,26,0.92) 在亮色壁纸上的可读性、16px 圆角在透明窗口上的实际观感。
- **R2 Overlay + set_decorations(false)**：设计零调用，风险不适用。但**红绿灯在 72px 高 Overlay 窗口内的垂直落点**是新的实测点（决定 `--compact-left-inset` 与条高微调）。
- **R3 屏幕几何 API**：必须实测 `tauri::Monitor` 的 work_area 可用性与语义、`current_monitor()` 在隐藏窗口（autostart --minimized 后 Ready）返回 None 的回退路径、多屏 + 非 1.0 scaleFactor 的物理换算。这是全仓库第一个运行时几何调用点。
- **R4 resizable:false + set_size**：预期程序化 set_size 不受阻、拖拽缩放仍禁用、`start-dragging` 在条上仍工作。三者一并实测。
- **R5 冻结背景切换瞬态**：设计选择"卸图层 + 1 帧接受"。实测两方向瞬态帧是否可见刺眼；刺眼则对调顺序（见 Tradeoffs 备选）。

 新增待决问题：

- alwaysOnTop 在 macOS 全屏应用前不显示（无 collectionBehavior API）——用户若在全屏工作流中依赖精简条，需决定是否后续用裸 objc 补 `CanJoinAllSpaces`（本设计明确不做）。
- 精简态下 `navigate 'settings'` 先退出再切视图会让窗口闪一次正常尺寸再加载设置页——是否可接受需走查；替代方案（设置页塞进条内）明确否决（scope）。
- 宽度公式 `96 + 230n` 的 230/96 与高 72 是纸面值，需以真实 9 家供应商数据走查后定版（experience-first：纸面推导不替代实机观感）。

## Next implementation step

先落 Rust 地基：`models.rs` 三个 serde-default 字段 + scheduler 派生循环 + `compact.rs`（含 GeometryBackup、几何数学与两命令）+ `lib.rs` 注册与 Ready 应用，配 `cargo test` 与托盘/通知载荷的手工核对——这一步不碰前端，`get_usage` 返回新字段即可单点验证派生正确，然后再写 useCompactMode 与 CompactBar。

## 模块地图（改哪些文件、各加什么）

| 文件 | 改动 |
|---|---|
| `src/types.ts` | +3 字段声明（QuotaInfo.reset_at 补声明、ProviderStatus.primary_quota/warning_level、AppConfig.compact_mode） |
| `src/App.tsx` | 设置按钮（L122-140）→ minimize-2 精简入口；L46 渲染分支插 CompactBar 层；L39-44 冻结图层条件化；L24-35 navigate 守卫；挂 useCompactMode |
| `src/hooks/useCompactMode.ts` | 新建（~90 行）：compact/busy/enter/exit + 启动采纳 effect + 配置纪律编排与失败回写 |
| `src/components/CompactBar.tsx` | 新建（~180 行）：胶囊条、芯片、四空态、30s 倒计时 tick、inline SVG（maximize-2 退出、刷新复用现有 path） |
| `src/components/UsagePanel.tsx` | `BrandLogo`（L16-24）与主数字格式化（L104-117 提取为 `formatBalanceValue`）加 export，两处共用 |
| `src/styles/app.css` | 尾部追加 `/* ===== 精简模式 ===== */` 一节（~90 行）：.compact-bar / .compact-chip(+warn-yellow/-red) / .compact-main / .compact-reset / .compact-actions / .compact-empty / `--compact-left-inset` |
| `src-tauri/src/models.rs` | AppConfig +compact_mode；ProviderStatus +primary_quota +warning_level（均 `#[serde(default)]`） |
| `src-tauri/src/compact.rs` | 新建（~140 行）：GeometryBackup 托管态、GeometrySnapshot、几何数学（选屏/work_area/scale/clamp）、apply/restore 共享内核、2 个 `#[tauri::command]` |
| `src-tauri/src/scheduler.rs` | poll_providers 内 `provider_statuses` 组装后加 2 行派生循环（L638 前） |
| `src-tauri/src/lib.rs` | `mod compact`；manage(GeometryBackup)；invoke_handler 注册 2 命令（L91-106）；Ready（L112-125）按配置应用精简几何（show 判定之前） |
| `src-tauri/capabilities/default.json` | **零改动**（全走 Rust 命令） |
| `.claude/skills/verify/mock/tauri-shim.js` | +2 命令 handler（记录进 `__VERIFY_MOCK__.geometry` 数组并翻转 fixture `config.compact_mode`）；usage fixture 补 primary_quota/warning_level/reset_at 字段样例（含多窗口 Claude Code） |
| `.claude/skills/verify/`（用例） | 新增走查：点入口→geometry 记录含 enter + 条渲染 3 芯片（智谱行红"异常"）；点芯片→exit 记录 + 回主页；精简中触发 navigate 'settings'→先 exit 再切设置页；fireUsageUpdated 空配置→条空态可点退出；倒计时随 tick 收缩；busy 期按钮 disabled |

调用链自检：App.tsx（按钮/分支）→ useCompactMode（配置纪律）→ invoke → compact.rs（几何）——数据从输入到输出三层，无更深的转发层；CompactBar 展示数据直接来自 App 已持有的 useUsage 状态，不新增订阅。
