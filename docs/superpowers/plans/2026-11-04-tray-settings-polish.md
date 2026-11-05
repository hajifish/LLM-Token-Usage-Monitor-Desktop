# 托盘与设置页走查打磨 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 走查反馈四项：①两张托盘设置卡片加间距；②指标文字总开关关闭时供应商列表禁用而非隐藏；③「打开主页」模式下窗口已可见时点击托盘改为弹详情菜单；④托盘菜单「退出」前加「设置」直达设置页。

**Architecture:** ①② 纯前端（CSS + Settings.tsx）；③ 用「标志随窗口可见性同步」机制实现——tauri 2.11.6 未透出 tray-icon 的 `show_menu()`（程序化弹菜单不可达），但点击标志是点击时实时读取的 Cell（tray-icon 0.24.2 已核），故窗口可见时把标志临时置 true、系统自动弹菜单，语义与需求一致；④ 新增 `open-settings` 菜单项（初始菜单 + 调度器动态菜单两处），复用前端已有的 `navigate` 事件监听。

**Tech Stack:** Tauri 2.11.6 / tray-icon 0.24.2 / React 18。

**Global Constraints:**
- 分支 `feature/provider-config-page`，不 push；提交中文 conventional commits + `Co-Authored-By: Claude Code <noreply@anthropic.com>`。
- Rust 门：`cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`（src-tauri/ 下）；前端门：`npm run build && npm run lint && npm run format:check`。
- 右键始终弹详情菜单的现状不动；任意点击触发后台刷新（`notify_one`）不动；「退出」仍是菜单最后项（Accessory 模式唯一出口）。

---

### Task 1: 四项打磨（单任务，改动集中且互相关联）

**Files:**
- Modify: `src/styles/app.css`（①间距 + ②禁用态样式）
- Modify: `src/components/Settings.tsx`（②列表常显禁用）
- Modify: `src-tauri/src/tray.rs`（③同步函数 + 左键分支改造 + ④初始菜单项 + 菜单事件分支）
- Modify: `src-tauri/src/scheduler.rs`（④动态菜单项 + 注释更新）
- Modify: `src-tauri/src/lib.rs`（③关窗时同步标志）

- [ ] **Step 1: app.css——间距与禁用态**

`.tray-display-card` 规则块（约 814-822 行）之后追加：

```css
/* 同区块内两张托盘卡片之间的留白 */
.settings-section .tray-display-card + .tray-display-card {
  margin-top: 12px;
}

/* 总开关关闭时供应商列表禁用仅供预览 */
.tray-provider-list.is-disabled {
  opacity: 0.45;
}

.tray-provider-list.is-disabled .tray-provider-row {
  cursor: default;
}
```

- [ ] **Step 2: Settings.tsx——列表常显、总关时禁用**

状态栏显示区块（现 189-212 行）：去掉 `config.tray_text_enabled &&` 条件渲染包裹，列表恒渲染；checkbox 追加禁用条件；label 加禁用态 class。改后整块：

```tsx
            <div
              className={`tray-provider-list${config.tray_text_enabled ? '' : ' is-disabled'}`}
            >
              {config.providers
                .map((p, i) => ({ p, i }))
                .filter(({ p }) => p.enabled)
                .map(({ p, i }) => (
                  <label key={`${p.name}-${p.alias || i}`} className="tray-provider-row">
                    <input
                      type="checkbox"
                      checked={p.show_in_tray}
                      disabled={trayBusy || !config.tray_text_enabled}
                      onChange={(e) =>
                        applyTrayDisplay({
                          providerIndex: i,
                          providerName: p.name,
                          value: e.target.checked,
                        })
                      }
                    />
                    <span>{p.alias?.trim() || providerLabel(p.name)}</span>
                  </label>
                ))}
            </div>
```

hint（现 213-215 行）改为：

```tsx
            <span className="update-hint">
              开关即时生效，无需保存。关闭总开关后菜单栏仅显示图标，下方供应商选项随之禁用仅供预览；也可对单个服务商关闭常显。
            </span>
```

- [ ] **Step 3: tray.rs——可见性同步函数 + 左键分支改造**

3a. `create_tray` 之前新增模块级函数（供托盘闭包、菜单事件与 lib.rs 关窗路径共用）：

```rust
/// 同步「左键点击行为」标志到当前窗口可见性（spec：程序化弹菜单在 tauri
/// 2.11.6 未透出，以标志实时读取等效实现）：
/// - 菜单模式（opens=false）→ 标志恒 true（点击弹菜单，builder 初值不变）；
/// - 打开主页模式（opens=true）→ 窗口可见时标志置 true（点击系统自动弹
///   详情菜单）、隐藏时置 false（点击走「打开主页」分支）。
/// 窗口显隐的每条路径（关窗隐藏 / 各处 show）之后都必须调用本函数。
pub fn sync_tray_click_flag(app: &AppHandle) {
    let opens = app
        .try_state::<std::sync::Arc<tokio::sync::RwLock<crate::models::AppConfig>>>()
        .and_then(|cfg| cfg.try_read().ok().map(|g| g.tray_click_opens_window))
        .unwrap_or(false);
    let visible = app
        .get_webview_window("main")
        .map(|w| w.is_visible().unwrap_or(false))
        .unwrap_or(false);
    if let Some(tray) = app.tray_by_id("main_tray") {
        tray.set_show_menu_on_left_click(!opens || visible);
    }
}
```

3b. `on_tray_icon_event` 闭包内左键分支：现在无条件 show+focus，改为「仅窗口不可见时打开」，并在收尾同步标志。现分支体：

```rust
                if opens {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
```

改为：

```rust
                if opens {
                    if let Some(window) = app.get_webview_window("main") {
                        // 窗口已可见时标志已被 sync 置 true：本次点击系统已自动弹
                        // 详情菜单，无需再处理；不可见时打开主窗口
                        if !window.is_visible().unwrap_or(false) {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    sync_tray_click_flag(app);
                }
```

（分支注释块里「窗口已可见时标志已被 sync 置 true」一句与上一行注释合并入代码位置即可，语义如上。）

3c. 菜单事件 `"open-main-window"` 分支（现 454-458 行区域）：show+focus 之后追加 `sync_tray_click_flag(app);`。随后新增 `"open-settings"` 分支：

```rust
            "open-settings" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                    // 前端 App.tsx 常驻监听 navigate（'settings' → 设置页视图）
                    let _ = app.emit("navigate", "settings");
                }
                sync_tray_click_flag(app);
            }
```

3d. `create_tray` 初始菜单（现 388-393 行）：在 quit 前插入设置项：

```rust
    let open_item = MenuItem::with_id(app, "open-main-window", "打开主页", true, None::<&str>)?;
    let separator1 = PredefinedMenuItem::separator(app)?;
    let update_item = create_update_item(app, "检查更新")?;
    let settings_item = MenuItem::with_id(app, "open-settings", "设置", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "⏻\u{FE0E} 退出", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[&open_item, &separator1, &update_item, &settings_item, &quit_item],
    )?;
```

3e. 导入核对：`AppHandle`/`Manager` 等 tray.rs 已具备（`emit` 来自 `Manager` trait——确认顶部 `use tauri::{…Manager…}`，若 `app.emit` 不可用则补）。

- [ ] **Step 4: scheduler.rs——动态菜单加设置项**

现 903-914 行区域（sep2 → quit）改为（注释同步更新——原注释「托盘不再重复」已过时）：

```rust
        // 6) 分隔线 → 设置 → 退出（设置直达设置页；退出仍是末项，Accessory 模式唯一出口）
        let Ok(separator2) = PredefinedMenuItem::separator(app_handle) else {
            return;
        };
        critical_ok &= menu.append(&separator2).is_ok();

        let Ok(settings_item) =
            MenuItem::with_id(app_handle, "open-settings", "设置", true, None::<&str>)
        else {
            return;
        };
        critical_ok &= menu.append(&settings_item).is_ok();

        let Ok(quit_item) =
            MenuItem::with_id(app_handle, "quit", "⏻\u{FE0E} 退出", true, None::<&str>)
        else {
            return;
        };
        critical_ok &= menu.append(&quit_item).is_ok();
```

- [ ] **Step 5: lib.rs——关窗隐藏时同步标志**

`on_window_event`（现 78-83 行）的 `WindowEvent::CloseRequested` 分支，hide 之后同步：

```rust
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
                // 窗口转入隐藏：打开主页模式下点击托盘应回到「打开主页」分支
                crate::tray::sync_tray_click_flag(window.app_handle());
            }
        })
```

- [ ] **Step 6: 验证门**

`cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`；仓库根 `npm run build && npm run lint && npm run format:check`——全部通过。

- [ ] **Step 7: Commit**

```bash
git add src/styles/app.css src/components/Settings.tsx src-tauri/src/tray.rs src-tauri/src/scheduler.rs src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat: 托盘与设置页走查打磨——卡片间距、列表禁用预览、点击行为按窗口可见性、托盘设置入口

- 设置页两张托盘卡片之间加 12px 间距
- 指标文字总开关关闭时供应商列表禁用仅供预览（不再整块隐藏）
- 「打开主页」模式下窗口已可见时点击托盘弹详情菜单：tauri 未透出
  show_menu()，以「标志随窗口可见性同步」等效实现（sync_tray_click_flag，
  关窗/各 show 路径调用）；窗口不可见时点击仍直接打开
- 托盘菜单「退出」前新增「设置」：show+focus + navigate 事件直达设置页
  （初始菜单与调度器动态菜单两处；更新过时注释「托盘不再重复」）

Co-Authored-By: Claude Code <noreply@anthropic.com>
EOF
)"
```

---

### 走查清单（完成后人工）

1. 设置页两张卡片有可见间距。
2. 总开关关闭：列表仍显示、灰化、不可点；开启恢复可操作。
3. 「打开主页」模式：窗口隐藏时点击→打开；窗口可见时点击→弹详情菜单；再点菜单外区域菜单收起；右键始终弹菜单。
4. 托盘菜单末尾「设置」：点击→主窗口显示并切到设置页；菜单模式下同样生效。
5. 菜单模式下行为与之前完全一致（回归）。
