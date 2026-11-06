# Compact Mode 设计 grounding（Phase A 产物）

> **2026-11-06 增补（arena 裁判发现的的前提变更）**：PR #121（71efe9f，9d1d2fe 合入 develop）已把主页头部 Skills 与设置按钮移除，入口移入托盘菜单（新增 "Skill & Plugin" 项，navigate 通道现认 'settings'|'skills'|'usage'）。本文正文中涉及 App.tsx:122-140 设置按钮的行号与"设置按钮在主页"的描述在 develop 9d1d2fe 之后过时。对本设计的影响：(1) 精简模式按钮直接新增于 header-actions 右端（现只有 时间/刷新/＋供应商），无需替换；(2) navigate 守卫需覆盖 'settings' 与 'skills' 两个视图切换目标；(3) 托盘菜单已有设置与 Skills 入口，"设置仍可达"约束自动满足。

本文件是 architect Phase B 全体 runner 的共享输入。第一部分是产品需求（用户原话转述），第二部分是子系统架构摸底（3 个 explorer + 1 个 explainer 合成）。路径均相对仓库根 `/Users/shin/dev/LLM-Token-Usage-Monitor-Desktop`。

## 一、产品需求（用户 2026-11-06 原话转述）

新功能"精简模式"，因为当前页面占屏太大。

1. **入口**：主页右上角，把现有"设置"按钮（`src/App.tsx:122-140`）换成一个新按钮，叫"精简模式"。图标按钮，选一个合适的 feather 风格图标表达入口含义。
2. **布局**：点击后主页进入极度精简的模式，更少信息、更简单布局展示完整信息。
   (a) 供应商展示：当前供应商都展示出来，尽量在一行内。
   (b) 页面位置：左右距离适当拉大（窗口变宽），自动放在屏幕最上方，看起来像"挖孔屏"扩大屏的感觉。
3. **信息**：压缩到更小的屏幕上，只展示最关键信息。
   (a) 突出展示：供应商图标及名字、"剩余多少"（剩余额度）、"额度重置"（啥时候重置）。
   (b) 其他信息：都缩小展示。

隐含要求（设计必须覆盖）：有退出路径回正常模式；窗口几何要恢复；设置入口被替换后仍可达（托盘菜单已有"设置"）。

## 二、架构摸底（合成版）

### Overview

Tauri 2 + React 18 的 macOS 菜单栏（Accessory，无 Dock 图标）常驻小窗应用：唯一主窗口 720×600、resizable:false、transparent + hudWindow 材质 + Overlay 标题栏。前端刻意极简——约 13 个 TS/TSX 文件 + 一个 1101 行扁平全局 CSS（`src/styles/app.css`），无路由库、无状态库、无 UI 库、无图标库，运行依赖仅 react@18.3 / react-dom / @tauri-apps/api / plugin-autostart。"主页"是 `App.tsx` 里 `view` 状态机（`'usage'|'settings'|'providers'|'skills'`）的条件渲染分支；数据由 Rust Scheduler 轮询后经 `usage-updated` 事件推送，前端纯展示。

**全仓库零运行时窗口几何代码**。窗口尺寸/位置完全由 `tauri.conf.json:14-29` 静态声明（历史 780×540 → 720×520 → 720×600 都只改 conf）；前端从未引用 `@tauri-apps/api/window`；Rust 侧唯一运行时窗口属性变更是 `background.rs:16-42` 的 `apply_window_effects`（hudWindow 材质启停）。精简模式的"更宽更矮、贴屏幕最上方"是仓库第一次运行时几何操作，没有现成范式，但 `apply_window_effects` 提供了"Rust 命令改窗口属性"模板。

### Key Concepts

- **view 状态机**：`App.tsx:15` useState 条件渲染整个分支，切换 = 卸载旧分支挂载新分支，无共享 context。
- **数据流**：Rust `Scheduler`（scheduler.rs:539 poll_providers，默认 180s）→ 写 `UsageSummary` RwLock → `emit("usage-updated")`；前端 `useUsage` 挂载 `invoke('get_usage')` + 常驻监听。手动刷新 = `trigger_refresh` 唤醒 Notify，等事件 10s 超时兜底。
- **数据形状**（snake_case 与 Rust serde 严格一致）：`UsageSummary{providers: ProviderStatus[], total_balance, last_updated}`；`ProviderStatus{name, alias(后端已回退恒非空), enabled, balance, usage, error, quota_infos}`；`BalanceData{available_balance, currency("CNY"|"%"|"USD"), voucher_balance, cash_balance}`——currency 三态决定主数字语义（%→配额剩余；USD→本月已花费；CNY→剩余余额）；`QuotaInfo{name, remaining_percent, reset_time(后端预格式化"1H24M"/"6D"), plan_label, used, total}`。
- **`reset_at` 类型缺口**：Rust `QuotaInfo`（models.rs:140-141）带 `#[serde(default)] reset_at: Option<String>`（ISO 8601 UTC），wire payload 有，但 TS `types.ts:16-25` 未声明。补一行类型即可做实时倒计时，零后端改动。
- **percent 型"剩余"两层**：`balance.available_balance` = 后端挑好的主窗口剩余（claude_code.rs:68-76，five_hour 优先回退 seven_day）；`quota_infos` = 全部窗口平铺（Claude Code 可有 5h/7d/7dOpus/7dSonnet 多行）。一行只放一个"剩余"取 balance 即与托盘、主页主数字三方同源。
- **托盘"一行 N 供应商"先例**（tray.rs）：`tray_short_code`（Z/K/KC/DS/OA/CX/A/CC/QD）+ 主值 + `countdown_compact`（12M/1H4M/6D/3D4H）+ 紧张优先排序 + ⚠️ 前缀 + 同短码多账号加序号。`primary_quota`（tray.rs:204-214）= 剩余百分比最低者（差<0.5 并列取重置更近），未通过 IPC 暴露给前端。
- **顶栏按钮组**（App.tsx:57-141）：header-time → 刷新(28×28) → ⚡Skills → ＋供应商 → ⚙设置；后三个共用 `settings-icon-btn` class（app.css L114-149，32×32 透明底）。图标全为手写 feather 风格 inline SVG（18×18, viewBox 24, stroke=currentColor, strokeWidth 2）。新图标 = 复制同风格 SVG path。
- **`.app-header`**（App.tsx:48）：`data-tauri-drag-region` 拖动；CSS `padding: 12px 16px 12px 76px`，左侧 76px 留 macOS 红绿灯（titleBarStyle Overlay）；`.app-title` 子元素 `pointer-events:none` 放行拖动。76px 在 Windows/Linux 也是死空格（无 cfg 补偿，仓库仅 menu_align 与 Accessory 两个 cfg(target_os) 门）。
- **`card-grid`**：`repeat(3, minmax(0,1fr))` gap 15（app.css:185-189，注释明说 fixed 3 columns at 720px）。
- **冻结背景**：`.background-frozen`（app.css:902-911）铺随包静态图 `public/default-background.png`，`background-size:100% 100%` + border-radius 10px。**窗口尺寸一变，图的比例和圆角就错**，切尺寸瞬间要处理。
- **配置纪律**：AppConfig 在 Rust 侧（models.rs:63-90），加密信封落盘（config.rs，临时文件 + rename 原子替换，load 失败只回默认绝不写盘）。前端无配置缓存，纪律 = 「get_config 读 fresh → 只改一个字段 → save_config → invoke apply 命令 → 失败 UI 回弹」。新字段用 `#[serde(default)]`。样板 = `useFrozenBackground.setMode`（useFrozenBackground.ts:64-90，含方向性防闪时序：frozen 先铺图后移除材质，follow 先恢复材质再撤图）。
- **capabilities**：`src-tauri/capabilities/default.json` 只授 core:default + event + `core:window:allow-start-dragging` + log。JS 直调 setSize/setPosition/setAlwaysOnTop 须加权限；走 Rust 命令不需要。
- **窗口生命周期**：setup（lib.rs:57-82）→ create_tray → Scheduler → `set_activation_policy(Accessory)`；窗口 visible:false，`RunEvent::Ready`（lib.rs:109-127）无 `--minimized`（autostart 固定传）才 show+focus；CloseRequested = hide 不退出（lib.rs:83-90）。conf 无 label，Rust 全用 `get_webview_window("main")`。
- **托盘纪律**：(1) 菜单整体 set_menu 与用户交互并发 = AppKit panic = release SIGABRT，必须"一次性装配 + 句柄托管原地 set_text"（TrayUpdateItem 范式，tray.rs:11-43）；(2) 每条窗口显隐路径后必须 `sync_tray_click_flag`（tray.rs:393-405）；(3) `navigate` 事件前端只认 'settings'/'usage'（托盘 emit 的 'skills' 是现存 drift）。
- **主题**：只有深色，`:root` 硬编码 macOS system colors dark（app.css:1-28）。现成语义类：`.meter-green/orange/red`、`.accent-green/.accent-blue`、`.p-card-warn-yellow/-red`、`.brand-logo-img`（24×24 圆角 7）。`BRAND_LOGOS`（UsagePanel.tsx:4-14）双路解析 `public/providers/*.png` / 首字母色块。
- **告警判级双实现**：主页 `warningLevel`（UsagePanel.tsx:47-54，只看 balance：USD 恒不告警、<=0 红、%<5 黄、CNY<10 黄）vs Rust `quota_warning_level`（tray.rs:99-116，%型按主配额 remaining_percent 更准）。
- **UsagePanel 4 个早退分支**：loading → error → 零配置 → 全禁用（app.css 对应空态）。
- **verify 配套**：`.claude/skills/verify/mock/tauri-shim.js` 只 mock invoke 命令表 + 事件，不 mock window 模块——新增窗口 IPC 或视图分支要同步扩 shim 和用例。
- **历史决策**：2026-10-31 紧凑化 spec 明确"固定紧凑尺寸、不做高度自适应"（docs/superpowers/specs/2026-10-31-main-window-compact-design.md:21,89）——动态算高度是逆既有决策，需新决策记录。"禁用供应商主页不显示"也是既定决策。

### Where Things Live

```
src/App.tsx                      视图状态机 + 顶栏按钮组（设置按钮 L122-140）+ navigate 监听
src/main.tsx / src/types.ts / src/providerMeta.ts
src/hooks/useUsage.ts            数据订阅
src/hooks/useFrozenBackground.ts 背景模式 + 配置「即时生效」样板
src/hooks/useUpdate.ts
src/components/UsagePanel.tsx    主页主体（4 早退分支 + card-grid + ProviderCard/QuotaRow/BrandLogo）
src/components/Settings.tsx      设置页（精简模式开关候选落点；现有 4 个即时生效开关）
src/components/Providers.tsx / Skills.tsx / UpdateNotice.tsx / ErrorBoundary.tsx
src/styles/app.css               唯一 CSS（1101 行）
public/providers/*.png           9 家品牌 logo
src-tauri/tauri.conf.json        窗口唯一事实源（L14-29）
src-tauri/capabilities/default.json
src-tauri/src/lib.rs             Builder 装配、生命周期
src-tauri/src/models.rs          AppConfig/UsageSummary/QuotaInfo（reset_at L140-141）
src-tauri/src/scheduler.rs       轮询 + 托盘菜单句柄托管
src-tauri/src/tray.rs            托盘标题/菜单/短码/倒计时/primary_quota/sync_tray_click_flag
src-tauri/src/background.rs      apply_window_effects（Rust 改窗口属性模板）
src-tauri/src/config.rs          配置加密落盘唯一通道
src-tauri/src/commands.rs        invoke 命令（13 个）
```

### 未验证风险（设计要么规避要么列入验证步骤）

1. hudWindow 材质在"宽矮横条"下的观感（含圆角/半径）。
2. Overlay 标题栏 + 运行时 set_decorations(false) / 红绿灯处理在 macOS 的行为。
3. "贴屏幕最上方"的屏幕几何（menu bar 高度、多屏选屏、scaleFactor）全仓库零先例。
4. resizable:false 窗口被运行时 set_size 后用户交互（能否拖动等）。
5. 冻结背景层在尺寸切换瞬间的比例/圆角错位。
