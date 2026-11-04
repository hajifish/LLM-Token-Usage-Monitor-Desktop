# 点击菜单栏图标行为 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增设置「点击菜单栏图标直接打开主页」：开启后左键点击托盘图标直接打开主窗口（右键仍弹详情菜单），关闭保持现状（点击弹详情菜单），设置页即时生效。

**Architecture:** `AppConfig` 新增 `tray_click_opens_window: bool`（serde default = false，零迁移）；`create_tray` 以参数接收初值，`on_tray_icon_event` 实时读配置 state 决定左键行为；新命令 `apply_tray_click_behavior` 用 `TrayIcon::set_show_menu_on_left_click` 做运行时切换；设置页走现有即时生效模式。

**Tech Stack:** Tauri 2.11.6（`show_menu_on_left_click` builder + `set_show_menu_on_left_click` 运行时 API，已在本地 crate 源码实证）、React 18 + TypeScript。

**Spec:** `docs/superpowers/specs/2026-11-04-tray-click-behavior-design.md`

## Global Constraints

- 分支 `feature/provider-config-page`（用户指定继续用当前分支），不 push。
- 提交信息中文 conventional commits，结尾 `Co-Authored-By: Claude Code <noreply@anthropic.com>`。
- Rust 验证门（CI 同款）：`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test`（在 `src-tauri/` 下执行），全部零错误。
- 前端验证门：`npm run build && npm run lint && npm run format:check`（无前端测试框架）。
- 数据结构向后兼容：新字段必须 `#[serde(default)]`，旧配置文件（无该字段）反序列化不得报错。
- 右键永远弹详情菜单；「任意点击触发后台刷新」的现状逻辑不得改动。

---

### Task 1: Rust 后端——配置字段、托盘启动初值、左键打开、运行时切换命令

**Files:**
- Modify: `src-tauri/src/models.rs`（AppConfig 字段 + 测试）
- Modify: `src-tauri/src/lib.rs`（setup 重排 + 命令注册）
- Modify: `src-tauri/src/tray.rs`（create_tray 签名 + builder + 事件分支）
- Modify: `src-tauri/src/commands.rs`（新命令）

**Interfaces:**
- Consumes: 现有 `config::load`、托盘 id `"main_tray"`、config state `Arc<RwLock<AppConfig>>`。
- Produces: `apply_tray_click_behavior(app: AppHandle, config_state: State<Arc<RwLock<AppConfig>>>) -> Result<(), String>`（Task 2 前端在 save_config 后 invoke）；`AppConfig.tray_click_opens_window: bool`（serde 反序列化口径，Task 2 的 types.ts 与之一致）。

- [ ] **Step 1: models.rs 加字段**

`AppConfig` 结构体（现 58-72 行区域）末尾追加（紧跟 `notifications_enabled` 之后）：

```rust
    /// 点击菜单栏图标直接打开主窗口（false = 点击显示详情菜单，默认现状；
    /// 旧配置文件缺此字段由 serde default 兜底为 false）
    #[serde(default)]
    pub tray_click_opens_window: bool,
```

- [ ] **Step 2: models.rs 加 serde 默认值测试**

在 models.rs 任一现有 `#[cfg(test)]` 模块中（176/191/208 行处任选其一）追加：

```rust
    #[test]
    fn tray_click_opens_window_defaults_false() {
        // 旧配置文件无此字段：反序列化兜底 false（= 点击弹详情菜单的现状）
        let cfg: AppConfig = serde_json::from_str(r#"{"providers":[]}"#).unwrap();
        assert!(!cfg.tray_click_opens_window);
        // 显式值可解析
        let cfg: AppConfig =
            serde_json::from_str(r#"{"providers":[],"tray_click_opens_window":true}"#).unwrap();
        assert!(cfg.tray_click_opens_window);
    }
```

若该测试模块未导入 `serde_json`，确认 models.rs 测试模块既有用法的同款路径（仓库其余测试已直接用 `serde_json::from_str`，无需新增依赖）。

- [ ] **Step 3: tray.rs——create_tray 签名 + builder + 左键打开分支**

3a. 签名（现 386 行）：

```rust
pub fn create_tray(app: &AppHandle, click_opens_window: bool) -> tauri::Result<()> {
```

3b. TrayIconBuilder（现 418-431 行）加 builder 标志并改造事件闭包：

```rust
    TrayIconBuilder::with_id("main_tray")
        .icon(tray_icon)
        .icon_as_template(false) // 全彩图标，不启用模板模式
        .show_menu_on_left_click(!click_opens_window) // 开启「点击打开主页」时左键不弹菜单（右键仍弹）
        .menu(&menu)
        .tooltip("Token Usage")
        .on_tray_icon_event(|tray, event| {
            // 每次点击托盘图标时触发后台刷新，数据在下次打开菜单时可见。
            // macOS 系统行为：点击菜单项后菜单必然关闭，因此刷新由托盘图标点击触发，
            // 而非菜单项点击，避免用户期望"菜单保持打开看更新"的落差。
            let app = tray.app_handle();
            if let Some(notify) = app.try_state::<std::sync::Arc<tokio::sync::Notify>>() {
                notify.notify_one();
            }
            // 「点击直接打开主页」模式：左键显示并聚焦主窗口；右键始终弹详情菜单
            // （show_menu_on_left_click(false) 的标准语义）。配置实时读取：
            // 设置页切换即时生效，无需重建托盘；state 不可用时回退现状（弹菜单）。
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                let opens = app
                    .try_state::<std::sync::Arc<tokio::sync::RwLock<crate::models::AppConfig>>>()
                    .and_then(|cfg| cfg.try_read().ok())
                    .map(|cfg| cfg.tray_click_opens_window)
                    .unwrap_or(false);
                if opens {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            }
        })
```

注意：`get_webview_window` 来自 `tauri::Manager` trait——确认 tray.rs 顶部已有 `use tauri::Manager;`（现文件已在 on_menu_event 中使用 `app.get_webview_window`，应已具备）；`RwLock`/`AppConfig` 用全限定路径（`tokio::sync::RwLock` / `crate::models::AppConfig`），不新增 use。

3c. 同文件 `#[cfg(test)] mod tray_title_tests`（454 行起）不需要改动；若有编译错误按 clippy 输出修正，不得改测试断言语义。

- [ ] **Step 4: commands.rs 新命令**

文件顶部确认导入（现有：`use crate::models::{AppConfig, UsageSummary}; use std::sync::Arc; use tauri::AppHandle; use tokio::sync::{Notify, RwLock};`），需补一行（若尚无）：

```rust
use tauri::Manager;
```

在 `send_test_notification` 命令之后追加：

```rust
/// 应用托盘点击行为运行时开关：读配置状态并同步到持久 TrayIcon（id "main_tray"）。
/// 设置页切换后调用，立即生效；托盘对象全程持久、每轮仅原地更新菜单内容，
/// 故无需重建托盘。create_tray 的启动初值由 lib.rs setup 传入，两处口径一致。
#[tauri::command]
pub async fn apply_tray_click_behavior(
    app: AppHandle,
    config_state: tauri::State<'_, Arc<RwLock<AppConfig>>>,
) -> Result<(), String> {
    let opens = config_state.read().await.tray_click_opens_window;
    if let Some(tray) = app.tray_by_id("main_tray") {
        tray.set_show_menu_on_left_click(!opens)
            .map_err(|e| format!("托盘点击行为设置失败: {e}"))?;
    }
    Ok(())
}
```

- [ ] **Step 5: lib.rs——setup 重排 + 注册命令**

5a. setup 块（现 52-54 行）把 `config::load` 上移并传参：

```rust
        .setup(|app| {
            let (app_config, config_health) = config::load(app.handle());
            tray::create_tray(app.handle(), app_config.tray_click_opens_window)?;
```

（原 53 行的 `tray::create_tray(app.handle())?;` 删除；`let (app_config, config_health) = config::load(app.handle());` 从 54 行上移，其余 setup 内容不动。）

5b. 命令导入（现 18 行 use 列表）加 `apply_tray_click_behavior`；`generate_handler!` 列表（现 84-94 行）在 `trigger_refresh,` 之后加一行 `apply_tray_click_behavior,`。

- [ ] **Step 6: Rust 验证门**

Run（在 `src-tauri/` 下）：`cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: 三者全部通过（新增测试在内全部 green）。fmt 先写后查可以接受，但提交前必须再跑一次 `cargo fmt --check` 确认干净。

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/models.rs src-tauri/src/lib.rs src-tauri/src/tray.rs src-tauri/src/commands.rs
git commit -m "$(cat <<'EOF'
feat: 托盘点击行为后端——左键直接打开主页（可配置）

AppConfig 新增 tray_click_opens_window（serde default false=现状弹详情菜单）；
create_tray 以参数接收启动初值（config::load 上移至 create_tray 之前）；
on_tray_icon_event 左键 Up 且开启时 show+focus 主窗口（配置实时读取，
切换即时生效；任意点击触发后台刷新的现状保留）；新命令
apply_tray_click_behavior 运行时同步 set_show_menu_on_left_click。
右键始终弹详情菜单。

Co-Authored-By: Claude Code <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: 前端——类型字段与设置页即时生效开关

**Files:**
- Modify: `src/types.ts`（AppConfig 字段）
- Modify: `src/components/Settings.tsx`（行为开关行 + applyTrayClickBehavior）

**Interfaces:**
- Consumes: Task 1 的 `apply_tray_click_behavior` 命令与 `tray_click_opens_window` 字段口径（boolean，snake_case）。
- Produces: 无下游消费。

- [ ] **Step 1: types.ts 加字段**

`AppConfig`（现 62-71 行）末尾追加：

```ts
  /** 点击菜单栏图标直接打开主页（false = 点击显示详情菜单，默认现状）。 */
  tray_click_opens_window: boolean;
```

- [ ] **Step 2: Settings.tsx 加即时生效函数**

在 `sendTestNotification` 函数之后追加（与 applyNotificationEnabled 同模式）：

```tsx
  /** 点击行为开关即时生效：读最新配置 → 仅改本字段 → 落盘 → 调命令同步托盘
   *  运行时开关。与 applyTrayDisplay 同模式：不经保存按钮、后端写入成功才同步
   *  本地状态；托盘侧 set_show_menu_on_left_click 立即生效，与每轮原地更新菜单
   *  互不干扰。 */
  const [trayClickBusy, setTrayClickBusy] = useState(false);
  const applyTrayClickBehavior = async (value: boolean) => {
    setTrayClickBusy(true);
    try {
      const fresh = await invoke<AppConfig>('get_config');
      fresh.tray_click_opens_window = value;
      await invoke('save_config', { newConfig: fresh });
      await invoke('apply_tray_click_behavior');
      setConfig((cur) => (cur ? { ...cur, tray_click_opens_window: value } : cur));
    } catch (e) {
      // 失败不静默：走 toast 通道，受控 checkbox 回弹真实值
      setMessage(`点击行为设置保存失败: ${e}`);
      setConfig((c) => (c ? { ...c } : c));
    } finally {
      setTrayClickBusy(false);
    }
  };
```

- [ ] **Step 3: Settings.tsx 加开关行**

状态栏显示区块（第一个 `settings-section`）内，现有 `tray-display-card` 之后、区块闭合之前新增独立卡片：

```tsx
          {/* 点击行为：左键直接打开主页（右键始终弹详情菜单） */}
          <div className="tray-display-card">
            <div className="update-row">
              <span className="update-row-label">点击菜单栏图标直接打开主页</span>
              <label className="toggle">
                <input
                  type="checkbox"
                  checked={config.tray_click_opens_window}
                  disabled={trayClickBusy}
                  onChange={(e) => applyTrayClickBehavior(e.target.checked)}
                />
              </label>
            </div>
            <span className="update-hint">
              开启后左键点击图标直接打开主窗口，右键仍可打开详情菜单；关闭时点击显示详情菜单（默认）。开关即时生效。
            </span>
          </div>
```

- [ ] **Step 4: 前端验证门**

Run: `npm run build && npm run lint && npm run format:check`
Expected: 全部零错误退出。

- [ ] **Step 5: Commit**

```bash
git add src/types.ts src/components/Settings.tsx
git commit -m "$(cat <<'EOF'
feat: 设置页「点击菜单栏图标直接打开主页」开关

状态栏显示区块新增即时生效开关：读 fresh → 落盘 → 调 apply_tray_click_behavior
同步托盘运行时开关；失败走 toast、checkbox 回弹。types.ts 同步字段口径。

Co-Authored-By: Claude Code <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: 人工走查（不产生提交；发现问题修复后重跑相关项）

**Files:** 无新增；修复只动 Task 1-2 涉及文件。

**Interfaces:**
- Consumes: Task 1-2 全部产出。tauri dev 检测到 Rust 变更会自动重编译并重启应用；前端走 HMR。
- Produces: 验收结论。

- [ ] **Step 1: 静态门复核**

Run: `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` 和 `npm run build && npm run lint && npm run format:check`
Expected: 全部零错误。

- [ ] **Step 2: 走查清单（spec「测试与验收」节）**

1. 默认（开关关）：左键点击图标弹详情菜单（现状不变）。
2. 设置页开启「点击菜单栏图标直接打开主页」→ 不重启，左键点击图标 → 主窗口显示并聚焦；右键仍弹详情菜单。
3. 关闭开关 → 左键恢复弹菜单。
4. 重启应用：开关状态保持、行为一致（启动初值路径）。
5. 两种模式下点击图标都触发后台刷新（菜单数据仍随点击更新）。
6. 旧配置（手工去掉该字段）启动无报错、行为为默认。
