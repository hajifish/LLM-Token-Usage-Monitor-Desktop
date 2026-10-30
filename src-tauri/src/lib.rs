mod commands;
mod config;
mod models;
mod providers;
mod scheduler;
mod secrets;
mod tray;
mod updater;

use std::sync::Arc;
use tauri::Manager;
use tokio::sync::{Notify, RwLock};

use commands::{get_config, get_config_health, get_usage, save_config, trigger_refresh};
use models::UsageSummary;
use scheduler::Scheduler;
use tauri_plugin_log::{Target, TargetKind};

/// 判断本次启动是否应显示主窗口：开机自启以 --minimized 启动时保持静默驻留托盘。
fn should_show_window(args: &[String]) -> bool {
    !args.iter().any(|a| a == "--minimized")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 日志落地：未安装 logger 时 log crate 走 NopLogger（max_level = Off），
        // release 构建下迁移失败 / 解密失败 / 指纹降级等诊断信息会全部丢失。
        // 写入应用日志目录 + 标准输出；日志内容严禁包含密钥、明文、指纹值与密文。
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .targets([
                    Target::new(TargetKind::LogDir { file_name: None }),
                    Target::new(TargetKind::Stdout),
                ])
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        // 应用内自动更新：端点与公钥在 tauri.conf.json 的 plugins.updater 中配置
        .plugin(tauri_plugin_updater::Builder::new().build())
        // 更新检查结果经系统通知反馈（通知驻留通知中心，避免托盘菜单收起后
        // 结果一闪而过）；macOS 首次投递时系统会自行弹授权询问
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            tray::create_tray(app.handle())?;
            let (app_config, config_health) = config::load(app.handle());
            let config_state = Arc::new(RwLock::new(app_config));
            let summary_state = Arc::new(RwLock::new(UsageSummary::default()));
            let health_state = Arc::new(RwLock::new(config_health));
            let notify = Arc::new(Notify::new());
            app.manage(config_state.clone());
            app.manage(summary_state.clone());
            app.manage(health_state.clone());
            app.manage(notify.clone());
            app.manage(updater::UpdateState::new(
                app.package_info().version.to_string(),
            ));
            let sched = Scheduler::new_with_notify(
                app.handle().clone(),
                config_state.clone(),
                summary_state,
                notify,
            );
            sched.start();
            updater::spawn_auto_check(app.handle().clone(), config_state);
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_usage,
            get_config,
            get_config_health,
            save_config,
            trigger_refresh,
            updater::get_update_status,
            updater::check_update,
            updater::download_update,
            updater::install_update,
            updater::set_skipped_update_version
        ])
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
