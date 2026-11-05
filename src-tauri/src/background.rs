//! 背景固化：固定背景 ↔ 跟随系统 的窗口材质切换。
//!
//! 固定背景图是随包内置资产（`public/default-background.png`，开发期一次性截取的
//! hudWindow 材质 × 桌面壁纸合成观感），应用运行时不做任何截屏——不存在屏幕录制
//! 权限问题。本模块只负责材质启停这一件事。

use crate::models::BackgroundMode;
use tauri::utils::config::WindowEffectsConfig;
use tauri::utils::{WindowEffect, WindowEffectState};

/// 按模式启停窗口材质。材质启停只走本函数一条路，避免多处改材质导致状态分叉。
///
/// 启动时**不做任何事**：follow 保持 tauri.conf 的 hudWindow；frozen 也先保持材质
/// 兜底观感，由前端铺好内置背景图后再经命令移除——图覆盖在材质之上，移除瞬间
/// 无观感变化，加载期不会闪透明窗口。
pub fn apply_effects(window: &tauri::WebviewWindow, mode: BackgroundMode) -> Result<(), String> {
    match mode {
        // 不移除时窗口圆角外的透明区会继续透出随壁纸变化的材质，
        // 违背「固定背景不随桌面变化」的目标
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

/// 运行时切换窗口材质（固定背景 ↔ 跟随系统）。前端在配置落盘、铺图完成后调用。
#[tauri::command]
pub async fn apply_window_effects(
    window: tauri::WebviewWindow,
    mode: BackgroundMode,
) -> Result<(), String> {
    apply_effects(&window, mode)
}
