# 背景固化（固定背景 / 跟随系统）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 设置页新增「背景」单开关——开启时把当前窗口半透明背景（hudWindow 材质 × 桌面壁纸）截屏固化为 `background.png` 铺为窗口底图，应用观感从此不再随桌面壁纸变化；关闭恢复实时半透明。

**Architecture:** 前端编排截取时序（清空内容 → 等重绘 → invoke Rust 截屏 → 落盘配置 → 铺图 → 切窗口材质）；Rust 提供 `capture_background`（权限预检 + `screencapture -R` 区域截屏）与 `apply_window_effects`（按模式 `set_effects`）两个命令，`AppConfig` 新增 `background_mode` 字段；图片经 assetProtocol 由前端加载。

**Tech Stack:** Tauri 2.11（`set_effects`、assetProtocol、CoreGraphics FFI）、React 18 hooks、`/usr/sbin/screencapture`。

**Spec:** `docs/superpowers/specs/2026-11-04-freeze-background-design.md`（定稿，含全部决策与错误处理表）

**分支:** `feature/freeze-background`（已存在，含 spec 提交，基于 develop `f207ac7`）

## Global Constraints

- **CI 三平台编译**：quality job 跑在 ubuntu-latest（`cargo clippy --all-targets -- -D warnings` + `cargo test --all-targets`），build 矩阵含 windows。所有 macOS 专用代码（FFI、`screencapture`）必须 `#[cfg(target_os = "macos")]` 门控，非 macOS 提供同名存根命令。
- **错误处理惯例**：所有命令返回 `Result<T, String>`；错误文案由 Rust 侧组好完整中文句子（含用户该做什么），前端原样 toast。
- **types.ts ↔ models.rs 严格同步**：字段名 snake_case，两边一致（`background_mode: 'follow' | 'frozen'` ↔ `enum BackgroundMode`，serde `rename_all = "lowercase"`）。
- **不碰版本号**：5 处静态版本号有 CI 一致性守卫，本功能不改任何版本字段。
- **提交信息**：中文 conventional commits（`feat:`/`fix:`/`chore:` + 中文描述）。
- **Rust edition 2021**：`extern "C"` 块内函数调用需 `unsafe` 块；`serde_json` 已是主依赖，测试可直接用。
- **即时生效纪律**：设置项写配置走「读 fresh 配置 → 仅改本字段 → `save_config`」，后端写入成功才同步本地开关状态；失败 toast 不静默。
- **clippy 时序**：`cargo clippy --all-targets -- -D warnings` 全量门在 Task 4（命令注册+启动接线完成）后才有意义——此前模块内函数未接线会误报 dead_code。Task 2/3 只跑 `cargo test` + `cargo fmt`。此外，仅被 macOS 命令引用的辅助函数必须带 `#[cfg_attr(not(target_os = "macos"), allow(dead_code))]`（ubuntu CI 无该调用链，否则必挂）。
- 所有命令在 `src-tauri` 目录下执行（`cd src-tauri && …`）；前端命令在仓库根目录。

---

### Task 1: Rust 配置模型扩展 `background_mode`

**Files:**
- Modify: `src-tauri/src/models.rs`（`AppConfig` struct，约 58-100 行；`Default` impl）
- Test: 同文件 `#[cfg(test)] mod background_mode_tests`

**Interfaces:**
- Consumes: 无
- Produces: `models::BackgroundMode`（`Follow`/`Frozen`，`Copy + PartialEq + Serialize + Deserialize`，serde lowercase，默认 `Follow`）；`AppConfig.background_mode` 字段。后续任务的命令签名引用它。

- [ ] **Step 1: 写失败测试**

在 `src-tauri/src/models.rs` 文件末尾追加：

```rust
#[cfg(test)]
mod background_mode_tests {
    use super::BackgroundMode;

    #[test]
    fn defaults_to_follow_when_field_missing() {
        #[derive(serde::Deserialize)]
        struct Partial {
            #[serde(default)]
            background_mode: BackgroundMode,
        }
        let parsed: Partial = serde_json::from_str("{}").expect("空对象应可解析");
        assert_eq!(parsed.background_mode, BackgroundMode::Follow);
    }

    #[test]
    fn parses_frozen_lowercase() {
        let mode: BackgroundMode =
            serde_json::from_str("\"frozen\"").expect("\"frozen\" 应可解析");
        assert_eq!(mode, BackgroundMode::Frozen);
    }

    #[test]
    fn rejects_unknown_value() {
        assert!(serde_json::from_str::<BackgroundMode>("\"auto\"").is_err());
    }

    #[test]
    fn serializes_lowercase() {
        let json = serde_json::to_string(&BackgroundMode::Frozen).expect("应可序列化");
        assert_eq!(json, "\"frozen\"");
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

```bash
cd src-tauri && cargo test background_mode
```

预期：编译错误 `cannot find type BackgroundMode`。

- [ ] **Step 3: 实现**

在 `src-tauri/src/models.rs` 中，`AppConfig` struct 定义之前加枚举：

```rust
/// 背景模式：follow=跟随系统半透明（默认，现状）；frozen=固定背景（截屏固化）。
/// 序列化为小写字符串，与前端 `types.ts` 的 `'follow' | 'frozen'` 严格一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BackgroundMode {
    #[default]
    Follow,
    Frozen,
}
```

`AppConfig` struct 的 `notifications_enabled` 字段后追加：

```rust
    /// 背景模式：follow=跟随系统半透明（默认）；frozen=固定背景（截屏固化，仅 macOS）
    #[serde(default)]
    pub background_mode: BackgroundMode,
```

`impl Default for AppConfig` 的 `Self { … }` 中 `notifications_enabled` 行后追加：

```rust
            background_mode: BackgroundMode::default(),
```

- [ ] **Step 4: 运行测试确认通过**

```bash
cd src-tauri && cargo test background_mode && cargo test --all-targets
```

预期：4 个新测试 PASS，存量测试不受影响。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/models.rs
git commit -m "feat: 配置模型新增 background_mode（follow/frozen，serde 兼容旧配置）"
```

---

### Task 2: Rust `background` 模块——纯函数与材质切换

**Files:**
- Create: `src-tauri/src/background.rs`
- Modify: `src-tauri/src/lib.rs:1-11`（mod 声明区，按字母序加 `mod background;`）
- Test: `src-tauri/src/background.rs` 内 `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `crate::models::BackgroundMode`（Task 1）
- Produces（后续任务依赖，签名精确如下）:
  - `pub fn region_arg(x: i32, y: i32, w: u32, h: u32, scale: f64) -> String`
  - `pub fn is_valid_capture_size(len: u64) -> bool`
  - `pub fn promote_capture(tmp_path: &std::path::Path, final_path: &std::path::Path) -> Result<(), String>`
  - `pub const BACKGROUND_IMAGE: &str`（值 `"background.png"`）
  - `pub fn apply_effects(window: &tauri::WebviewWindow, mode: BackgroundMode) -> Result<(), String>`
  - `pub fn init_window_effects(app: &tauri::AppHandle, mode: BackgroundMode) -> Result<(), String>`

- [ ] **Step 1: 写失败测试**

创建 `src-tauri/src/background.rs`，先只含测试：

```rust
use crate::models::BackgroundMode;

/// 背景图固定文件名，存放于应用数据目录（不进配置文件）。
pub const BACKGROUND_IMAGE: &str = "background.png";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_arg_retina_2x() {
        assert_eq!(region_arg(100, 200, 720, 600, 2.0), "-R50,100,360,300");
    }

    #[test]
    fn region_arg_negative_coords_secondary_display() {
        assert_eq!(region_arg(-1440, 100, 1440, 900, 2.0), "-R-720,50,720,450");
    }

    #[test]
    fn region_arg_fractional_scale() {
        assert_eq!(region_arg(0, 0, 450, 300, 1.5), "-R0,0,300,200");
    }

    #[test]
    fn capture_size_bounds() {
        assert!(!is_valid_capture_size(0));
        assert!(!is_valid_capture_size(19_999));
        assert!(is_valid_capture_size(20_000));
        assert!(is_valid_capture_size(20_000_000));
        assert!(!is_valid_capture_size(20_000_001));
    }

    #[test]
    fn promote_capture_rejects_tiny_output_and_keeps_old_image() {
        let dir = std::env::temp_dir().join(format!("llmtm-bg-reject-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tmp = dir.join("bg.tmp");
        let final_p = dir.join(BACKGROUND_IMAGE);
        std::fs::write(&final_p, vec![0u8; 40_000]).unwrap(); // 旧图
        std::fs::write(&tmp, vec![0u8; 100]).unwrap(); // 异常小的新截取
        assert!(promote_capture(&tmp, &final_p).is_err());
        assert!(!tmp.exists(), "异常产物应被删除");
        assert_eq!(
            std::fs::metadata(&final_p).unwrap().len(),
            40_000,
            "旧图应保留"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn promote_capture_promotes_valid_output() {
        let dir = std::env::temp_dir().join(format!("llmtm-bg-ok-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tmp = dir.join("bg.tmp");
        let final_p = dir.join(BACKGROUND_IMAGE);
        std::fs::write(&tmp, vec![1u8; 30_000]).unwrap();
        promote_capture(&tmp, &final_p).expect("合法产物应通过");
        assert_eq!(std::fs::metadata(&final_p).unwrap().len(), 30_000);
        assert!(!tmp.exists(), "临时文件应已被改名消费");
        std::fs::remove_dir_all(&dir).ok();
    }
}
```

在 `src-tauri/src/lib.rs` 的 `mod commands;` 上一行（mod 列表按字母序最前）加：

```rust
mod background;
```

- [ ] **Step 2: 运行测试确认失败**

```bash
cd src-tauri && cargo test background
```

预期：编译错误 `cannot find function region_arg`。

- [ ] **Step 3: 实现纯函数与材质切换**

在 `src-tauri/src/background.rs` 中 `BACKGROUND_IMAGE` 常量之后、`mod tests` 之前追加：

```rust
use tauri::Manager;
use tauri::utils::config::{WindowEffect, WindowEffectState, WindowEffectsConfig};

/// 物理像素窗口几何 → `screencapture -R` 的逻辑点参数（原点主屏左上、y 向下，
/// 与 Tauri 全局坐标同空间）。四舍五入到整点；±1pt 误差由前端 `background-size:
/// 100% 100%` 拉伸兜底，不影响正确性。
// 仅 capture_background（macOS）调用；非 macOS 无调用链，显式豁免 dead_code（ubuntu CI clippy 必须过）
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn region_arg(x: i32, y: i32, w: u32, h: u32, scale: f64) -> String {
    let (x, y, w, h) = (x as f64 / scale, y as f64 / scale, w as f64 / scale, h as f64 / scale);
    format!("-R{},{},{},{}", x.round(), y.round(), w.round(), h.round())
}

/// 截取产物大小合理性区间（过小=黑屏/空白截屏，过大=异常产物）。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn is_valid_capture_size(len: u64) -> bool {
    (20_000..=20_000_000).contains(&len)
}

/// 校验临时截取产物并原子覆盖正式背景图（同目录 rename）。
/// 校验失败删除临时文件、保留旧图不覆盖。纯 fs 操作，跨平台可单测。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn promote_capture(
    tmp_path: &std::path::Path,
    final_path: &std::path::Path,
) -> Result<(), String> {
    let len = std::fs::metadata(tmp_path)
        .map_err(|e| format!("读取截取产物失败: {e}"))?
        .len();
    if !is_valid_capture_size(len) {
        let _ = std::fs::remove_file(tmp_path);
        return Err("截取产物大小异常（可能未获授权或屏幕被锁定），已保留原背景图".into());
    }
    std::fs::rename(tmp_path, final_path).map_err(|e| format!("保存背景图失败: {e}"))
}

/// 按模式启停窗口材质。材质启停只走本函数一条路（含启动初始化与运行时命令），
/// 避免 capture 命令与 effects 命令两处都能改材质导致状态分叉。
pub fn apply_effects(window: &tauri::WebviewWindow, mode: BackgroundMode) -> Result<(), String> {
    match mode {
        // 材质在 webview 之下；不移除时窗口圆角外的透明区会继续透出随壁纸变化的
        // 材质，违背「固定背景不随桌面变化」的目标。
        BackgroundMode::Frozen => window
            .set_effects(None)
            .map_err(|e| format!("移除窗口材质失败: {e}")),
        // 与 tauri.conf.json windows[0].windowEffects 现配置保持一致
        BackgroundMode::Follow => window
            .set_effects(WindowEffectsConfig {
                effects: vec![WindowEffect::HudWindow],
                state: Some(WindowEffectState::Active),
                radius: None,
                color: None,
            })
            .map_err(|e| format!("恢复窗口材质失败: {e}")),
    }
}

/// 启动初始化：固定模式且存量图在 → 直接移除材质（前端随后铺图）。
/// 图缺失时不动作（保留材质），前端预加载失败会回退跟随外观并横幅提示。
pub fn init_window_effects(app: &tauri::AppHandle, mode: BackgroundMode) -> Result<(), String> {
    if mode == BackgroundMode::Frozen {
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("获取应用数据目录失败: {e}"))?;
        if dir.join(BACKGROUND_IMAGE).exists() {
            if let Some(window) = app.get_webview_window("main") {
                return apply_effects(&window, mode);
            }
        }
    }
    Ok(())
}
```

- [ ] **Step 4: 运行测试确认通过 + fmt**

```bash
cd src-tauri && cargo test background && cargo fmt
```

预期：6 个测试 PASS（3 个 region_arg + 1 个 size bounds + 2 个 promote）。clippy 全量门按 Global Constraints 延后到 Task 4/8（本任务结束时 `apply_effects` 等尚未接线，dead_code 会误报）。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/background.rs src-tauri/src/lib.rs
git commit -m "feat: 背景固化 Rust 模块——区域换算/产物校验纯函数与材质切换（含单测）"
```

---

### Task 3: `capture_background` 命令（权限预检 + screencapture 截屏落盘）

**Files:**
- Modify: `src-tauri/src/background.rs`（追加命令）

**Interfaces:**
- Consumes: Task 2 的 `region_arg` / `is_valid_capture_size` / `BACKGROUND_IMAGE`
- Produces: Tauri 命令 `capture_background() -> Result<String, String>`（成功返回落盘路径绝对字符串）。前端 `invoke<string>('capture_background')` 调用，无参数。

注：截取本身依赖真实屏幕与权限，无法单测；本任务验证 = 编译 + clippy + 存量测试，端到端在 Task 8 走查。权限语义（来自 spec）：`CGPreflightScreenCaptureAccess` 预检，未授权时调 `CGRequestScreenCaptureAccess` 弹系统授权框后**直接报错**（授权须重启应用才生效，当场截图必然失败）。

- [ ] **Step 1: 实现 macOS 命令**

在 `src-tauri/src/background.rs` 的 `init_window_effects` 之后、`mod tests` 之前追加。FFI 放模块级（函数内 `#[link]` extern 块有被 rustc 忽略的风险，不采用）：

```rust
/// CoreGraphics 屏幕录制权限 FFI（macOS 10.15+）。直接 extern 声明，不引新 crate。
#[cfg(target_os = "macos")]
mod screen_capture {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        pub fn CGPreflightScreenCaptureAccess() -> bool;
        pub fn CGRequestScreenCaptureAccess() -> bool;
    }
}

/// 截取当前窗口区域的「纯净半透明背景」（调用前前端已把全部内容
/// `visibility: hidden`，窗口只剩 hudWindow 材质 × 桌面壁纸的合成观感），
/// 校验后覆盖应用数据目录 `background.png`。仅 macOS。
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn capture_background(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<String, String> {
    // SAFETY: 两个 CG 函数均为无副作用的权限查询/请求入口
    unsafe {
        if !screen_capture::CGPreflightScreenCaptureAccess() {
            // 弹系统授权框；即便用户当场允许，本进程也要重启后才能截屏，
            // 故直接报错引导重启，不继续截图
            let _ = screen_capture::CGRequestScreenCaptureAccess();
            return Err(
                "需要屏幕录制权限：请在 系统设置 → 隐私与安全性 → 屏幕录制 中勾选本应用，\
                 重启应用后再开启固定背景"
                    .into(),
            );
        }
    }

    let pos = window
        .outer_position()
        .map_err(|e| format!("获取窗口位置失败: {e}"))?;
    let size = window
        .outer_size()
        .map_err(|e| format!("获取窗口尺寸失败: {e}"))?;
    let region = region_arg(pos.x, pos.y, size.width, size.height, window.scale_factor());

    tauri::async_runtime::spawn_blocking(move || {
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("获取应用数据目录失败: {e}"))?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建应用数据目录失败: {e}"))?;
        let final_path = dir.join(BACKGROUND_IMAGE);
        let tmp_path = dir.join(format!("{BACKGROUND_IMAGE}.tmp"));

        let status = std::process::Command::new("/usr/sbin/screencapture")
            .args(["-x", &region, &tmp_path.to_string_lossy()])
            .status()
            .map_err(|e| format!("启动 screencapture 失败: {e}"))?;
        if !status.success() {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(format!(
                "截取失败（screencapture 退出码 {}），请重试",
                status.code().unwrap_or(-1)
            ));
        }

        // 校验通过才覆盖旧图；校验失败保留原背景图（promote_capture 已单测）
        promote_capture(&tmp_path, &final_path)?;
        log::info!("背景固化: 已保存 {}", final_path.display());
        Ok(final_path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| format!("截取任务异常: {e}"))?
}

/// 非 macOS：命令存在但直接报错（保证前端 invoke 不至于「命令不存在」，
/// 也让 generate_handler 在三平台都能注册同名命令）。
#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub async fn capture_background() -> Result<String, String> {
    Err("固定背景仅支持 macOS".into())
}
```

注意：`tauri::Manager` trait 已在 Task 2 引入（`app.path()` 需要）。

- [ ] **Step 2: 注册命令**

`src-tauri/src/lib.rs`：在 `use commands::{…}` 之前加：

```rust
use background::capture_background;
```

`generate_handler![…]` 列表中 `trigger_refresh,` 之后加一行：

```rust
        capture_background,
```

- [ ] **Step 3: 编译与测试验证**

```bash
cd src-tauri && cargo test --all-targets && cargo fmt
```

预期：全部测试 PASS。clippy 全量门按 Global Constraints 延后到 Task 4（此时 `apply_effects`/`init_window_effects` 尚未接线，会误报 dead_code）。三平台编译正确性由 cfg 门控与非 macOS 存根保证（Task 8 与 CI 最终把关）。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/background.rs src-tauri/src/lib.rs
git commit -m "feat: capture_background 命令——屏幕权限预检+screencapture 区域截屏落盘"
```

---

### Task 4: `apply_window_effects` 命令 + 启动初始化接线

**Files:**
- Modify: `src-tauri/src/background.rs`（追加命令）
- Modify: `src-tauri/src/lib.rs:52-77`（setup 内接线）

**Interfaces:**
- Consumes: Task 2 的 `apply_effects` / `init_window_effects`
- Produces: Tauri 命令 `apply_window_effects(mode)`，前端以 `invoke('apply_window_effects', { mode: 'frozen' | 'follow' })` 调用。

- [ ] **Step 1: 实现命令**

`src-tauri/src/background.rs` 的 `capture_background` 之后追加（**不**加 cfg——跨平台可编译，frozen 模式在非 macOS 永远进不来，命令不会被调到 frozen 分支以外的异常路径）：

```rust
/// 运行时切换窗口材质（固定背景 ↔ 跟随系统）。前端在配置落盘、铺图完成后调用。
#[tauri::command]
pub async fn apply_window_effects(
    window: tauri::WebviewWindow,
    mode: BackgroundMode,
) -> Result<(), String> {
    apply_effects(&window, mode)
}
```

- [ ] **Step 2: 注册命令 + 启动接线**

`src-tauri/src/lib.rs`：

Task 3 引入的 `use background::capture_background;` 改为：

```rust
use background::{apply_window_effects, capture_background};
```

`generate_handler![…]` 中 `capture_background,` 之后加：

```rust
        apply_window_effects,
```

setup 内接线（两处小改）。第一处，`config::load` 之后立刻取出模式副本（`app_config` 会被 move 进 `RwLock`）：

```rust
            let (app_config, config_health) = config::load(app.handle());
            // app_config 随即被 move 进 RwLock；背景模式为 Copy，先取出副本供启动初始化
            let background_mode = app_config.background_mode;
```

第二处，`updater::spawn_auto_check(app.handle().clone(), config_state);` 之后、`#[cfg(target_os = "macos")]` 的 `set_activation_policy` 之前加：

```rust
            // 背景固化：启动时若为固定模式且存量图在，直接移除窗口材质（前端随后铺图）；
            // 失败仅告警不阻断启动（最坏表现为材质残留，观感回退半透明）
            if let Err(e) = background::init_window_effects(app.handle(), background_mode) {
                log::warn!("背景固化: 启动材质初始化失败: {}", e);
            }
```

- [ ] **Step 3: 编译 + 测试 + clippy/fmt**

```bash
cd src-tauri && cargo clippy --all-targets -- -D warnings && cargo test --all-targets && cargo fmt
```

预期：全部通过。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/background.rs src-tauri/src/lib.rs
git commit -m "feat: apply_window_effects 命令与启动材质初始化接线"
```

---

### Task 5: 前端类型 + `useFrozenBackground` hook + assetProtocol

**Files:**
- Modify: `src/types.ts`（`AppConfig` interface，62-71 行）
- Create: `src/hooks/useFrozenBackground.ts`
- Modify: `src-tauri/tauri.conf.json`（`app.security` 加 `assetProtocol`）

**Interfaces:**
- Consumes: 命令 `get_config` / `save_config` / `capture_background` / `apply_window_effects`（Task 1-4）
- Produces: `useFrozenBackground(): FrozenBackgroundState`，其中

```ts
export interface FrozenBackgroundState {
  mode: 'follow' | 'frozen';
  imageUrl: string | null;
  busy: boolean;
  /** 启动时配置为固定背景但图片加载失败（缺失/损坏）——设置页显示横幅。 */
  loadFailed: boolean;
  setMode: (mode: 'follow' | 'frozen') => Promise<void>;
}
```

`setMode` 语义：成功时状态已翻转；失败时抛出中文错误串且状态不变（开关回弹）；`apply_window_effects` 失败属软失败——状态已翻转但抛「背景已固定，但…」提示文案。Task 6/7 依赖此约定。

- [ ] **Step 1: types.ts 加字段**

`src/types.ts` 的 `AppConfig` interface，`notifications_enabled: boolean;` 之后加：

```ts
  /** 背景模式：follow=跟随系统半透明（默认）；frozen=固定背景（截屏固化，仅 macOS）。 */
  background_mode: 'follow' | 'frozen';
```

- [ ] **Step 2: tauri.conf.json 开 assetProtocol**

`"security": {` 内、`"csp"` 之前加（`csp` 的 `img-src` 已含 `asset:`，无需改 CSP）：

```json
      "assetProtocol": {
        "enable": true,
        "scope": ["$APPDATA/**"]
      },
```

- [ ] **Step 3: 实现 hook**

创建 `src/hooks/useFrozenBackground.ts`：

```ts
import { useCallback, useEffect, useState } from 'react';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { appDataDir, join } from '@tauri-apps/api/path';
import type { AppConfig } from '../types';

export interface FrozenBackgroundState {
  mode: 'follow' | 'frozen';
  imageUrl: string | null;
  busy: boolean;
  /** 启动时配置为固定背景但图片加载失败（缺失/损坏）——设置页显示横幅。 */
  loadFailed: boolean;
  setMode: (mode: 'follow' | 'frozen') => Promise<void>;
}

/** 等待 WebView 完成重绘：双 rAF 保证合成帧提交，再留 150ms 给 WKWebView 落屏。 */
async function nextPaint() {
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
  await new Promise<void>((resolve) => setTimeout(resolve, 150));
}

/** 预加载探测：URL 能否真的取回图（assetProtocol/文件缺失都会走 onerror）。 */
function preload(url: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve();
    img.onerror = () => reject(new Error('背景图加载失败'));
    img.src = url;
  });
}

/** 数据目录 background.png → assetProtocol URL；时间戳 query 防会话内重截后读到缓存。 */
async function buildUrl(): Promise<string> {
  const path = await join(await appDataDir(), 'background.png');
  return `${convertFileSrc(path)}?v=${Date.now()}`;
}

/**
 * 背景固化状态与切换编排。
 *
 * 编排时序（spec「截取流程」）：进入捕获态（html.capture-blank 隐藏全部内容，
 * 只剩窗口材质）→ 等重绘 → capture_background → 读 fresh 配置改本字段落盘 →
 * 预加载新图 → 铺图退出捕获态 → apply_window_effects。
 * 失败路径：退出捕获态、抛错、状态不翻转（开关回弹）、配置不落盘。
 */
export function useFrozenBackground(): FrozenBackgroundState {
  const [mode, setModeState] = useState<'follow' | 'frozen'>('follow');
  const [imageUrl, setImageUrl] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [loadFailed, setLoadFailed] = useState(false);

  // 启动：配置为 frozen 时直接加载存量图（不重截，避免闪屏）；
  // 加载失败回退跟随外观并置 loadFailed 供设置页横幅提示。
  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const cfg = await invoke<AppConfig>('get_config');
        if (!alive || cfg.background_mode !== 'frozen') return;
        try {
          const url = await buildUrl();
          await preload(url);
          if (!alive) return;
          setImageUrl(url);
        } catch {
          if (alive) setLoadFailed(true);
        }
      } catch {
        // get_config 失败：保持跟随外观，配置问题由设置页既有链路提示
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  const setMode = useCallback(async (next: 'follow' | 'frozen') => {
    if (next === 'frozen') {
      setBusy(true);
      document.documentElement.classList.add('capture-blank');
      try {
        await nextPaint();
        await invoke<string>('capture_background');
        // 读最新配置 → 仅改本字段 → 落盘（与设置页其他即时生效区块同纪律）
        const fresh = await invoke<AppConfig>('get_config');
        fresh.background_mode = 'frozen';
        await invoke('save_config', { newConfig: fresh });
        const url = await buildUrl();
        await preload(url);
        setModeState('frozen');
        setImageUrl(url);
        setLoadFailed(false);
      } finally {
        document.documentElement.classList.remove('capture-blank');
        setBusy(false);
      }
      // 材质切换失败为软失败：背景已固定，仅提示（spec 错误处理表）
      await invoke('apply_window_effects', { mode: 'frozen' }).catch((e) => {
        throw new Error(`背景已固定，但窗口材质切换失败：${e}`);
      });
    } else {
      setBusy(true);
      try {
        const fresh = await invoke<AppConfig>('get_config');
        fresh.background_mode = 'follow';
        await invoke('save_config', { newConfig: fresh });
        setModeState('follow');
        setImageUrl(null);
        setLoadFailed(false);
      } finally {
        setBusy(false);
      }
      await invoke('apply_window_effects', { mode: 'follow' }).catch((e) => {
        throw new Error(`已恢复跟随系统，但窗口材质切换失败：${e}`);
      });
    }
  }, []);

  return { mode, imageUrl, busy, loadFailed, setMode };
}
```

- [ ] **Step 4: 类型检查 + lint + 格式化**

```bash
npm run build && npm run lint && npm run format && npm run format:check
```

预期：tsc/vite 构建通过，eslint 无错误，prettier 已归一。

- [ ] **Step 5: 提交**

```bash
git add src/types.ts src/hooks/useFrozenBackground.ts src-tauri/tauri.conf.json
git commit -m "feat: 背景固化前端状态 hook 与 assetProtocol 配置"
```

---

### Task 6: 固定背景层渲染 + 捕获态 CSS + App 集成

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/styles/app.css`（`.app` 规则约 32-37 行；文件末尾追加新规则）

**Interfaces:**
- Consumes: `useFrozenBackground()`（Task 5）；`FrozenBackgroundState` 作为 `background` prop 传给 `Settings`（Task 7 消费）
- Produces: `.app` 内可用的 `background` prop 通道；DOM 约定 `html.capture-blank`（hook 维护）与 `.background-frozen` 层。

- [ ] **Step 1: CSS——背景层与捕获态**

`src/styles/app.css` 的 `.app` 规则改为（加 `position`/`z-index`，让内容压在固定背景层之上）：

```css
.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
  background: transparent;
  position: relative;
  z-index: 1;
}
```

文件末尾追加：

```css
/* ===== 背景固化：固定背景层与截取捕获态 ===== */

/* 固定背景层：铺截取的「半透明背景」图（含材质观感），压在窗口材质之上、
   全部内容之下。截图像素已含窗口圆角外的桌背内容，故按窗口圆角裁掉，
   保持今天的透明圆角观感（半径与实机窗口圆角对齐，Task 8 走查校准）。 */
.background-frozen {
  position: fixed;
  inset: 0;
  background-position: center;
  background-size: 100% 100%;
  background-repeat: no-repeat;
  border-radius: 10px;
  pointer-events: none;
  z-index: 0;
}

/* 截取捕获态：隐藏全部页面内容（含本背景层），仅露出窗口材质供截屏。
   class 由 useFrozenBackground 挂在 <html> 上。 */
html.capture-blank #root {
  visibility: hidden;
}
```

- [ ] **Step 2: App.tsx 集成**

`src/App.tsx`：import 区加（`useUsage` 导入之后）：

```ts
import { useFrozenBackground } from './hooks/useFrozenBackground';
```

组件体内 `useUsage()` 行之后加：

```ts
  const background = useFrozenBackground();
```

return 结构改为（背景层置于 ErrorBoundary 内、`.app` 之前；`Settings` 加 `background` prop）：

```tsx
  return (
    <ErrorBoundary>
      {background.imageUrl && (
        <div
          className="background-frozen"
          style={{ backgroundImage: `url(${background.imageUrl})` }}
        />
      )}
      <div className="app">
```

（`.app` div 内其余内容不变；末尾 `<Settings onBack={() => setView('usage')} />` 改为）

```tsx
          <Settings onBack={() => setView('usage')} background={background} />
```

- [ ] **Step 3: 编译 + lint + 格式化**

```bash
npm run build && npm run lint && npm run format && npm run format:check
```

预期：通过。（此任务后 `npm run tauri dev` 尚无法完整走查——Settings 还没接收 prop 会类型报错，故 Task 6 与 Task 7 必须都完成才能跑 dev；类型错误在本步即暴露，属于预期之外情况需立即修复。）

**注意**：Task 6 Step 2 会因 `Settings` 尚未声明 `background` prop 而 tsc 报错。若希望每步独立可编译，可把 Step 2 的 `background={background}` 传参留到 Task 7 一并加——两种执行顺序均可，以任务执行者按 Task 7 完成后整体编译通过为准。

- [ ] **Step 4: 提交**

```bash
git add src/App.tsx src/styles/app.css
git commit -m "feat: 固定背景层渲染与截取捕获态 CSS，App 接入 useFrozenBackground"
```

---

### Task 7: 设置页「背景」区块

**Files:**
- Modify: `src/components/Settings.tsx`（props 约 6-9 行；「低余额通知」区块结束 479 行与「服务商配置」481 行之间插入新区块；`applyNotificationEnabled` 之后加本区块的处理函数与 busy 状态）

**Interfaces:**
- Consumes: `background: FrozenBackgroundState` prop（Task 6 传入）；`BANNER_TONE` / `BANNER_TITLE` 常量（Settings 内已有）；toast 走现有 `message` state + `save-toast`。
- Produces: 无（终端 UI 任务）。

- [ ] **Step 1: props 与处理函数**

`Settings.tsx` import 区加：

```ts
import type { FrozenBackgroundState } from '../hooks/useFrozenBackground';
```

props interface 改为：

```ts
interface SettingsProps {
  onBack: () => void;
  background: FrozenBackgroundState;
}
```

组件签名 `function Settings({ onBack }: SettingsProps)` 改为 `function Settings({ onBack, background }: SettingsProps)`。

`applyNotificationEnabled` 定义之后加：

```ts
  /** 背景区块开关：编排逻辑在 useFrozenBackground（App 持有），本页只管
   *  busy 态与失败 toast。hook 状态不翻转 = 开关回弹，天然覆盖失败路径。 */
  const [backgroundBusy, setBackgroundBusy] = useState(false);
  const applyBackgroundMode = async (frozen: boolean) => {
    setBackgroundBusy(true);
    try {
      await background.setMode(frozen ? 'frozen' : 'follow');
    } catch (e) {
      setMessage(`背景设置失败: ${e}`);
    } finally {
      setBackgroundBusy(false);
    }
  };
```

- [ ] **Step 2: 区块 JSX**

「低余额通知」区块 `</div>` 结束（479 行）与 `{/* 服务商配置 */}`（481 行）之间插入：

```tsx
        {/* 背景 */}
        <div className="settings-section">
          <h3>背景</h3>
          <div className="tray-display-card">
            <div className="update-row">
              <span className="update-row-label">固定背景（不随桌面壁纸变化）</span>
              <label className="toggle">
                <input
                  type="checkbox"
                  checked={background.mode === 'frozen'}
                  disabled={background.busy || backgroundBusy}
                  onChange={(e) => applyBackgroundMode(e.target.checked)}
                />
              </label>
            </div>
            {background.loadFailed && (
              <div style={BANNER_TONE.warn}>
                <span style={BANNER_TITLE}>背景图加载失败</span>
                <span>已回退为跟随系统外观；请重新开启一次「固定背景」重新截取。</span>
              </div>
            )}
            <span className="update-hint">
              开关即时生效。开启时会闪现约半秒纯背景，随即把当前半透明效果截取为固定图；
              再次开启即重新截取。关闭后恢复实时跟随桌面壁纸。首次使用需授予屏幕录制权限。
            </span>
          </div>
        </div>
```

- [ ] **Step 3: 编译 + lint + 格式化**

```bash
npm run build && npm run lint && npm run format && npm run format:check
```

预期：全部通过（Task 6 遗留的 prop 类型错误至此一并消除）。

- [ ] **Step 4: 提交**

```bash
git add src/components/Settings.tsx
git commit -m "feat: 设置页背景区块——固定背景/跟随系统单开关"
```

---

### Task 8: 全量质量门 + 人工走查验收

**Files:**
- 无新改动（本任务是验证；走查中发现的修复按 `fix:` 提交）

- [ ] **Step 1: 全量质量门（与 CI quality job 同口径）**

```bash
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --all-targets
cd .. && npm run format:check && npm run lint && npm run build
```

预期：全部通过。

- [ ] **Step 2: 人工走查（`npm run tauri dev`）**

按顺序执行（顺序重要：先走未授权失败路径，再授权走完整链路）：

1. **未授权路径**：dev 构建未授予屏幕录制权限时，打开设置 → 背景 → 开启开关 → 预期：系统弹出「屏幕录制」授权对话框（或无弹窗直接失败）、toast 显示「需要屏幕录制权限…重启应用后再开启固定背景」、开关回弹为关、`~/Library/Application Support/com.hajifish.llm-token-monitor/background.png` 不存在。
2. **授权**：系统设置 → 隐私与安全性 → 屏幕录制 → 勾选 dev 构建二进制（`target/debug/...`）→ **重启 dev 应用**（TCC 要求重启生效）。
3. **首次固化**：开启开关 → 预期：闪现约半秒纯半透明背景（无任何 UI 内容残影）→ 恢复后窗口铺固定图，观感与切换前一致（卡片叠加层、圆角、暗化程度）；`background.png` 存在且大小 20KB–20MB。
4. **观感比对**：固化后与固化前对比——若圆角处有残留桌面像素或出现双重圆角，调 `.background-frozen` 的 `border-radius` 与实机窗口圆角对齐后重试。
5. **核心目标**：系统更换桌面壁纸（系统设置 → 壁纸换一张）→ 预期：应用观感**完全不变**。
6. **重启持久**：重启 dev 应用 → 预期：仍为固定背景、无闪屏、无重新截取（`background.png` mtime 不变）。
7. **切回跟随**：关闭开关 → 预期：恢复实时半透明（新壁纸透出）；`background.png` 仍在。
8. **重截语义**：再次开启 → 按当前（新）壁纸重新截取。
9. **横幅路径**：手动删除 `background.png` 后以 frozen 配置重启 → 预期：回退跟随外观 + 设置页「背景图加载失败」横幅；重新开启开关恢复。
10. **多显示器（如有条件）**：窗口移到副屏重截 → 截取内容与窗口位置一致。若坐标错位：实现 spec 备选方案（`screencapture -l<windowID>`），作为后续任务，不阻塞本分支。
11. **`?v=` 缓存语义**：同一会话内 连续两次「关→开」重截 → 第二次铺图应为新图（非缓存旧图）。若 WKWebView 对 asset 协议忽略 query 导致旧图缓存，改用 `#v=${Date.now()}` 片段并复测。

- [ ] **Step 3: 修复走查发现的问题（如有）**

小问题（圆角半径、文案、时序毫秒数）直接改并 `fix:` 提交；坐标/缓存类结构性问题回 spec 记录决策后实施。

- [ ] **Step 4: 提交（如有修复）并汇报**

```bash
git status --short   # 确认无未提交改动
git log --oneline develop..HEAD
```

向用户汇报走查结果与遗留事项，等待人工验收（CLAUDE.md 流程：验收通过后才合回 develop）。
