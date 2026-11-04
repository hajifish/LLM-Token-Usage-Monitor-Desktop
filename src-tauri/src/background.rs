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
