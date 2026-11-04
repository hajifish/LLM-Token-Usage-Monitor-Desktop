# 点击菜单栏图标行为设计

- 日期：2026-11-04
- 分支：`feature/provider-config-page`（用户指定继续用当前分支，不另拉分支）
- 状态：设计已确认（用户 2026-11-04 确认开关式方案；待实施）
- 背景分支文档：供应商配置独立页设计（同日，`2026-11-04-provider-config-page-design.md`）已实施并通过终审，本功能为其范围外遗留项

## 背景与目标

当前点击菜单栏（托盘）图标的行为固定为：触发后台刷新 + 弹出详情菜单（macOS 默认 `show_menu_on_left_click = true`）。本功能增加一个设置，允许用户选择点击图标时是「打开主页」还是「显示详情菜单」。

## 决策

| 项 | 决策 |
| --- | --- |
| 配置字段 | `AppConfig` 新增 `tray_click_opens_window: bool`，`#[serde(default)]`（缺省 `false` = 现状：点击显示详情菜单；旧配置零迁移） |
| 交互形态 | 设置页开关式（非分段选择器）：「点击菜单栏图标直接打开主页」；关 = 详情菜单（现状） |
| 右键行为 | 始终显示详情菜单（`show_menu_on_left_click(false)` 的标准语义），两种模式下都可用 |
| 后台刷新 | 保留现状：任意点击（左/右键）都触发一次后台刷新（现有 `on_tray_icon_event` 的 `notify_one` 不动） |
| 生效方式 | 设置页即时生效（现有关闭模式同款：读 fresh → 改字段 → `save_config` → 新命令 `apply_tray_click_behavior` 同步托盘运行时开关），不经「保存」按钮；设置页本就没有保存按钮 |
| 运行时切换 | tauri 2.11.6 `TrayIcon::set_show_menu_on_left_click(bool)`（tray/mod.rs:607 实证）；托盘对象全程持久、每轮仅原地更新菜单内容，切换后无需重建托盘 |
| 启动时初值 | `create_tray` 增加参数传入（`lib.rs` setup 中 `config::load` 上移到 `create_tray` 之前——当前 create_tray 在 manage(config) 之前运行，不能读 state） |
| 点击打开主页的实现 | `on_tray_icon_event` 内实时读配置 state（`try_state` + `try_read`）：左键 `Click` 且配置开启 → `window.show()` + `set_focus()`；实时读取使设置页切换即时生效，无需重建托盘 |
| 平台限制 | macOS / Windows 完整支持；Linux 部分 DE（GNOME AppIndicator 等）左键强制弹菜单、无法覆盖——如实记录，best-effort |
| 菜单「打开主页」项 | 保留不动（菜单模式下仍是有用入口） |

## 数据流

设置页切换开关：

```
读 get_config（fresh）→ fresh.tray_click_opens_window = value → save_config 落盘
→ invoke('apply_tray_click_behavior')：读配置 state → tray.set_show_menu_on_left_click(!value)
→ 后端写入成功后才同步本页状态（与 applyNotificationEnabled 同模式，失败 toast、checkbox 回弹）
```

托盘左键点击（开启模式）：

```
TrayIconEvent::Click { button: Left, button_state: Up } → notify_one（刷新，保留现状）
→ 读配置 state（实时）→ opens == true → show + focus 主窗口
```

## 组件设计

### Rust

- `models.rs`：`AppConfig` 末尾追加字段（`#[serde(default)]`）；`#[cfg(test)]` 增加默认值测试（`{"providers":[]}` 反序列化 → `false`；显式 `true` 可解析）。
- `lib.rs`：`config::load` 上移至 `create_tray` 之前；`create_tray(app.handle(), app_config.tray_click_opens_window)`；`apply_tray_click_behavior` 注册进 `generate_handler!` 与 use 导入。
- `tray.rs`：`create_tray(app: &AppHandle, click_opens_window: bool)`；builder 加 `.show_menu_on_left_click(!click_opens_window)`；`on_tray_icon_event` 闭包改为接收 `event`，在保留刷新触发之后增加左键打开分支。
- `commands.rs`：新增 `apply_tray_click_behavior` 命令（`app.tray_by_id("main_tray")` → `set_show_menu_on_left_click`；`use tauri::Manager;` 按需补充）。

### 前端

- `src/types.ts`：`AppConfig` 增加 `tray_click_opens_window: boolean`（snake_case 与 Rust serde 严格一致）。
- `src/components/Settings.tsx`：状态栏显示区块内、现有卡片之后新增独立 `tray-display-card`：开关行（`trayClickBusy` 独立 busy 态）+ hint；`applyTrayClickBehavior` 即时生效函数（见数据流）。

## 错误处理

- `apply_tray_click_behavior` 失败（托盘句柄缺失/设置失败）：命令返回 Err 字符串，前端走 toast 通道显示「点击行为设置保存失败」；本地 checkbox 回弹（受控组件既有机制）。
- 配置 state 在托盘事件闭包中不可用（理论上不发生）：`unwrap_or(false)` 回退现状行为（弹菜单）。

## 测试与验收

Rust 侧：`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test`（新增 serde 默认值测试在内全部通过）。
前端侧：`npm run build` + `npm run lint` + `npm run format:check`。
人工走查（tauri dev 自动重编译 Rust 并重启）：

1. 默认（开关关）：点击图标弹详情菜单（现状不变）。
2. 设置页开启开关 → 不重启，直接左键点击图标 → 主窗口显示并聚焦；右键仍弹详情菜单。
3. 关闭开关 → 左键恢复弹菜单。
4. 重启应用：开关状态保持，行为一致（启动初值路径）。
5. 「点击触发后台刷新」在两种模式下都不受影响（菜单里数据仍随点击而新）。
6. 旧配置文件（无该字段）启动无报错，反序列化默认 false。
