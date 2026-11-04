use crate::models::BackgroundMode;

/// 背景图固定文件名，存放于应用数据目录（不进配置文件）。
pub const BACKGROUND_IMAGE: &str = "background.png";

use tauri::utils::config::WindowEffectsConfig;
use tauri::utils::{WindowEffect, WindowEffectState};
use tauri::Manager;

/// 物理像素窗口几何 → `screencapture -R` 的逻辑点参数（原点主屏左上、y 向下，
/// 与 Tauri 全局坐标同空间）。四舍五入到整点；±1pt 误差由前端 `background-size:
/// 100% 100%` 拉伸兜底，不影响正确性。
// 仅 capture_background（macOS）调用；非 macOS 无调用链，显式豁免 dead_code（ubuntu CI clippy 必须过）
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn region_arg(x: i32, y: i32, w: u32, h: u32, scale: f64) -> String {
    let (x, y, w, h) = (
        x as f64 / scale,
        y as f64 / scale,
        w as f64 / scale,
        h as f64 / scale,
    );
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
    let scale = window
        .scale_factor()
        .map_err(|e| format!("获取窗口缩放系数失败: {e}"))?;
    let region = region_arg(pos.x, pos.y, size.width, size.height, scale);

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
