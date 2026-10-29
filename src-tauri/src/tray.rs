use crate::models::{AppConfig, ProviderStatus, UsageSummary};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager,
};

fn tray_icon_path(app: &AppHandle) -> std::path::PathBuf {
    let resource = app
        .path()
        .resource_dir()
        .expect("failed to get resource dir")
        .join("icons")
        .join("tray-icon@2x.png");
    if resource.exists() {
        resource
    } else {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("icons")
            .join("tray-icon@2x.png")
    }
}

/// 0 = ok, 1 = yellow (low), 2 = red (empty)
pub fn provider_warning_level(p: &ProviderStatus) -> u8 {
    match &p.balance {
        Some(b) => {
            let v = b.available_balance;
            // 成本型（USD）供应商不参与低余额告警，避免 $0 花费被误判红色
            if b.is_usd() {
                return 0;
            }
            if v <= 0.0 {
                2
            } else if b.is_percent() {
                if v < 5.0 {
                    1
                } else {
                    0
                }
            } else {
                if v < 10.0 {
                    1
                } else {
                    0
                }
            }
        }
        None => 0,
    }
}

pub fn worst_warning(summary: &UsageSummary) -> u8 {
    summary
        .providers
        .iter()
        .map(provider_warning_level)
        .max()
        .unwrap_or(0)
}

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
/// 两侧 alias 均 trim 后比较：scheduler 的 display_alias 仅过滤纯空白、原样保留其余空白，
/// 配置 alias 带前后空白时若只 trim 一侧会失配，导致逐服务商开关静默失效。
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
                    == p.alias.trim()
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

/// Overlay a colored dot (red/yellow) on the tray icon when any provider is low/empty.
pub fn update_tray_badge(app: &AppHandle, summary: &UsageSummary) {
    let level = worst_warning(summary);
    let Some(tray) = app.tray_by_id("main_tray") else {
        return;
    };
    let Ok(base) = Image::from_path(tray_icon_path(app)) else {
        return;
    };
    let (w, h) = (base.width(), base.height());
    let mut rgba = base.rgba().to_vec();

    if level > 0 {
        let (dr, dg, db) = if level == 2 {
            (255u8, 59u8, 48u8)
        } else {
            (255u8, 204u8, 0u8)
        };
        let cx = w as i32 - 16;
        let cy = 16i32;
        let dot_r = 9i32;
        let ring_r = 12i32;
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let dx = x - cx;
                let dy = y - cy;
                let d2 = dx * dx + dy * dy;
                let idx = (y * w as i32 * 4 + x * 4) as usize;
                if idx + 3 >= rgba.len() {
                    continue;
                }
                if d2 <= dot_r * dot_r {
                    rgba[idx] = dr;
                    rgba[idx + 1] = dg;
                    rgba[idx + 2] = db;
                    rgba[idx + 3] = 255;
                } else if d2 <= ring_r * ring_r {
                    rgba[idx] = 255;
                    rgba[idx + 1] = 255;
                    rgba[idx + 2] = 255;
                    rgba[idx + 3] = 255;
                }
            }
        }
    }

    let img = Image::new_owned(rgba, w, h);
    let _ = tray.set_icon(Some(img));
}

pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    // 初始菜单：紧凑风格，scheduler 会动态替换为带供应商详情的完整菜单
    let open_item = MenuItem::with_id(app, "open-main-window", "打开主页", true, None::<&str>)?;
    let separator1 = PredefinedMenuItem::separator(app)?;
    let update_item = MenuItem::with_id(app, "check-update", "检查更新", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", "⚙ 设置", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "⏻\u{FE0E} 退出", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &open_item,
            &separator1,
            &update_item,
            &settings_item,
            &quit_item,
        ],
    )?;

    // 加载全彩图标（使用 @2x 高分辨率版本，macOS 自动缩放）
    let icon_path = app
        .path()
        .resource_dir()
        .expect("failed to get resource dir")
        .join("icons")
        .join("tray-icon@2x.png");
    let fallback = || {
        app.default_window_icon().cloned().unwrap_or_else(|| {
            log::error!("No default window icon available");
            panic!("No tray icon could be loaded")
        })
    };
    let tray_icon = if icon_path.exists() {
        Image::from_path(&icon_path).unwrap_or_else(|_| fallback())
    } else {
        // dev 模式下从项目目录加载
        let dev_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("icons")
            .join("tray-icon@2x.png");
        Image::from_path(&dev_path).unwrap_or_else(|_| fallback())
    };

    TrayIconBuilder::with_id("main_tray")
        .icon(tray_icon)
        .icon_as_template(false) // 全彩图标，不启用模板模式
        .menu(&menu)
        .tooltip("LLM Token Monitor")
        .on_tray_icon_event(|tray, _event| {
            // 每次点击托盘图标时触发后台刷新，数据在下次打开菜单时可见。
            // macOS 系统行为：点击菜单项后菜单必然关闭，因此刷新由托盘图标点击触发，
            // 而非菜单项点击，避免用户期望"菜单保持打开看更新"的落差。
            let app = tray.app_handle();
            if let Some(notify) = app.try_state::<std::sync::Arc<tokio::sync::Notify>>() {
                notify.notify_one();
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open-main-window" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => app.exit(0),
            "check-update" => crate::updater::on_tray_click(app),
            "settings" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                    let _ = window.emit("navigate", "settings");
                }
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}

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
            providers: vec![status("智谱", Some(balance(70.0, "%")), None)],
            ..Default::default()
        };
        assert_eq!(build_tray_title(&cfg, &s), None);
    }

    #[test]
    fn title_none_when_no_visible_providers() {
        assert_eq!(
            build_tray_title(&config(vec![]), &UsageSummary::default()),
            None
        );
    }

    #[test]
    fn title_joins_mixed_segments_with_separator() {
        let s = UsageSummary {
            providers: vec![
                status("智谱", Some(balance(70.0, "%")), None),
                status("DeepSeek", Some(balance(88.88, "CNY")), None),
                status("OpenAI", Some(balance(12.34, "USD")), None),
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
            providers: vec![status("智谱", Some(balance(70.4, "%")), None)],
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
        let mut p = status("智谱", Some(balance(70.0, "%")), None);
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
            providers: vec![status("智谱", Some(balance(70.0, "%")), None)],
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
        // 调度器实际输出中 name 为服务商类型名（provider.name()），alias 才是账号别名
        let mut work = status("工作号", Some(balance(88.0, "CNY")), None);
        work.name = "DeepSeek".into();
        let mut home = status("私人号", Some(balance(5.0, "CNY")), None);
        home.name = "DeepSeek".into();
        let s = UsageSummary {
            providers: vec![work, home],
            ..Default::default()
        };
        assert_eq!(build_tray_title(&cfg, &s).unwrap(), "工作号 ¥88.00");
    }

    #[test]
    fn long_alias_truncated_to_seven_chars_plus_ellipsis() {
        // "Claude Code"（11 字符）→ 截 7 字符去尾空格 + …
        let s = UsageSummary {
            providers: vec![status("Claude Code", Some(balance(70.0, "%")), None)],
            ..Default::default()
        };
        assert_eq!(
            build_tray_title(&config(vec![]), &s).unwrap(),
            "Claude… 70%"
        );
    }

    #[test]
    fn eight_char_alias_not_truncated() {
        // "Kimi API" 恰 8 字符（K,i,m,i,空格,A,P,I）→ 不截断
        let s = UsageSummary {
            providers: vec![status("Kimi API", Some(balance(70.0, "%")), None)],
            ..Default::default()
        };
        assert_eq!(
            build_tray_title(&config(vec![]), &s).unwrap(),
            "Kimi API 70%"
        );
    }

    #[test]
    fn cjk_multibyte_long_alias_truncated_by_char_count() {
        // "智谱清言助手测试版" 共 9 个汉字 > 8 → 按字符数截前 7 字 + …
        let s = UsageSummary {
            providers: vec![status("智谱清言助手测试版", Some(balance(70.0, "%")), None)],
            ..Default::default()
        };
        assert_eq!(
            build_tray_title(&config(vec![]), &s).unwrap(),
            "智谱清言助手测… 70%"
        );
    }

    #[test]
    fn alias_with_surrounding_whitespace_still_respects_show_in_tray() {
        // 配置 alias 带前后空白：scheduler 原样保留为 display_alias（未 trim），
        // provider_show_in_tray 必须对两侧都 trim 才能匹配，否则开关静默失效
        let cfg = config(vec![pconfig("DeepSeek", Some(" 工作号 "), false)]);
        let mut work = status(" 工作号 ", Some(balance(88.0, "CNY")), None);
        work.name = "DeepSeek".into();
        let s = UsageSummary {
            providers: vec![work],
            ..Default::default()
        };
        assert_eq!(build_tray_title(&cfg, &s), None);
    }
}
