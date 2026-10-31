# HUD 窗口 + 状态栏常显与逐项控制 — 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把主窗口改造为 780×540 固定的 macOS HUD 毛玻璃风格，并在菜单栏托盘图标旁常显各服务商主指标文字（每个服务商可独立开关）。

**Architecture:** 纯配置 + CSS 完成窗口 HUD 化（`hudWindow` 窗口效果，无需 Rust 窗口代码）；状态栏文字通过 `build_tray_title` 纯函数在既有 `update_tray_menu` 流程中用 `TrayIcon::set_title` 更新；开关状态存入 `AppConfig`/`ProviderConfig`（serde default 保证旧配置兼容），复用既有 `save_config` → `Notify` → 轮询链路即时生效。

**Tech Stack:** Tauri 2（tauri 2.x，tray-icon + image-png features）、React 18 + TypeScript、纯手写 CSS、Rust 内联 `#[cfg(test)]` 单测。

**Spec:** `docs/superpowers/specs/2026-10-29-hud-ui-tray-display-design.md`

## Global Constraints

- 仓库：`/Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud`，分支 `feature/hud-ui-tray-display`
- Tauri 2 API 面：`TrayIcon::set_title(Option<&str>)`、窗口配置 `titleBarStyle`/`hiddenTitle`/`theme`/`transparent`/`visible`/`windowEffects` 均为 tauri 2 稳定配置；`windowEffects.radius` 为 Windows 专属，**不使用**
- TS 类型与 Rust serde 字段严格 snake_case 一致（types.ts 是镜像，注释风格照旧）
- 配置新增字段一律 `#[serde(default)]`/`#[serde(default = "...")]`，旧配置文件必须无损加载
- 前端 UI 文案与代码注释用中文，与现有风格一致
- 测试命令：Rust — `cd src-tauri && cargo test`；TS 类型检查 + 构建 — `npm run build`；lint — `npm run lint`
- 每个任务结束时 commit（conventional 提交信息，末尾加 `Co-Authored-By: Claude Code <noreply@anthropic.com>`）
- 主指标约定：% 型显剩余百分比（整数），CNY 显可用余额（2 位小数），USD 显本月花费（2 位小数）

---

### Task 1: 配置模型扩展（tray_text_enabled / show_in_tray）+ 兼容测试

**Files:**
- Modify: `src-tauri/src/models.rs`（ProviderConfig 约 :39-48、AppConfig 约 :50-75）
- Modify: `src-tauri/src/config.rs`（测试 `sample_config` 约 :536-558，新增测试）
- Modify: `src/types.ts`（ProviderConfig :52-58、AppConfig :60-67）
- Modify: `src/components/Settings.tsx`（`addProvider` :162-173）

**Interfaces:**
- Consumes: 无（首个任务）
- Produces: `AppConfig.tray_text_enabled: bool`（默认 true）、`ProviderConfig.show_in_tray: bool`（默认 true）；后续 Task 2/3/6/7 依赖这两个字段名

- [ ] **Step 1: 写失败的兼容测试**

在 `src-tauri/src/config.rs` 的 `mod tests` 中，`sample_config()` 之后追加：

```rust
    /// 旧版配置（无托盘显示字段）反序列化时由 serde default 兜底，无损加载。
    #[test]
    fn legacy_config_without_tray_fields_gets_defaults() {
        let legacy = r#"{"providers":[{"name":"DeepSeek","api_key":"sk-x","enabled":true}],"refresh_interval":60}"#;
        let cfg: AppConfig = serde_json::from_str(legacy).unwrap();
        assert!(cfg.tray_text_enabled, "缺省的 tray_text_enabled 应默认为 true");
        assert!(cfg.providers[0].show_in_tray, "缺省的 show_in_tray 应默认为 true");
    }

    /// 新增托盘字段随加密信封 roundtrip 不丢失。
    #[test]
    fn tray_fields_survive_save_load_roundtrip() {
        let path = unique_temp_path("tray-roundtrip");
        let mut cfg = sample_config();
        cfg.tray_text_enabled = false;
        cfg.providers[0].show_in_tray = false;
        save_to_path(&path, &cfg, false).unwrap();

        let (loaded, health) = load_from_path(&path);
        assert!(!health.unreadable);
        assert!(!loaded.tray_text_enabled);
        assert!(!loaded.providers[0].show_in_tray);
        assert!(loaded.providers[1].show_in_tray);

        let _ = fs::remove_file(&path);
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud/src-tauri && cargo test legacy_config_without_tray_fields tray_fields_survive_save_load_roundtrip`
Expected: **编译失败**（`tray_text_enabled` / `show_in_tray` 字段不存在）

- [ ] **Step 3: 扩展 models.rs**

`src-tauri/src/models.rs` — `ProviderConfig` 增加字段（在 `alias` 字段后）：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub api_key: String,
    pub enabled: bool,
    #[serde(default)]
    pub platform_token: Option<String>,
    /// 用户自定义别名（用于区分同一服务商的多个账号）；为空时回退到 name
    #[serde(default)]
    pub alias: Option<String>,
    /// 状态栏常显开关：是否在菜单栏托盘文字中展示该服务商的主指标
    #[serde(default = "default_show_in_tray")]
    pub show_in_tray: bool,
}

fn default_show_in_tray() -> bool {
    true
}
```

`AppConfig` 增加字段（在 `skipped_update_version` 后）：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub providers: Vec<ProviderConfig>,
    pub refresh_interval: u64,
    /// 是否自动检查更新（启动 60 秒后首查，之后每 24 小时一次）
    #[serde(default = "default_auto_check_update")]
    pub auto_check_update: bool,
    /// 用户「忽略此版本」记录的新版本号；仅对完全相同的版本号生效
    #[serde(default)]
    pub skipped_update_version: Option<String>,
    /// 托盘文字总开关：关闭后菜单栏仅显示图标
    #[serde(default = "default_tray_text_enabled")]
    pub tray_text_enabled: bool,
}

fn default_tray_text_enabled() -> bool {
    true
}
```

同步更新 `Default for AppConfig`（models.rs :66-75）：

```rust
impl Default for AppConfig {
    fn default() -> Self {
        Self {
            providers: Vec::new(),
            refresh_interval: 60,
            auto_check_update: default_auto_check_update(),
            skipped_update_version: None,
            tray_text_enabled: default_tray_text_enabled(),
        }
    }
}
```

- [ ] **Step 4: 修复 config.rs 测试的 sample_config**

两处 `ProviderConfig {`（config.rs :539、:546）各追加 `show_in_tray` 字段：

```rust
                ProviderConfig {
                    name: "deepseek".to_string(),
                    api_key: "sk-DEEP-SECRET-abc123".to_string(),
                    enabled: true,
                    platform_token: Some("plat-token-xyz".to_string()),
                    alias: Some("工作账号".to_string()),
                    show_in_tray: true,
                },
                ProviderConfig {
                    name: "kimi".to_string(),
                    api_key: "sk-KIMI-SECRET-def456".to_string(),
                    enabled: false,
                    platform_token: None,
                    alias: None,
                    show_in_tray: true,
                },
```

Run: `cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud/src-tauri && cargo test`
Expected: 全部 PASS（含两个新测试）

- [ ] **Step 5: 同步 types.ts 与 Settings.tsx**

`src/types.ts` — `ProviderConfig` 与 `AppConfig`：

```typescript
export interface ProviderConfig {
  name: string;
  api_key: string;
  enabled: boolean;
  platform_token?: string | null;
  alias?: string | null;
  /** 状态栏常显开关：是否在菜单栏托盘文字中展示该服务商。 */
  show_in_tray: boolean;
}

export interface AppConfig {
  providers: ProviderConfig[];
  refresh_interval: number;
  /** 是否自动检查更新（启动后 60 秒首查，之后每 24 小时一次）。 */
  auto_check_update: boolean;
  /** 用户「忽略此版本」记录的新版本号；出现更新的版本时自动失效。 */
  skipped_update_version: string | null;
  /** 托盘文字总开关：关闭后菜单栏仅显示图标。 */
  tray_text_enabled: boolean;
}
```

`src/components/Settings.tsx` `addProvider`（:162-173）中的新对象补上默认值：

```typescript
        { name: def.value, api_key: '', enabled: true, platform_token: null, alias: null, show_in_tray: true },
```

- [ ] **Step 6: 类型检查 + 提交**

Run: `cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud && npm run build && npm run lint`
Expected: 构建成功、无 lint 错误

```bash
cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud
git add src-tauri/src/models.rs src-tauri/src/config.rs src/types.ts src/components/Settings.tsx
git commit -m "feat: 配置模型增加状态栏常显开关（tray_text_enabled / show_in_tray）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: `build_tray_title` 纯函数（TDD）

**Files:**
- Modify: `src-tauri/src/tray.rs`（导入区 :1-7、`worst_warning` 之后新增函数、文件尾新增测试模块）

**Interfaces:**
- Consumes: Task 1 的 `AppConfig.tray_text_enabled`、`ProviderConfig.show_in_tray`
- Produces: `pub fn build_tray_title(config: &AppConfig, summary: &UsageSummary) -> Option<String>`（Task 3 调用）；片段格式 `短名 主指标`，分隔符 `" · "`，无数据/出错段 `短名 –`，None = 回退纯图标

- [ ] **Step 1: 写失败的测试**

`src-tauri/src/tray.rs` 文件末尾追加：

```rust
#[cfg(test)]
mod tray_title_tests {
    use super::*;
    use crate::models::{AppConfig, BalanceData, ProviderConfig};

    fn balance(v: f64, currency: &str) -> BalanceData {
        BalanceData {
            provider: "test".into(),
            available_balance: v,
            voucher_balance: 0.0,
            cash_balance: 0.0,
            total_balance: v,
            currency: currency.into(),
        }
    }

    fn status(alias: &str, balance: Option<BalanceData>, error: Option<String>) -> ProviderStatus {
        ProviderStatus {
            name: alias.to_string(),
            alias: alias.to_string(),
            enabled: true,
            balance,
            usage: None,
            error,
            quota_infos: None,
        }
    }

    fn pconfig(name: &str, alias: Option<&str>, show_in_tray: bool) -> ProviderConfig {
        ProviderConfig {
            name: name.to_string(),
            api_key: String::new(),
            enabled: true,
            platform_token: None,
            alias: alias.map(Into::into),
            show_in_tray,
        }
    }

    fn config(providers: Vec<ProviderConfig>) -> AppConfig {
        AppConfig {
            providers,
            refresh_interval: 60,
            auto_check_update: true,
            skipped_update_version: None,
            tray_text_enabled: true,
        }
    }

    #[test]
    fn title_none_when_global_disabled() {
        let mut cfg = config(vec![]);
        cfg.tray_text_enabled = false;
        let s = UsageSummary {
            providers: vec![status("智谱", Some(balance(70.0, "%"), None))],
            ..Default::default()
        };
        assert_eq!(build_tray_title(&cfg, &s), None);
    }

    #[test]
    fn title_none_when_no_visible_providers() {
        assert_eq!(build_tray_title(&config(vec![]), &UsageSummary::default()), None);
    }

    #[test]
    fn title_joins_mixed_segments_with_separator() {
        let s = UsageSummary {
            providers: vec![
                status("智谱", Some(balance(70.0, "%"), None)),
                status("DeepSeek", Some(balance(88.88, "CNY"), None)),
                status("OpenAI", Some(balance(12.34, "USD"), None)),
            ],
            ..Default::default()
        };
        assert_eq!(
            build_tray_title(&config(vec![]), &s).unwrap(),
            "智谱 70% · DeepSeek ¥88.88 · OpenAI $12.34"
        );
    }

    #[test]
    fn percent_value_formats_as_integer() {
        let s = UsageSummary {
            providers: vec![status("智谱", Some(balance(70.4, "%"), None))],
            ..Default::default()
        };
        assert_eq!(build_tray_title(&config(vec![]), &s).unwrap(), "智谱 70%");
    }

    #[test]
    fn error_segment_shows_dash() {
        let s = UsageSummary {
            providers: vec![status("Kimi", None, Some("timeout".into()))],
            ..Default::default()
        };
        assert_eq!(build_tray_title(&config(vec![]), &s).unwrap(), "Kimi –");
    }

    #[test]
    fn no_data_segment_shows_dash() {
        let s = UsageSummary {
            providers: vec![status("Unknown", None, None)],
            ..Default::default()
        };
        assert_eq!(build_tray_title(&config(vec![]), &s).unwrap(), "Unknown –");
    }

    #[test]
    fn disabled_provider_excluded() {
        let mut p = status("智谱", Some(balance(70.0, "%"), None));
        p.enabled = false;
        let s = UsageSummary {
            providers: vec![p],
            ..Default::default()
        };
        assert_eq!(build_tray_title(&config(vec![]), &s), None);
    }

    #[test]
    fn show_in_tray_false_excluded() {
        let cfg = config(vec![pconfig("智谱", None, false)]);
        let s = UsageSummary {
            providers: vec![status("智谱", Some(balance(70.0, "%"), None))],
            ..Default::default()
        };
        assert_eq!(build_tray_title(&cfg, &s), None);
    }

    #[test]
    fn same_name_accounts_matched_by_alias() {
        // 同类型两个账号，仅关闭其中一个
        let cfg = config(vec![
            pconfig("DeepSeek", Some("工作号"), true),
            pconfig("DeepSeek", Some("私人号"), false),
        ]);
        let s = UsageSummary {
            providers: vec![
                status("工作号", Some(balance(88.0, "CNY"), None)),
                status("私人号", Some(balance(5.0, "CNY"), None)),
            ],
            ..Default::default()
        };
        assert_eq!(build_tray_title(&cfg, &s).unwrap(), "工作号 ¥88.00");
    }

    #[test]
    fn long_alias_truncated_to_seven_chars_plus_ellipsis() {
        // "Claude Code"（11 字符）→ 截 7 字符去尾空格 + …
        let s = UsageSummary {
            providers: vec![status("Claude Code", Some(balance(70.0, "%"), None))],
            ..Default::default()
        };
        assert_eq!(build_tray_title(&config(vec![]), &s).unwrap(), "Claude… 70%");
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud/src-tauri && cargo test tray_title_tests`
Expected: **编译失败**（`build_tray_title` 未定义）

- [ ] **Step 3: 实现纯函数**

`src-tauri/src/tray.rs` 导入区改为：

```rust
use crate::models::{AppConfig, ProviderStatus, UsageSummary};
```

在 `worst_warning` 函数之后、`update_tray_badge` 之前插入：

```rust
/// 短名：alias 已由 scheduler 回退为原名；超过 8 字符截为 7 字符（去尾部空白）+ …
fn tray_short_name(alias: &str) -> String {
    let count = alias.chars().count();
    if count <= 8 {
        return alias.to_string();
    }
    let truncated: String = alias.chars().take(7).collect();
    format!("{}…", truncated.trim_end())
}

/// 状态栏片段：短名 + 主指标值；无数据/出错显示「短名 –」
fn tray_segment(p: &ProviderStatus) -> String {
    let name = tray_short_name(&p.alias);
    match &p.balance {
        Some(b) if b.is_percent() => format!("{} {:.0}%", name, b.available_balance),
        Some(b) if b.is_usd() => format!("{} ${:.2}", name, b.available_balance),
        Some(b) => format!("{} ¥{:.2}", name, b.available_balance),
        None => format!("{} –", name),
    }
}

/// 该服务商是否需要在状态栏展示：按 name + 回退后的 alias 匹配配置项，
/// 匹配不到时默认展示（与 serde default 的「新字段默认开」语义一致）。
fn provider_show_in_tray(config: &AppConfig, p: &ProviderStatus) -> bool {
    config
        .providers
        .iter()
        .find(|c| {
            c.name == p.name
                && c.alias
                    .as_deref()
                    .map(str::trim)
                    .filter(|a| !a.is_empty())
                    .unwrap_or(&c.name)
                    == p.alias.as_str()
        })
        .map(|c| c.show_in_tray)
        .unwrap_or(true)
}

/// 构建状态栏常显文字；全局关闭或无可展示服务商时返回 None（托盘回退纯图标）。
pub fn build_tray_title(config: &AppConfig, summary: &UsageSummary) -> Option<String> {
    if !config.tray_text_enabled {
        return None;
    }
    let parts: Vec<String> = summary
        .providers
        .iter()
        .filter(|p| p.enabled && provider_show_in_tray(config, p))
        .map(tray_segment)
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud/src-tauri && cargo test tray_title_tests`
Expected: 11 个测试全部 PASS

- [ ] **Step 5: 全量测试 + 提交**

Run: `cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud/src-tauri && cargo test`
Expected: 全部 PASS（含既有 config/scheduler 测试）

```bash
cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud
git add src-tauri/src/tray.rs
git commit -m "feat: build_tray_title 纯函数（状态栏常显文字构建）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: 调度器集成 `set_title`

**Files:**
- Modify: `src-tauri/src/scheduler.rs`（导入区 :1、`poll_providers` 调用点 :216、`update_tray_menu` :225-226、`refresh_tray_menu` :382-390）

**Interfaces:**
- Consumes: Task 2 的 `build_tray_title(&AppConfig, &UsageSummary) -> Option<String>`；lib.rs 已托管的 `Arc<RwLock<AppConfig>>` 状态（lib.rs :47）
- Produces: 托盘标题随每次轮询/配置保存即时更新（无新接口）

- [ ] **Step 1: `update_tray_menu` 改为 async 并集成 set_title**

`src-tauri/src/scheduler.rs` 导入区（:1）改为：

```rust
use crate::models::{AppConfig, BalanceData, ProviderStatus, UsageSummary};
```

`update_tray_menu` 签名与开头（:225-227）改为：

```rust
async fn update_tray_menu(app_handle: &AppHandle, summary: &UsageSummary) {
    if let Some(tray) = app_handle.tray_by_id("main_tray") {
        // --- 状态栏常显文字（菜单栏图标旁的主指标摘要） ---
        let tray_config = match app_handle.try_state::<Arc<RwLock<AppConfig>>>() {
            Some(state) => state.read().await.clone(),
            None => AppConfig::default(),
        };
        let title = crate::tray::build_tray_title(&tray_config, summary);
        if let Err(e) = tray.set_title(title.as_deref()) {
            log::warn!("Failed to update tray title: {}", e);
        }

        // --- 构建 tooltip（鼠标悬停提示） ---
```

（其后 tooltip 与菜单构建逻辑保持不变）

- [ ] **Step 2: 更新两处调用点**

`poll_providers` 内（:216）：

```rust
    // 更新托盘菜单和 tooltip
    update_tray_menu(app_handle, &summary).await;
```

`refresh_tray_menu` 的 spawned 任务内（:387）：

```rust
            update_tray_menu(&app, &snapshot).await;
```

- [ ] **Step 3: 编译 + 测试**

Run: `cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud/src-tauri && cargo test`
Expected: 编译通过、全部 PASS

- [ ] **Step 4: 手动冒烟验证**

Run: `cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud && npm run tauri dev`
Expected: 托盘图标旁出现文字（如 `智谱 70% · DeepSeek ¥xx.xx`，取决于已配置的服务商）；无服务商时仅图标

- [ ] **Step 5: 提交**

```bash
cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud
git add src-tauri/src/scheduler.rs
git commit -m "feat: 菜单栏常显各服务商主指标（TrayIcon::set_title）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: 启动行为修复（--minimized + 初始隐藏）

**Files:**
- Modify: `src-tauri/src/lib.rs`（`run()` :20-86，文件尾新增测试）
- Modify: `src-tauri/tauri.conf.json`（windows :13-19）

**Interfaces:**
- Consumes: 无
- Produces: `fn should_show_window(args: &[String]) -> bool`；主窗口默认 `visible: false`、就绪后按参数决定是否显示。**注意：本任务落地后、Task 5 之前，应用启动时窗口默认隐藏，需用托盘「打开主页」唤出**

- [ ] **Step 1: 写失败的测试**

`src-tauri/src/lib.rs` 文件末尾追加：

```rust
/// 判断本次启动是否应显示主窗口：开机自启以 --minimized 启动时保持静默驻留托盘。
fn should_show_window(args: &[String]) -> bool {
    !args.iter().any(|a| a == "--minimized")
}

#[cfg(test)]
mod launch_args_tests {
    use super::should_show_window;

    #[test]
    fn minimized_flag_hides_window() {
        let args = vec!["app".to_string(), "--minimized".to_string()];
        assert!(!should_show_window(&args));
    }

    #[test]
    fn normal_launch_shows_window() {
        let args = vec!["app".to_string()];
        assert!(should_show_window(&args));
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud/src-tauri && cargo test should_show_window minimized_flag normal_launch`
Expected: **编译失败**（`should_show_window` 未定义）——若测试已存在则跳过到 Step 3

- [ ] **Step 3: 改造 run() 尾部 + 窗口默认隐藏**

`src-tauri/src/lib.rs` — `run()` 尾部（:84-85）：

```rust
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
```

改为：

```rust
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // 就绪后再显示窗口，避免窗体未完成渲染时闪现矩形；
            // 开机自启（--minimized）保持静默，仅驻留托盘。
            if let tauri::RunEvent::Ready = event {
                let args: Vec<String> = std::env::args().collect();
                if should_show_window(&args) {
                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            }
        });
```

`src-tauri/tauri.conf.json` — windows 数组的窗口对象追加 `"visible": false`：

```json
      {
        "title": "Local LLM Token Usage Monitor",
        "width": 800,
        "height": 600,
        "visible": false
      }
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud/src-tauri && cargo test`
Expected: 全部 PASS

- [ ] **Step 5: 手动冒烟验证**

Run: `cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud && npm run tauri dev`
Expected: 普通启动窗口正常弹出；从托盘菜单「退出」后，模拟自启参数启动（`npm run tauri dev -- -- --minimized`，参数经 tauri CLI 转发给二进制）窗口不弹、托盘在

- [ ] **Step 6: 提交**

```bash
cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud
git add src-tauri/src/lib.rs src-tauri/tauri.conf.json
git commit -m "fix: 开机自启（--minimized）不再弹窗，主窗口就绪后再显示

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 5: 窗口 HUD 化（配置 + 透明主题 CSS）

**Files:**
- Modify: `src-tauri/tauri.conf.json`（windows :13-20）
- Modify: `src/styles/app.css`（多处，见各步骤）

**Interfaces:**
- Consumes: Task 4 的 `visible: false` + Ready 显示逻辑
- Produces: HUD 视觉基调（透明窗体 + hudWindow 材质 + 卡片浮层），Task 6/7 的开关 UI 叠加其上

- [ ] **Step 1: 窗口配置**

`src-tauri/tauri.conf.json` windows 数组完整替换为：

```json
    "windows": [
      {
        "title": "Local LLM Token Usage Monitor",
        "width": 780,
        "height": 540,
        "resizable": false,
        "transparent": true,
        "titleBarStyle": "Overlay",
        "hiddenTitle": true,
        "theme": "dark",
        "visible": false,
        "windowEffects": {
          "effects": ["hudWindow"],
          "state": "active"
        }
      }
    ],
```

说明：`hudWindow` 对应参考项目的 `NSVisualEffectView(material: .hudWindow)`；`Overlay` + `hiddenTitle` = 内容顶到窗口边缘、红绿灯浮动；`radius` 字段为 Windows 专属故不设置；若 `hudWindow` 效果在当前版本不可用，回退为 `"under-window-background"`。

- [ ] **Step 2: CSS 透明化（逐处修改 `src/styles/app.css`）**

2a. `:root`（:27）：`background-color: var(--system-bg);` → `background-color: transparent;`

2b. `.app`（:35-40）：

```css
.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
  background: transparent;
}
```

2c. `.app-header`（:43-50）：透明浮层 + 左侧让出红绿灯区（约 76px）：

```css
.app-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px 12px 76px;
  background: rgba(255, 255, 255, 0.04);
  border-bottom: 1px solid var(--system-separator);
}
```

2d. `.usage-panel`（:142-147）：`background: var(--system-bg);` → `background: transparent;`

2e. `.p-card`（:194-200）与 `.p-card:hover`：改为浮层公式：

```css
.p-card {
  padding: 16px;
  background: rgba(255, 255, 255, 0.06);
  border-radius: 12px;
  border: 1px solid var(--system-separator);
  transition: all 0.2s ease;
}

.p-card:hover {
  background: rgba(255, 255, 255, 0.10);
  border-color: rgba(255, 255, 255, 0.15);
}
```

2f. `.settings`（:465-470）：`background: var(--system-bg);` → `background: transparent;`

2g. `.settings-header`（:478-486）：`padding: 10px 16px;` → `padding: 10px 16px 10px 76px;`；`background: var(--system-secondary-bg);` → `background: rgba(255, 255, 255, 0.04);`

2h. `.global-settings-row`（:551-559）、`.provider-config`（:626-632）、`.update-card`（:824-832）：`background: var(--system-card-bg);` → `background: rgba(255, 255, 255, 0.06);`（三处）

2i. 输入控件（:609-618）：`background: var(--system-secondary-bg);` → `background: rgba(255, 255, 255, 0.08);`

2j. `.empty-icon`（:425-433）：`background: var(--system-secondary-bg);` → `background: rgba(255, 255, 255, 0.06);`

- [ ] **Step 3: 类型检查**

Run: `cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud && npm run build`
Expected: 构建成功

- [ ] **Step 4: 视觉冒烟验证**

Run: `cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud && npm run tauri dev`
Expected: 窗口 780×540 不可拉伸；毛玻璃透出桌面背景（移动窗口背后内容可见模糊变化）；卡片呈半透明浮层；顶栏/设置头不遮挡红绿灯；顶栏空白区域按住可拖动窗口前需先加 `data-tauri-drag-region`（本任务先不加拖动，视觉确认即可）

- [ ] **Step 5: 提交**

```bash
cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud
git add src-tauri/tauri.conf.json src/styles/app.css
git commit -m "feat: 主窗口 HUD 化（780×540 固定 + hudWindow 毛玻璃 + 半透明浮层卡片）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 6: 顶栏球开关（状态栏总开关，即时生效）

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/styles/app.css`（`.app-header` 附近新增样式块）

**Interfaces:**
- Consumes: Task 1 的 `AppConfig.tray_text_enabled`；既有 `get_config` / `save_config` 命令
- Produces: 顶栏「状态栏」球开关，点击即时落盘并联动托盘；`fetchTrayEnabled` 在视图切换回来时刷新以防设置页保存后状态过期

- [ ] **Step 1: App.tsx 改造**

`src/App.tsx` 完整替换为：

```tsx
import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import './styles/app.css';
import UsagePanel from './components/UsagePanel';
import Settings from './components/Settings';
import UpdateNotice from './components/UpdateNotice';
import ErrorBoundary from './components/ErrorBoundary';
import { listen } from '@tauri-apps/api/event';
import { useUsage } from './hooks/useUsage';
import type { AppConfig } from './types';

function App() {
  const [view, setView] = useState<'usage' | 'settings'>('usage');
  const { summary, loading, error, refresh } = useUsage();
  const [trayTextEnabled, setTrayTextEnabled] = useState(true);

  /** 拉取托盘文字开关；从设置页切回时也刷新，避免设置页保存后状态过期 */
  const fetchTrayEnabled = useCallback(async () => {
    try {
      const cfg = await invoke<AppConfig>('get_config');
      setTrayTextEnabled(cfg.tray_text_enabled);
    } catch (e) {
      console.error(e);
    }
  }, []);

  useEffect(() => {
    fetchTrayEnabled();
  }, [fetchTrayEnabled, view]);

  /** 球开关即时保存：save_config 唤醒调度器重建托盘，菜单栏立即生效 */
  const toggleTrayText = useCallback(async () => {
    try {
      const cfg = await invoke<AppConfig>('get_config');
      const next = { ...cfg, tray_text_enabled: !cfg.tray_text_enabled };
      await invoke('save_config', { newConfig: next });
      setTrayTextEnabled(next.tray_text_enabled);
    } catch (e) {
      console.error(e);
    }
  }, []);

  useEffect(() => {
    const unlisten = listen<string>('navigate', (event) => {
      if (event.payload === 'settings') {
        setView('settings');
      } else if (event.payload === 'usage') {
        setView('usage');
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  return (
    <ErrorBoundary>
      <div className="app">
        {view === 'usage' ? (
          <div className="usage-view">
            <div className="app-header" data-tauri-drag-region>
              <h1 className="app-title">
                <span className="app-title-logo-wrap">
                  <img src="/tray-logo.png" alt="" className="app-title-logo" />
                </span>
                用量
              </h1>
              <div className="header-actions">
                <label className="ball-toggle" title="菜单栏显示开关">
                  <input type="checkbox" checked={trayTextEnabled} onChange={toggleTrayText} />
                  <span className="ball-dot" aria-hidden="true" />
                  <span className="ball-label">状态栏</span>
                </label>
                {summary?.last_updated && (
                  <span className="header-time">{summary.last_updated}</span>
                )}
                <button className="header-refresh-btn" onClick={refresh} title="刷新数据">
                  <svg
                    width="14"
                    height="14"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2.2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <polyline points="23 4 23 10 17 10"></polyline>
                    <polyline points="1 20 1 14 7 14"></polyline>
                    <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"></path>
                  </svg>
                </button>
                <button
                  className="settings-icon-btn"
                  onClick={() => setView('settings')}
                  title="设置"
                >
                  <svg
                    width="18"
                    height="18"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <circle cx="12" cy="12" r="3"></circle>
                    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"></path>
                  </svg>
                </button>
              </div>
            </div>
            <UpdateNotice />
            <UsagePanel summary={summary} loading={loading} error={error} refresh={refresh} />
          </div>
        ) : (
          <Settings onBack={() => setView('usage')} />
        )}
      </div>
    </ErrorBoundary>
  );
}

export default App;
```

- [ ] **Step 2: 球开关样式**

`src/styles/app.css` — `.header-refresh-btn,` 规则之前插入：

```css
/* ===== 球开关（状态栏显示总开关，仿参考项目：选中白实心 / 未选空心环） ===== */
.ball-toggle {
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
  user-select: none;
  margin-right: 4px;
}

.ball-toggle input {
  position: absolute;
  opacity: 0;
  width: 0;
  height: 0;
}

.ball-dot {
  width: 18px;
  height: 18px;
  border-radius: 50%;
  border: 1.5px solid rgba(255, 255, 255, 0.45);
  background: transparent;
  transition: all 0.15s;
  flex-shrink: 0;
}

.ball-toggle input:checked + .ball-dot {
  background: #fff;
  border-color: #fff;
}

.ball-label {
  font-size: 11px;
  color: var(--system-secondary-label);
  transition: color 0.15s;
}

.ball-toggle input:checked ~ .ball-label {
  color: var(--system-label);
}
```

- [ ] **Step 3: 构建 + 手动验证**

Run: `cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud && npm run build`
Expected: 构建成功

Run: `npm run tauri dev`，验证：
1. 点击球开关 → 菜单栏文字立即出现/消失（图标不变）
2. 顶栏空白处拖动可移动窗口；球开关/按钮点击不受影响
3. 设置页保存配置（不改托盘字段）→ 切回用量视图 → 球开关状态与实际一致

- [ ] **Step 4: 提交**

```bash
cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud
git add src/App.tsx src/styles/app.css
git commit -m "feat: 顶栏状态栏球开关，即时切换菜单栏常显文字

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 7: 设置页「状态栏显示」区块（逐服务商开关）

**Files:**
- Modify: `src/components/Settings.tsx`（全局设置区块之后、服务商区块之前插入，约 :309 位置）
- Modify: `src/styles/app.css`（Settings 样式区追加）

**Interfaces:**
- Consumes: Task 1 的 `tray_text_enabled` / `show_in_tray`；既有 `updateProvider(i, field, value)`（Settings.tsx :155-160）
- Produces: 设置页配置表单内的「状态栏显示」区块，随「保存」按钮落盘（与设置页既有显式保存模型一致）

- [ ] **Step 1: 设置页顶栏也支持拖动窗口**

`src/components/Settings.tsx`（:224）的设置页头加拖动区域，与用量视图顶栏行为一致：

```tsx
      <div className="settings-header" data-tauri-drag-region>
```

- [ ] **Step 2: 插入区块 JSX**

`src/components/Settings.tsx` — 在「全局设置」`</div>`（`settings-section` 结束，:308）之后、「服务商」区块之前插入：

```tsx
        {/* 状态栏显示 */}
        <div className="settings-section">
          <h3>状态栏显示</h3>
          <div className="tray-display-card">
            <div className="update-row">
              <span className="update-row-label">在菜单栏图标旁显示指标文字</span>
              <label className="toggle">
                <input
                  type="checkbox"
                  checked={config.tray_text_enabled}
                  onChange={(e) =>
                    setConfig({ ...config, tray_text_enabled: e.target.checked })
                  }
                />
              </label>
            </div>
            {config.tray_text_enabled && (
              <div className="tray-provider-list">
                {config.providers
                  .map((p, i) => ({ p, i }))
                  .filter(({ p }) => p.enabled)
                  .map(({ p, i }) => (
                    <label key={`${p.name}-${p.alias || i}`} className="tray-provider-row">
                      <input
                        type="checkbox"
                        checked={p.show_in_tray}
                        onChange={(e) => updateProvider(i, 'show_in_tray', e.target.checked)}
                      />
                      <span>{p.alias?.trim() || providerLabel(p.name)}</span>
                    </label>
                  ))}
              </div>
            )}
            <span className="update-hint">
              关闭总开关后菜单栏仅显示图标；也可对单个服务商关闭常显。修改需点击右上角「保存」生效。
            </span>
          </div>
        </div>
```

- [ ] **Step 3: 区块样式**

`src/styles/app.css` — `.update-card` 规则之后追加：

```css
/* ===== 状态栏显示设置 ===== */
.tray-display-card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px 14px;
  background: rgba(255, 255, 255, 0.06);
  border-radius: 10px;
  border: 1px solid var(--system-separator);
}

.tray-provider-list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px 14px;
}

.tray-provider-row {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  color: var(--system-secondary-label);
  cursor: pointer;
}

.tray-provider-row input {
  width: auto;
}
```

- [ ] **Step 4: 构建 + 手动验证**

Run: `cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud && npm run build`
Expected: 构建成功

Run: `npm run tauri dev`，验证：
1. 设置页出现「状态栏显示」区块；总开关关闭时服务商列表隐藏
2. 取消某服务商勾选 → 保存 → 菜单栏对应片段消失
3. 已禁用的服务商不出现在列表中
4. 设置页顶栏空白处可拖动窗口，「保存」等按钮不受影响

- [ ] **Step 5: 提交**

```bash
cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud
git add src/components/Settings.tsx src/styles/app.css
git commit -m "feat: 设置页状态栏显示区块，支持逐服务商常显开关

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 8: 刷新体验优化（保留旧数据 + 局部加载指示）

**Files:**
- Modify: `src/hooks/useUsage.ts`
- Modify: `src/components/UsagePanel.tsx`（props :158-163、容器 :198-206）
- Modify: `src/App.tsx`（UsagePanel 调用与刷新按钮）
- Modify: `src/styles/app.css`

**Interfaces:**
- Consumes: 无
- Produces: `useUsage()` 返回值增加 `refreshing: boolean`（手动刷新进行中）；`UsagePanelProps` 增加 `refreshing: boolean`

- [ ] **Step 1: useUsage.ts 增加 refreshing 状态**

完整替换为：

```typescript
import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { UsageSummary } from '../types';

export type { BalanceData, QuotaInfo, UsageData, ProviderStatus, UsageSummary } from '../types';

export function useUsage() {
  const [summary, setSummary] = useState<UsageSummary | null>(null);
  // loading 仅表示首次加载（尚无任何数据）；手动刷新用 refreshing，避免整面板被 loading 替换
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchUsage = useCallback(async () => {
    try {
      setError(null);
      const data = await invoke<UsageSummary>('get_usage');
      setSummary(data);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  const refresh = useCallback(async () => {
    setRefreshing(true);
    try {
      // 先注册一次性事件监听，确保不遗漏后端发出的 usage-updated
      const eventPromise = new Promise<UsageSummary>((resolve) => {
        listen<UsageSummary>('usage-updated', (event) => {
          resolve(event.payload);
        });
      });

      // 通知后端立即轮询供应商
      await invoke('trigger_refresh');

      // 等待 usage-updated 事件（10 秒超时兜底）
      const timeout = new Promise<UsageSummary | null>((resolve) =>
        setTimeout(() => resolve(null), 10000),
      );

      const result = await Promise.race([eventPromise, timeout]);
      if (result) {
        setSummary(result);
      }
      await fetchUsage(); // 最终确认
    } catch {
      await fetchUsage();
    } finally {
      setRefreshing(false);
    }
  }, [fetchUsage]);

  useEffect(() => {
    fetchUsage();

    const unlisten = listen<UsageSummary>('usage-updated', (event) => {
      setSummary(event.payload);
      setLoading(false);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [fetchUsage]);

  return { summary, loading, refreshing, error, refresh };
}
```

- [ ] **Step 2: UsagePanel 传递 refreshing 并加 is-refreshing 类**

`src/components/UsagePanel.tsx` — props 接口与解构（:158-165）改为：

```tsx
interface UsagePanelProps {
  summary: UsageSummary | null;
  loading: boolean;
  refreshing: boolean;
  error: string | null;
  refresh: () => void;
}

function UsagePanel({ summary, loading, refreshing, error, refresh }: UsagePanelProps) {
```

数据面板容器（:198-199）改为：

```tsx
  return (
    <div className={`usage-panel${refreshing ? ' is-refreshing' : ''}`}>
```

- [ ] **Step 3: App.tsx 同步调用与按钮**

`src/App.tsx` 解构处加 `refreshing`：

```tsx
  const { summary, loading, refreshing, error, refresh } = useUsage();
```

刷新按钮加旋转态：

```tsx
                <button
                  className={`header-refresh-btn${refreshing ? ' is-refreshing' : ''}`}
                  onClick={refresh}
                  title="刷新数据"
                >
```

UsagePanel 调用补 prop：

```tsx
            <UsagePanel
              summary={summary}
              loading={loading}
              refreshing={refreshing}
              error={error}
              refresh={refresh}
            />
```

- [ ] **Step 4: 样式**

`src/styles/app.css` — `.usage-panel.loading, .usage-panel.empty` 规则之后追加：

```css
/* 手动刷新进行中：保留旧数据，整体轻微降透明度；顶栏刷新按钮旋转 */
.usage-panel.is-refreshing {
  opacity: 0.6;
  transition: opacity 0.15s;
}

.header-refresh-btn.is-refreshing svg {
  animation: spin 0.8s linear infinite;
}
```

（`@keyframes spin` 已存在于 app.css，无需重复定义）

- [ ] **Step 5: 构建 + 手动验证**

Run: `cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud && npm run build`
Expected: 构建成功

Run: `npm run tauri dev`，点击刷新按钮：旧卡片保持可见、整体降透明度、按钮图标旋转；数据到达后恢复。首次启动（无数据）仍显示加载态。

- [ ] **Step 6: 提交**

```bash
cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud
git add src/hooks/useUsage.ts src/components/UsagePanel.tsx src/App.tsx src/styles/app.css
git commit -m "feat: 手动刷新保留旧数据并显示局部加载指示

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 9: 清理死样式 + 修复未定义变量引用

**Files:**
- Modify: `src/styles/app.css`

**Interfaces:**
- Consumes: 无
- Produces: 无（纯清理，行为不变）

- [ ] **Step 1: grep 验证以下选择器/变量在 src 中无引用**

Run:
```bash
cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud
grep -rn 'total-card\|total-label\|total-value\|p-card-value\|panel-footer\|footer-btn\|footer-time\|system-purple\|system-pink\|system-card-bg\|system-secondary-bg' src --include='*.tsx' --include='*.ts'
```
Expected: 仅 `app.css` 自身定义处有命中、`src/components` 与 `src/App.tsx` 中无 className 使用（`.total-*`、`.p-card-value`、`.panel-footer`、`.footer-*` 均应无 TSX 引用）。若发现仍被引用的选择器，保留并记录，不删。

- [ ] **Step 2: 删除死样式块**

从 `src/styles/app.css` 删除以下整块（行号为大致位置，前置任务可能使其偏移，**以选择器名与注释为准定位**）：
- `/* Total balance card */` 至 `.total-value { ... }` 结束（:158-184）
- `.p-card-value { ... }`（:287-291）
- `/* Footer */` 至 `.footer-btn:hover { ... }` 结束（:392-422）
- `:root` 中 `--system-purple: #bf5af2;` 与 `--system-pink: #ff375f;`（:18-19）
- 若 Step 1 确认 `--system-card-bg`、`--system-secondary-bg` 已无任何使用（Task 5 已替换全部使用处），一并从 `:root` 删除这两个变量定义

- [ ] **Step 3: 修复未定义变量引用**

`.cancel-btn:hover { background: var(--system-quaternary-fill); }`（:686）——`--system-quaternary-fill` 从未定义，改为：

```css
.cancel-btn:hover { background: rgba(255, 255, 255, 0.1); }
```

- [ ] **Step 4: 构建 + lint**

Run: `cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud && npm run build && npm run lint`
Expected: 均通过

- [ ] **Step 5: 提交**

```bash
cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud
git add src/styles/app.css
git commit -m "chore: 清理无引用死样式，修复 cancel-btn 未定义变量引用

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 10: 端到端验证

**Files:** 无新改动（发现问题则回到对应任务修复后重跑）

- [ ] **Step 1: Rust 全量测试**

Run: `cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud/src-tauri && cargo test`
Expected: 全部 PASS

- [ ] **Step 2: 前端构建 + lint**

Run: `cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud && npm run build && npm run lint`
Expected: 均通过

- [ ] **Step 3: 手动验收清单（spec 验收项）**

Run: `npm run tauri dev`，逐项核对：

1. **HUD 视觉**：毛玻璃透出桌面且模糊；卡片半透明浮层；窗口 780×540 不可拉伸；顶栏可拖动；红绿灯不遮挡内容；设置视图在固定高度内滚动
2. **状态栏常显**：菜单栏图标旁出现 `智谱 70% · DeepSeek ¥88.88 · …`；等下次轮询数值更新
3. **总开关**：球开关关闭 → 文字立即消失仅剩图标；开启 → 恢复
4. **逐项控制**：设置页取消某服务商勾选 → 保存 → 对应片段消失
5. **联动一致**：设置页总开关与球开关联动（同一字段，任一处改动另一处刷新后一致）
6. **自启静默**：`npm run tauri dev -- -- --minimized` 窗口不弹、托盘在；普通启动正常弹窗
7. **刷新体验**：手动刷新期间旧数据保留、按钮旋转、无 console 报错
8. **回归**：告警打点、tooltip、菜单重建、关窗隐藏到托盘、自动更新入口均正常

- [ ] **Step 4: 收尾提交（如有验收期修补）**

```bash
cd /Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop-hud
git status --short   # 确认无未提交改动；有则按所属任务语义补充 commit
```
