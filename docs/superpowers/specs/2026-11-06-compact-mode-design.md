# 精简模式（Compact Mode）设计

- 日期：2026-11-06
- 状态：待人工确认（确认后实施）
- 分支：feature/compact-mode（基于 develop 9d1d2fe）
- 设计过程：4 个并行候选（opus / fable / sonnet / haiku 各一份完整设计）+ 跨模型裁判盲评，本文为合成稿。候选与评分存档于 `.audit/compact-mode-*`。

## 1. 背景与目标

主页 720×600 占屏太大。用户要一个"精简模式"：

1. 主页右上角新增"精简模式"图标按钮（原话是"把设置按钮换成精简模式按钮"；PR #121 已先行移除主页设置/Skills 按钮、入口移入托盘，所以现在是新增按钮落位右上角，无按钮可替换）。
2. 点击后进入极度精简形态：供应商尽量一行内展示；窗口左右拉宽，自动贴屏幕最上方，观感如"挖孔屏扩大屏"。
3. 只突出供应商图标及名字、"剩余多少"、"额度重置（啥时候重置）"；其他信息缩小。
4. 有退出路径回正常模式并恢复窗口几何。

## 2. 决策概览（合成结论）

四候选在核心架构上完全收敛，直接采纳为定论：

| 收敛点 | 结论 |
| --- | --- |
| 几何所有权 | Rust 单命令独占全部运行时窗口几何（尺寸/位置/置顶），前端零窗口 API 调用，capabilities 零扩容 |
| 状态持久化 | `AppConfig.compact_mode: bool`（serde default false）；启动时 Rust 在窗口 show 之前预套几何，持久形态无闪变 |
| 窗口属性 | 不碰 decorations（规避 Overlay+set_decorations 未验证行为）；保留红绿灯，条内 76px 让位；alwaysOnTop 随进出切换 |
| 图标 | feather `minimize-2` 进 / `maximize-2` 出，与现有按钮同规格（18×18 inline SVG） |
| 不加的东西 | 设置页开关、托盘菜单项、Esc/双击退出、多行换行、宽度自适应 |

基准候选为 D（haiku 稿），裁判与 picker 独立评分后共同裁定其核心模块最优（结构编码的不变量 + 可组合 wire 原语）。嫁接清单：

- 自 A（opus）：`compact: boolean | null` 三态与启动 null 门（防"正常视图挤进条形窗口"首帧闪现）；材质/背景切换时序的注释纪律。
- 自 B（fable）：单命令原子落盘（persist 在命令内，`set_refresh_interval` 先例）；失败收敛表作为测试清单；config 幂等早退闸；`Monitor::work_area()` 在锁定版本 tauri 2.11.6 存在的实证（tauri-2.11.6/src/window/mod.rs:96，`PhysicalRect` 字段公开）。
- 自 C（sonnet）：启动路径走同一条捕获-再套用边沿（窗口仍在 conf 几何时快照即天然基线，消灭手抄 720×600 常量）；降级形态显式定义；单行横向滚动；空态带"打开设置"动作。

被否决的主要替代：JS 直调窗口 API（capabilities 扩容 + 几何策略散落前端 + 启动时序无解）；独立第二窗口（生命周期/托盘纪律全要复制一遍，状态分裂）；CompactCell 封闭投影（丢币种信息，第一次扩展就要动 wire 四处）；条内塞设置页（56px 放不下，navigate 先退出再切页已覆盖）。

## 3. 详细设计

### 3.1 状态与持久化

`AppConfig` 新增 `compact_mode: bool`，`#[serde(default)]`，旧配置零迁移。模式是常驻偏好（同 `background_mode` 先例）：重启（含 `--minimized` 自启后从托盘唤起）直接以条形态出现。

几何本身不持久化。普通形态永远以 tauri.conf.json 为事实源；"进入前的几何"只存进程内存。精简中退出应用，下次"退出精简"回到 conf 默认尺寸居中，与全应用"不记忆窗口位置"的现状一致。

### 3.2 wire 数据增量（types.ts ↔ models.rs 严格镜像）

```ts
// src/types.ts
export interface QuotaInfo {
  // …现有字段不动
  reset_at: string | null;            // 补声明：wire 上已有（models.rs serde default），实时倒计时数据源
}
export interface ProviderStatus {
  // …现有字段不动
  primary_quota: QuotaInfo | null;    // 主窗口（tray::primary_quota 选出：剩余%最低，差<0.5 并列取重置更近）
  warning_level: number;              // 0 平静 / 1 黄 / 2 红（tray::quota_warning_level，阈值只在 Rust 一处）
}
export interface AppConfig {
  // …现有字段不动
  compact_mode: boolean;
}
```

两个字段都在 `scheduler.rs` `poll_providers` 组装 `provider_statuses` 之后统一回填，复用 `tray.rs` 现有 pub 函数。`get_usage` 读路径与 `usage-updated` 事件同源，不存在分叉。前端不在 TS 复制主窗口挑选或告警阈值（主页 `warningLevel` 与托盘 `quota_warning_level` 的双实现漂移是既有事故，不再加第三份）。

精简条主值口径与托盘一致：percent 型取 `primary_quota.remaining_percent`（缺失回退 `balance.available_balance`），CNY/USD 取 `balance`。主页主数字维持现状（balance 口径），对齐主页是后续独立决策（见 §6.5）。

### 3.3 Rust：compact.rs（新模块，几何唯一所有者）

```rust
//! 精简模式窗口几何：进入 = 宽矮横条贴工作区顶端 + 置顶；退出 = 恢复进入前几何。
//! 全仓库运行时几何写点只此一处。

const BAR_HEIGHT: f64 = 64.0;        // 逻辑 px，走查校准
const BAR_WIDTH_RATIO: f64 = 0.8;    // × 工作区宽
const BAR_MIN_WIDTH: f64 = 560.0;    // 逻辑 px

pub struct GeometryBackup(std::sync::Mutex<Option<GeometrySnapshot>>);
// GeometrySnapshot = outer_position + outer_size（物理 px）

// 纯函数（单测锚点）：工作区 + scale → 目标条矩形（宽高逻辑 px、位置物理 px）
fn bar_geometry(work: &PhysicalRect<i32, u32>, scale: f64) -> (LogicalSize<f64>, PhysicalPosition<i32>);

// 进入：backup 为 None 才捕获（边沿捕获，防失败重进时把条几何存成基线）
//   → set_always_on_top(true) → set_size → set_position
fn enter_bar(window: &WebviewWindow, backup: &GeometryBackup, default_size: PhysicalSize<u32>) -> Result<(), String>;

// 退出：backup.take() 消费；Some → 原样恢复；None → default_size + 当前工作区居中
//   → set_always_on_top(false)
fn exit_bar(window: &WebviewWindow, backup: &GeometryBackup, default_size: PhysicalSize<u32>) -> Result<(), String>;

#[tauri::command]
pub async fn set_compact_mode(
    window: WebviewWindow, app: AppHandle,
    config_state: State<'_, Arc<RwLock<AppConfig>>>,
    health_state: State<'_, Arc<RwLock<ConfigHealth>>>,
    backup: State<'_, GeometryBackup>,
    enabled: bool,
) -> Result<(), String>;
// 1. 幂等闸：config.compact_mode == enabled → Ok（双击/重放安全）
// 2. persist_config 落盘（唯一落盘通道，set_refresh_interval 先例）
// 3. enabled ? enter_bar : exit_bar
```

选屏：`current_monitor()`（窗口当前所在屏），None 回退 `primary_monitor()`。贴顶：y = `work_area.position.y`（macOS 菜单栏/刘海高度由工作区天然扣除，无需常量）。宽度 = 工作区宽 × 0.8（≥560 逻辑 px），水平居中于工作区。

启动时序（lib.rs setup，窗口仍 `visible:false`）：加载 config → 捕获创建尺寸为 `default_size` → `compact_mode` 为真则先捕获当前（conf）几何为基线再套条几何 → Ready 阶段 show 时几何已正确。`--minimized` 自启不 show，几何已备好，托盘唤起即条形态。

失败收敛（实施时配测试逐格验证）：任何单步失败后用户再点一次目标按钮必收敛。三个守卫保证这一点：幂等闸以 config 为准；边沿捕获防止条几何覆盖有效基线；`take()` 消费防止二次恢复旧几何。

降级形态：启动时取不到显示器 → `log::error` + 保持正常几何，config 仍为真；前端按 config 渲染条于常规窗口内，条上退出按钮可自救。不为极端分支写恢复逻辑。

### 3.4 前端

**useCompactMode.ts（新 hook）**

```ts
export function useCompactMode(): {
  compact: boolean | null;   // null = 启动形态未决，App 首帧渲染空（防错形闪现）
  busy: boolean;
  enter(): Promise<void>;    // await invoke('set_compact_mode', { enabled: true }) 成功才翻转
  exit(): Promise<void>;     // 失败不翻转 + console.error（按钮回弹，同 useFrozenBackground 纪律）
}
```

启动 effect 读 `get_config` 定初值；null 期间 App 渲染 null。

**App.tsx 接线**

- `header-actions` 右端（＋供应商 之后）新增精简按钮：`minimize-2` SVG，`title="精简模式"`，class 沿用 `settings-icon-btn`。
- `compact === true` 时不渲染 `.background-frozen`、`UpdateNotice`、`version-tag`，整棵正常树替换为 `<CompactBar/>`。
- `navigate` 监听：payload 为 `'settings'` 或 `'skills'`（完整页面导航）时先 `exit()` 再 `setView`；`'usage'`（托盘"打开主页"）不退出，精简条即主页形态。

**CompactBar.tsx（新组件）**

单行布局：`[76px 红绿灯让位] [logo + TokenUsage 字标] [chips 横排，溢出横向滚动（滚动条隐藏）] [弹性空隙] [数据时间 时:分:秒 小字] [退出按钮 maximize-2]`。条背景 `data-tauri-drag-region`，chips 与按钮正常收事件。

每枚 chip：`[品牌 logo][alias（省略号截断）][主值（着色，tabular-nums，永不截断）][微标签][重置倒计时小字]`。

- 主值格式化复用从 `UsagePanel.tsx` 提取的 `formatBalanceValue`（两处共用一份实现）；USD 型蓝色点缀（`accent-blue`）。
- 着色由 `warning_level` 映射：0 无 / 1 黄 / 2 红（复用 `.p-card-warn-*` 同族视觉）；阈值只在 Rust。
- 倒计时：`reset_at` 可解析且未过期 → `countdownCompact`（TS 纯函数，格式对齐 Rust `countdown_compact`：12M / 1H4M / 6D / 3D4H，由 verify 用例以固定 `reset_at` 锚定）；不可解析/缺失 → 回退 `reset_time` 预格式化文本；已过期 → 不显示（不回退陈旧文本，对齐托盘语义）。30 秒 tick 重算，避免秒级重渲。
- 供应商级错误：chip 显示 logo + alias + 红"异常"角标，值位 `–`，错误全文进 `title` tooltip。
- 悬停 chip 的 tooltip 给全量配额窗口明细（多窗口不进条，托盘同款取舍）。

**空态（四个早退分支的一一映射，条内单行文案）**

| 主页语义 | 精简条 | 动作 |
| --- | --- | --- |
| loading | 加载中… | 无 |
| error | 获取数据失败 | 内联"重试"（调 `refresh()`） |
| 零配置 | 暂无供应商 | "打开设置"（exit 后 `setView('settings')`） |
| 全部禁用 | 所有供应商已禁用 | "打开设置"（同上） |

退出按钮在所有形态下恒在。

### 3.5 样式与背景

`app.css` 末尾追加"精简模式"一节（约 90 行）：`.compact-bar / -chip / -value / -label / -reset / -empty / -exit-btn`，只复用现有 CSS 变量与语义类，不开新文件。

精简态卸载 `.background-frozen`（静态图按 720×600 拉伸，条形下必然失真）。条自绘近不透明深色胶囊（`rgba(24,24,26,0.92)` + 圆角）作为默认表面；hudWindow 材质接管作为走查备选（一个 CSS 类的差别，实机对比后定稿）。材质模块（`useFrozenBackground` + `apply_window_effects`）零改动，两个窗口属性子集各归其主：材质归 background.rs，几何归 compact.rs。

### 3.6 verify 配套

- `tauri-shim.js`：新增 `set_compact_mode` handler（记录 `__VERIFY_MOCK__.compactCalls` 序列 + 翻转 fixture `config.compact_mode`）；usage fixture 补 `primary_quota` / `warning_level` / `reset_at`（含多窗口 Claude Code 样例）；新增 `fireNavigate(payload)` 助手。
- `features/compact-mode.md` 新用例：
  - Lane A（浏览器）：入口按钮在、点击后条渲染 chips（值/着色/倒计时与 fixture 断言）、四空态逐个驱动、退出回正常树、`compactCalls` 与 `savedConfigs` 断言、`fireNavigate('settings')` 在精简态先退出再切页。
  - Lane B（真机）：进出几何往返、贴顶贴合、拖动可用、置顶生效、重启直接进条、退出恢复原位。

## 4. 追踪表

需求 → 设计（Phase F 验证：每条需求有唯一落点）：

| 需求 | 落点 |
| --- | --- |
| 右上角图标按钮入口 | §3.4 App.tsx header-actions + minimize-2 |
| 供应商一行内展示 | §3.4 CompactBar 单行 + 横向滚动 |
| 窗口拉宽贴顶、挖孔屏感 | §3.3 bar_geometry（0.8×工作区、64px、work_area 顶）+ alwaysOnTop |
| 突出图标/名字/剩余/重置 | §3.4 chip 结构（主值永不截断，其余缩小） |
| 其他信息缩小 | 微标签/时间小字、明细进 tooltip |
| 退出恢复 | §3.3 exit_bar（内存基线 + conf 兜底） |
| 设置仍可达 | 托盘菜单既有入口（PR #121 已保障） |

约束 → 处置：capabilities 零扩容（走 Rust 命令）；托盘零改动（不加菜单项，绕开 set_menu SIGABRT 纪律与 sync_tray_click_flag 联动——精简不改窗口显隐，标志不受影响）；`save_config` 身份校验不受影响（新字段非身份键）；`navigate` 守卫覆盖 settings 与 skills（PR #121 后的两个完整页面目标）。

## 5. 实施计划（每步一个 commit，各自可验证）

1. **Rust 数据地基**：models.rs 三字段 + scheduler 回填 + 单测（legacy config 兼容、primary_quota/warning_level 用 tray 测试同款 fixture 断言）。验证 `cargo test`。
2. **compact.rs 几何核心**：`bar_geometry` 纯函数 + 单测、`GeometryBackup`、命令、lib.rs 装配（含 setup 预套与降级分支）。验证 `cargo test` + dev 真机手动进出一次。
3. **前端接线**：`useCompactMode`（null 门）、App.tsx 按钮与渲染门、navigate 守卫、背景/水印/横幅门控、`formatBalanceValue` 提取。
4. **CompactBar**：组件 + CSS 节 + 空态 + 倒计时。验证 verify 技能 Lane A。
5. **verify 配套**：shim + 用例文档 + SKILL.md 命令计数（14→15）。验证 Lane A 全绿。
6. **走查定稿**：宽/高/胶囊 vs 材质/红绿灯间距/贴顶观感，按实测调常量。验证 Lane B 清单。

## 6. 人工拍板项（均已按默认设计，可一句话推翻）

1. **置顶**：默认 alwaysOnTop 开（HUD 隐喻，不被其他窗口盖住）。关掉是一行常量。
2. **条宽**：默认工作区 80%（"左右适当拉大"的居中留边观感）。备选近全宽（工作区 −16）。
3. **持久化**：默认记住模式跨重启。备选每次启动回正常形态。
4. **倒计时**：默认 30 秒步进实时走动。备选随轮询的静态快照文本。
5. **主页口径**：默认本 PR 不动主页主数字（条与托盘同口径，主页维持 balance 口径，分歧为既有现状）；备选本 PR 顺带把主页 % 型切到主窗口口径（多一个红测与走查点，Claude Code 双窗口用户主页数字会变）。
6. **条高**：64 逻辑 px 初值，走查后定稿。

## 7. 风险与实测清单

已消掉：`Monitor::work_area()` 存在性（tauri 2.11.6 源码实证）；材质观感依赖（自绘胶囊默认）；frozen 图失真（精简态卸载）；set_decorations 行为（不调用）；首帧错形（null 门）。

待实机验证（Lane B 首轮）：`resizable:false` 下 set_size 后拖动与红绿灯交互；贴顶与菜单栏的贴合度；胶囊与材质版本对比；多屏/外接屏换算（无第二块屏则记为已知限制）；启动降级路径；红绿灯在 64px 高条内的垂直落点。
