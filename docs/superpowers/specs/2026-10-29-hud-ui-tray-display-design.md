# HUD 窗口 + 状态栏常显与逐项控制 — 设计文档

日期：2026-10-29
分支：`feature/hud-ui-tray-display`（基于 `ff391cc`）
参考项目：`/Users/shin/dev/Cellular-Hotspot-Network-Monitor`（Traffic Monitor，原生 Swift/AppKit 菜单栏应用）

## 背景与目标

参考 Traffic Monitor 的两个核心设计，改进本项目：

1. **窗口 HUD 化**：固定尺寸、半透明毛玻璃（macOS HUD 风格）、恒深色的仪表盘窗口
2. **状态栏常显 + 逐项控制**：菜单栏实时显示各服务商主指标，每个服务商可独立开关是否上状态栏

顺带纳入：修复自启弹窗（`--minimized`）、刷新体验优化、清理死样式。

## 范围

**做**：A 窗口 HUD 化 / B 状态栏常显与控制 / C 刷新体验 + 清死样式 / D 修复自启弹窗。

**不做（明确排除）**：
- 顶部总余额汇总条（用户明确不需要，`total_balance` 仍仅用于托盘 tooltip）
- 托盘菜单内的显示开关（事件与菜单重建复杂度高，留作后续）
- 亮色主题（HUD 风格恒深色，与参考项目一致）
- 系统通知/告警声音、多语言、窗口位置记忆

## A. 窗口 HUD 化

### 窗口配置（tauri.conf.json，无 Rust 代码）

```json
{
  "width": 780,
  "height": 540,
  "resizable": false,
  "transparent": true,
  "titleBarStyle": "Overlay",
  "hiddenTitle": true,
  "theme": "dark",
  "visible": false,
  "windowEffects": { "effects": ["hudWindow"], "state": "active" }
}
```

- `hudWindow` 即参考项目 `NSVisualEffectView(material: .hudWindow)` 的 Tauri 等价物；若该效果在当前 Tauri 版本不可用，回退 `under-window-background`（毛玻璃感稍弱，仍透明）
- `titleBarStyle: Overlay` + `hiddenTitle` = 参考项目的 `.fullSizeContentView` + 透明标题栏：内容顶到窗口边缘，红绿灯浮动
- `theme: "dark"` 强制恒深色（参考项目同样强制 darkAqua，仪表盘不随系统亮暗切换）
- `visible: false` 配合 D（启动行为）
- `windowEffects` 若支持 `radius` 字段则设 `12`（圆角窗口）

### CSS 层（app.css）

- 页面根背景由 `#1e1e1e` 改为**全透明**，让 HUD 材质透出
- 卡片浮层公式（照搬参考项目）：`background: rgba(255,255,255,.06); border: 1px solid rgba(255,255,255,.10); border-radius: 12px`
- 文字保持浅色系；`tabular-nums` 已有，保留
- **交通灯避让**：`titleBarStyle: Overlay` 后内容延伸到标题栏区域，`.app-header` 左侧预留约 76px padding 避开红绿灯；`.app-header` 加 `data-tauri-drag-region` 使顶栏可拖动窗口（对等参考项目 `isMovableByWindowBackground`）
- 设置视图在固定高度内 `overflow-y: auto` 滚动

### 球开关（主窗右上角）

- `.app-header` 右侧新增一枚球开关 + 文字「状态栏」，绑定全局 `tray_text_enabled`
- 视觉照搬参考项目：18px 圆形，选中 = 白色实心，未选 = 1.5px `rgba(255,255,255,.45)` 空心环；文字用灰阶区分态

## B. 状态栏常显 + 逐项控制

### 配置结构（models.rs / types.ts）

- `AppConfig` 增加 `tray_text_enabled: bool`（`#[serde(default)]`，默认 `true`）
- `ProviderConfig` 增加 `show_in_tray: bool`（`#[serde(default)]`，默认 `true`）
- 旧配置文件缺少这两个字段时由 serde default 兜底，无损加载

### 标题构建（tray.rs 新增纯函数 `build_tray_title`）

```text
输入：全局开关、providers（已启用）的 (短名, show_in_tray, 主指标值/错误态)
输出：Option<String> —— None 或空串时 set_title("") 回退纯图标
```

- 片段格式：`短名 + 主指标`，片段间 ` · ` 分隔，例：`智谱 70% · DS ¥88.88 · GPT $12.34`
- 主指标沿用现有约定：% 型 → `70%`（整数）；CNY → `¥88.88`（2 位小数）；USD → `$12.34`（2 位小数）
- 短名：alias 优先，否则原名；超过 8 字符截断为 7 + `…`（用户可用设置页已有 alias 字段自行缩短）
- 出错/无数据片段：显示 `短名 –`
- 过滤规则：仅统计 `enabled == true` 且 `show_in_tray == true` 的服务商
- 回退规则：全局关 / 过滤后列表为空 → 不显示文字，仅保留图标（对等参考项目「都关时只显示图标」）
- 告警打点（图标右上角圆点）、tooltip、点击图标刷新均**保持不变**

### 更新时机

- 复用现有流程：`scheduler::update_tray_menu` 内（每次轮询后）调用 `build_tray_title` → `TrayIcon::set_title`；`save_config` 已有 `Notify` 唤醒调度器立即轮询 → 配置改动即时生效
- `set_title` 失败仅记日志，不影响主流程

### 设置入口（Settings.tsx）

- 全局行下方新增「状态栏显示」区块：
  - 总开关（与右上角球开关联动，同一配置字段）
  - 每个 `enabled` 服务商一行 checkbox（默认勾选），文案 = alias 或原名 + 主指标预览
- 已禁用的服务商不出现在该区块

## C. 刷新体验 + 清死样式

- 刷新时**保留旧数据**继续渲染：手动刷新期间面板容器加 `.is-refreshing`（整体透明度降至 ~0.6），顶栏刷新按钮加旋转动画；事件到达后移除。仅**首次无任何数据**时才显示整面板加载态
- 删除死样式（实现时以 grep 确认无引用后再删）：`.total-card` / `.total-value` 系列、`.panel-footer` / `.footer-btn` / `.footer-time`、未使用的 `.accent-purple` / `.accent-pink`
- Settings 健康横幅的 inline style 本次不动（刻意为之，见 Settings.tsx 内注释）

## D. 修复自启弹窗（--minimized）

- 主窗口 `visible: false`（见 A）
- `lib.rs` 在应用就绪时（`RunEvent::Ready` 或 setup 尾部）检测启动参数：`std::env::args().any(|a| a == "--minimized")`
  - **带**参数（开机自启，autostart 插件以 `--minimized` 启动，lib.rs:34-37）→ 不 `show()`，静默驻留托盘
  - **不带**参数（手动启动）→ `window.show()`，正常弹窗
- 在 Ready 时而非 setup 时 show，避免透明窗口未渲染完成时闪现矩形
- `CloseRequested → hide()` 行为不变

## 数据流（改动后）

```text
配置保存（球开关/设置页 checkbox）
  → invoke save_config → Notify 调度器
  → poll_providers → UsageSummary 更新
  → update_tray_menu（重建菜单 + tooltip + build_tray_title → set_title + 打点）
  → emit usage-updated → 前端刷新（保留旧数据，仅换内容）
```

无新增事件、无新增命令；`save_config`/`get_config` 自动携带新字段。

## 测试与验收

**Rust 单测**：
- `build_tray_title`：% / CNY / USD 混合格式、` · ` 分隔、错误段 `名 –`、8 字符截断、禁用或 `show_in_tray=false` 过滤、全局关与空列表回退 `None`
- 配置兼容：旧 JSON（无新字段）反序列化得到默认值；新字段 roundtrip 保存/加载
- `--minimized` 参数检测函数

**手动验收清单**：
1. HUD 视觉：毛玻璃透出桌面、卡片浮层质感、窗口 780×540 不可拉伸、顶栏可拖动、红绿灯不遮挡内容
2. 状态栏：菜单栏图标旁出现 `智谱 70% · …` 文字；轮询后数值更新；关总开关立即回退纯图标；逐个关闭 provider checkbox 对应片段消失
3. 设置页与右上角球开关联动一致
4. 开机自启场景（`--minimized`）不弹窗；手动启动正常显示
5. 刷新期间旧数据保留、按钮旋转；无 JS console 报错

## 风险与备注

- `windowEffects` 具体枚举序列化以 Tauri 2 配置 schema 为准（`hudWindow`），不可用时回退 `under-window-background`
- 透明 + Overlay 标题栏在 macOS 无需私有 API（`macOSPrivateApi` 仅 Windows 透明需要），预计零 Rust 侧窗口代码
- 若 hudWindow 材质在某些桌面壁纸下对比不足，备选：CSS 叠加 `rgba(0,0,0,.35)` 底色增强对比（实现时目测定）
