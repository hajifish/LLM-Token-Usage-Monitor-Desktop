use crate::models::{AppConfig, ProviderStatus, UsageSummary};
use chrono::{DateTime, Utc};
use std::sync::Mutex;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, Wry,
};

/// 托盘中「检查更新」条目的当前句柄。
///
/// 数据轮询会全量重建菜单并创建新条目，因此句柄随每次重建登记更新；
/// 更新状态变化时只对当前句柄原地 `set_text`（主线程顺序派发），
/// 绝不因状态变化整体替换菜单对象 —— `set_menu` 与用户的菜单交互并发时，
/// AppKit 在菜单动作分发中会 panic（`panic = "abort"` 下表现为 SIGABRT 闪退）。
pub struct TrayUpdateItem(Mutex<MenuItem<Wry>>);

/// 创建「检查更新」条目并登记句柄。菜单重建与初始创建统一走这里，
/// 保证托管句柄始终指向当前已挂载到托盘上的那个条目。
pub fn create_update_item(app: &AppHandle, label: &str) -> tauri::Result<MenuItem<Wry>> {
    let item = MenuItem::with_id(app, "check-update", label, true, None::<&str>)?;
    match app.try_state::<TrayUpdateItem>() {
        Some(slot) => {
            *slot.0.lock().unwrap_or_else(|e| e.into_inner()) = item.clone();
        }
        None => {
            app.manage(TrayUpdateItem(Mutex::new(item.clone())));
        }
    }
    Ok(item)
}

/// 原地刷新「检查更新」条目文案（更新状态变化时调用）。
pub fn refresh_update_item(app: &AppHandle) {
    let label = crate::updater::tray_item_label(app);
    if let Some(state) = app.try_state::<TrayUpdateItem>() {
        let item = state.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Err(e) = item.set_text(label) {
            log::warn!("Tray: 更新条目文案刷新失败: {}", e);
        }
    }
}

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

/// 告警等级（与 provider_warning_level 同阈值）：percent 型且能选出主窗口时按主窗口
/// remaining_percent 判定（≤0→2 / <5→1 / 否则 0），其余情况回退 provider_warning_level。
/// 使 ⚠️/状态点与 V3 显示值同源，配额紧张不再对告警通道不可见。
pub fn quota_warning_level(p: &ProviderStatus) -> u8 {
    if p.balance.as_ref().map(|b| b.is_percent()).unwrap_or(false) {
        if let Some(primary) = p.quota_infos.as_ref().and_then(|qs| primary_quota(qs)) {
            let v = primary.remaining_percent;
            return if v <= 0.0 {
                2
            } else if v < 5.0 {
                1
            } else {
                0
            };
        }
    }
    provider_warning_level(p)
}

/// 服务商类型的菜单栏短代码（1-2 字母，按用户可读性约定）。
/// 未收录的类型返回 None，回退到别名前缀截断。
fn tray_short_code(name: &str) -> Option<&'static str> {
    match name {
        "Zhipu" => Some("Z"),
        "Kimi API" => Some("K"),
        "Kimi Code" => Some("KC"),
        "DeepSeek" => Some("DS"),
        "OpenAI" => Some("OA"),
        "Codex" => Some("CX"),
        "Anthropic" => Some("A"),
        "Claude Code" => Some("CC"),
        _ => None,
    }
}

/// 回退短名（仅未收录类型）：超 8 字符截为 7 字符（去尾部空白）+ …
fn fallback_short_name(alias: &str) -> String {
    let count = alias.chars().count();
    if count <= 8 {
        return alias.to_string();
    }
    let truncated: String = alias.chars().take(7).collect();
    format!("{}…", truncated.trim_end())
}

/// 状态栏片段的指标值部分；无数据/出错显示「–」
fn tray_value(p: &ProviderStatus) -> String {
    match &p.balance {
        Some(b) if b.is_percent() => format!("{:.0}%", b.available_balance),
        Some(b) if b.is_usd() => format!("${:.2}", b.available_balance),
        Some(b) => format!("¥{:.2}", b.available_balance),
        None => "–".to_string(),
    }
}

/// 解析 ISO/RFC3339 重置时刻为距 now 的剩余时长；解析失败或已到期返回 None。
/// （当前仅 quota_countdown_compact 内联同构逻辑，保留为语义文档与测试锚点）
#[cfg_attr(not(test), allow(dead_code))]
pub fn reset_remaining(iso: &str, now: DateTime<Utc>) -> Option<chrono::Duration> {
    let dt = iso.parse::<DateTime<Utc>>().ok()?;
    let d = dt.signed_duration_since(now);
    (d > chrono::Duration::zero()).then_some(d)
}

/// 紧凑倒计时（菜单与菜单栏共用）：分钟 M、小时 H、天 D，整单位省略低位。
/// 如 12M / 1H4M / 6D / 3D4H。
pub fn countdown_compact(d: chrono::Duration) -> String {
    let mins = d.num_minutes().max(1);
    if mins < 60 {
        format!("{mins}M")
    } else if mins < 24 * 60 {
        let (h, m) = (mins / 60, mins % 60);
        if m == 0 {
            format!("{h}H")
        } else {
            format!("{h}H{m}M")
        }
    } else {
        let hours = d.num_hours();
        let (days, h) = (hours / 24, hours % 24);
        if h == 0 {
            format!("{days}D")
        } else {
            format!("{days}D{h}H")
        }
    }
}

/// QuotaInfo 倒计时（紧凑格式）：reset_at 能解析时精确换算——未过期格式化、
/// 已过期省略（spec §5.4，不回退陈旧文本）；reset_at 缺失/解析失败才回退
/// reset_time 原文（provider 层同样输出紧凑格式）
pub fn quota_countdown_compact(q: &crate::models::QuotaInfo, now: DateTime<Utc>) -> Option<String> {
    if let Some(iso) = q.reset_at.as_deref() {
        if let Ok(dt) = iso.parse::<DateTime<Utc>>() {
            let d = dt.signed_duration_since(now);
            return (d > chrono::Duration::zero()).then(|| countdown_compact(d));
        }
    }
    q.reset_time.clone()
}

/// 订阅计划多窗口中的主窗口：剩余百分比最低者；
/// 差 <0.5 视为并列，取重置时刻更近者；无 reset_at 的排最后
pub fn primary_quota(quotas: &[crate::models::QuotaInfo]) -> Option<&crate::models::QuotaInfo> {
    quotas.iter().min_by(|a, b| {
        if (a.remaining_percent - b.remaining_percent).abs() >= 0.5 {
            a.remaining_percent
                .partial_cmp(&b.remaining_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        } else {
            reset_timestamp(a).cmp(&reset_timestamp(b))
        }
    })
}

/// 重置时刻换算为 Unix 秒（并列时比远近用）；缺失/解析失败视为最远
fn reset_timestamp(q: &crate::models::QuotaInfo) -> i64 {
    q.reset_at
        .as_deref()
        .and_then(|s| s.parse::<DateTime<Utc>>().ok())
        .map(|dt| dt.timestamp())
        .unwrap_or(i64::MAX)
}

/// 该服务商是否需要在状态栏展示：按 name + 回退后的 alias 匹配配置项，
/// 匹配不到时默认展示（与 serde default 的「新字段默认开」语义一致）。
/// 两侧 alias 均 trim 后比较：scheduler 的 display_alias 仅过滤纯空白、原样保留其余空白，
/// 配置 alias 带前后空白时若只 trim 一侧会失配，导致逐服务商开关静默失效。
/// 无别名时两侧回退同源：都取 providers::display_name（统一显示名）。
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
                    .unwrap_or(crate::providers::display_name(&c.name))
                    == p.alias.trim()
        })
        .map(|c| c.show_in_tray)
        .unwrap_or(true)
}

/// V3 聚合主值：percent 型取主窗口剩余%（无 quota 回退 balance）；API 型取余额；
/// 无数据返回 f64::MAX（排序垫底）
fn aggregate_value(p: &ProviderStatus) -> f64 {
    let Some(b) = &p.balance else {
        return f64::MAX;
    };
    if b.is_percent() {
        if let Some(primary) = p.quota_infos.as_ref().and_then(|qs| primary_quota(qs)) {
            return primary.remaining_percent;
        }
    }
    b.available_balance
}

fn aggregate_is_percent(p: &ProviderStatus) -> bool {
    p.balance.as_ref().map(|b| b.is_percent()).unwrap_or(false)
}

/// 聚合并列时的主窗口重置时刻（Unix 秒；无则 i64::MAX）
fn p_reset_ts(p: &ProviderStatus) -> i64 {
    p.quota_infos
        .as_ref()
        .and_then(|qs| primary_quota(qs))
        .map(reset_timestamp)
        .unwrap_or(i64::MAX)
}

/// 构建状态栏常显文字（V2 全列表模式）：所有可见供应商逐项短码+主值，
/// 紧张优先排序；告警项加 ⚠️ 前缀。仅展示进度值，不含重置倒计时
/// （重置时刻只进下拉菜单明细行）。同短码多账号从第二个起追加序号（DS、DS2）。
pub fn build_tray_title(config: &AppConfig, summary: &UsageSummary) -> Option<String> {
    if !config.tray_text_enabled {
        return None;
    }
    let mut visible: Vec<&ProviderStatus> = summary
        .providers
        .iter()
        .filter(|p| p.enabled && provider_show_in_tray(config, p))
        .collect();
    if visible.is_empty() {
        return None;
    }
    visible.sort_by(|a, b| {
        crate::tray::quota_warning_level(b)
            .cmp(&crate::tray::quota_warning_level(a))
            .then_with(|| aggregate_is_percent(b).cmp(&aggregate_is_percent(a)))
            .then_with(|| {
                aggregate_value(a)
                    .partial_cmp(&aggregate_value(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                let ta = p_reset_ts(a);
                let tb = p_reset_ts(b);
                ta.cmp(&tb)
            })
    });

    let mut code_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let parts: Vec<String> = visible
        .iter()
        .map(|p| {
            let base = tray_short_code(&p.name)
                .map(str::to_string)
                .unwrap_or_else(|| fallback_short_name(&p.alias));
            let n = code_counts.entry(base.clone()).or_insert(0);
            *n += 1;
            let code = if *n == 1 {
                base
            } else {
                format!("{}{}", base, n)
            };
            let value = match &p.balance {
                Some(b) if b.is_percent() => format!("{:.0}%", aggregate_value(p)),
                _ => tray_value(p),
            };
            let prefix = if quota_warning_level(p) > 0 {
                "⚠️ "
            } else {
                ""
            };
            format!("{prefix}{code} {value}")
        })
        .collect();
    Some(parts.join(" · "))
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
    let update_item = create_update_item(app, "检查更新")?;
    let quit_item = MenuItem::with_id(app, "quit", "⏻\u{FE0E} 退出", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&open_item, &separator1, &update_item, &quit_item])?;

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
        .tooltip("Token Usage")
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
            id if id.starts_with("interval_") => {
                // 托盘「刷新间隔」预设档直选：持久化并唤醒调度器立即生效
                if let Ok(secs) = id.trim_start_matches("interval_").parse::<u64>() {
                    crate::scheduler::set_refresh_interval(app, secs);
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

    /// 带 quota_infos 的状态（V3 聚合测试用）；name/alias 同为 alias，可再覆写 name
    fn status_q(
        alias: &str,
        balance: Option<BalanceData>,
        quotas: Option<Vec<crate::models::QuotaInfo>>,
    ) -> ProviderStatus {
        ProviderStatus {
            name: alias.to_string(),
            alias: alias.to_string(),
            enabled: true,
            balance,
            usage: None,
            error: None,
            quota_infos: quotas,
        }
    }

    fn quota(name: &str, remaining: f64, reset_at: Option<&str>) -> crate::models::QuotaInfo {
        crate::models::QuotaInfo {
            name: name.into(),
            used: 100.0 - remaining,
            total: 100.0,
            current_value: None,
            remaining: Some(remaining),
            remaining_percent: remaining,
            reset_time: None,
            plan_label: None,
            reset_at: reset_at.map(Into::into),
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
    fn v2_lists_all_visible_calm_ordered_by_tension() {
        // V2 全列表：均无告警 → percent 型在前（智谱），API 型按余额升序（$12.34 < ¥88.88）
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
            "智谱 70% · OA $12.34 · DS ¥88.88"
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
        // 同类型两个账号，仅关闭其中一个；展示名用类型短码，alias 不再进菜单栏
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
        // V3 告警只看可见候选：私人号（show_in_tray=false）被隐藏，其 ¥5 告警
        // 不再给标题抢 ⚠️ 前缀，由图标红点（worst_warning/update_tray_badge）承担
        assert_eq!(build_tray_title(&cfg, &s).unwrap(), "DS ¥88.00");
    }

    #[test]
    fn v2_alert_item_marked_and_ranked_first() {
        // V2 全列表：两账号均可见，¥5 黄档（<10）→ 告警项排首并带 ⚠️；
        // 序号按紧张序分配（排序后先到者用基础码）：¥5 → DS，¥88 → DS2
        let mut a = status("工作号", Some(balance(88.0, "CNY")), None);
        a.name = "DeepSeek".into();
        let mut b = status("私人号", Some(balance(5.0, "CNY")), None);
        b.name = "DeepSeek".into();
        let s = UsageSummary {
            providers: vec![a, b],
            ..Default::default()
        };
        assert_eq!(
            build_tray_title(&config(vec![]), &s).unwrap(),
            "⚠️ DS ¥5.00 · DS2 ¥88.00"
        );
    }

    #[test]
    fn provider_types_map_to_short_codes() {
        // 8 种内置类型的短码映射
        let cases = [
            ("Zhipu", "Z"),
            ("Kimi API", "K"),
            ("Kimi Code", "KC"),
            ("DeepSeek", "DS"),
            ("OpenAI", "OA"),
            ("Codex", "CX"),
            ("Anthropic", "A"),
            ("Claude Code", "CC"),
        ];
        for (name, code) in cases {
            let mut p = status(name, Some(balance(70.0, "%")), None);
            p.name = name.into();
            let s = UsageSummary {
                providers: vec![p],
                ..Default::default()
            };
            assert_eq!(
                build_tray_title(&config(vec![]), &s).unwrap(),
                format!("{} 70%", code),
                "类型 {} 应映射为短码 {}",
                name,
                code
            );
        }
    }

    #[test]
    fn known_type_uses_short_code_even_if_long() {
        // "Claude Code" 是已收录类型 → 用短码 CC，不走截断
        let s = UsageSummary {
            providers: vec![status("Claude Code", Some(balance(70.0, "%")), None)],
            ..Default::default()
        };
        assert_eq!(build_tray_title(&config(vec![]), &s).unwrap(), "CC 70%");
    }

    #[test]
    fn fallback_long_unknown_name_truncated() {
        // 未收录类型回退到别名截断：超 8 字符截前 7 字符 + …
        let s = UsageSummary {
            providers: vec![status(
                "SomeUnknownProvider",
                Some(balance(70.0, "%")),
                None,
            )],
            ..Default::default()
        };
        assert_eq!(
            build_tray_title(&config(vec![]), &s).unwrap(),
            "SomeUnk… 70%"
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
    fn warning_prefix_shown_when_level_one() {
        // 余量告警（黄）：文字模式下标题加 ⚠️ 前缀（红黄不再区分，统一警示符）
        let s = UsageSummary {
            providers: vec![status("智谱", Some(balance(3.0, "%")), None)],
            ..Default::default()
        };
        assert_eq!(build_tray_title(&config(vec![]), &s).unwrap(), "⚠️ 智谱 3%");
    }

    #[test]
    fn warning_prefix_shown_when_level_two() {
        // 余量耗尽（红）：同样加 ⚠️ 前缀
        let s = UsageSummary {
            providers: vec![status("DeepSeek", Some(balance(0.0, "CNY")), None)],
            ..Default::default()
        };
        assert_eq!(
            build_tray_title(&config(vec![]), &s).unwrap(),
            "⚠️ DS ¥0.00"
        );
    }

    #[test]
    fn no_warning_prefix_when_level_zero() {
        let s = UsageSummary {
            providers: vec![status("DeepSeek", Some(balance(88.88, "CNY")), None)],
            ..Default::default()
        };
        assert_eq!(build_tray_title(&config(vec![]), &s).unwrap(), "DS ¥88.88");
    }

    #[test]
    fn every_builtin_provider_type_has_short_code() {
        // ALL_PROVIDER_TYPES 是类型清单单一事实源：每个内置类型都必须有短码，
        // 否则菜单栏回退到别名截断，展示退化
        for t in crate::providers::ALL_PROVIDER_TYPES {
            assert!(tray_short_code(t).is_some(), "类型 {} 缺少短码", t);
        }
    }

    #[test]
    fn no_alias_providers_align_via_display_name_matching() {
        // 无别名时两侧回退必须同源：scheduler 的 display_alias 回退到统一显示名
        // （智谱），provider_show_in_tray 的配置侧回退也必须是同一显示名，
        // 否则逐服务商开关对无别名账号静默失效
        let cfg = config(vec![pconfig("Zhipu", None, false)]);
        let mut p = status("智谱", Some(balance(70.0, "%")), None);
        p.name = "Zhipu".into();
        let s = UsageSummary {
            providers: vec![p],
            ..Default::default()
        };
        assert_eq!(build_tray_title(&cfg, &s), None);
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

    #[test]
    fn v2_alert_item_shows_prefix_without_countdown() {
        // Codex 5h 剩 3%（<5 触发黄档）→ ⚠️ 前缀；状态条只展示进度值，
        // 重置倒计时只进下拉菜单明细行，不进状态条
        let qs = vec![quota("5小时", 3.0, Some("2999-01-01T00:00:00Z"))];
        let mut p = status_q("Codex", Some(balance(3.0, "%")), Some(qs));
        p.name = "Codex".into();
        let s = UsageSummary {
            providers: vec![p],
            ..Default::default()
        };
        let title = build_tray_title(&config(vec![]), &s).unwrap();
        assert_eq!(title, "⚠️ CX 3%");
    }

    #[test]
    fn v2_alert_source_matches_display_value() {
        // balance 62% 但主窗口（7天Opus 剩 3%）才是显示值：告警判定必须与显示值同源，
        // ⚠️ 不能因 balance 62% 平静而丢失（quota_warning_level 修复前会漏告警）
        let qs = vec![quota("5小时", 62.0, None), quota("7天Opus", 3.0, None)];
        let mut cc = status_q("CC", Some(balance(62.0, "%")), Some(qs));
        cc.name = "Claude Code".into();
        let s = UsageSummary {
            providers: vec![cc],
            ..Default::default()
        };
        let title = build_tray_title(&config(vec![]), &s).unwrap();
        assert!(title.starts_with("⚠️"), "got: {title}");
        assert!(title.contains("3%"), "got: {title}");
    }

    #[test]
    fn quota_warning_level_tiers_follow_primary_window() {
        // percent 型且有主窗口：按主窗口 remaining_percent 判级（≤0 红 / <5 黄 / 其余绿）
        let qs = |v: f64| vec![quota("5小时", v, None)];
        let p = |v: f64| {
            let mut s = status_q("CC", Some(balance(62.0, "%")), Some(qs(v)));
            s.name = "Claude Code".into();
            s
        };
        assert_eq!(quota_warning_level(&p(0.0)), 2);
        assert_eq!(quota_warning_level(&p(4.9)), 1);
        assert_eq!(quota_warning_level(&p(5.0)), 0);
        assert_eq!(quota_warning_level(&p(62.0)), 0);
        // 无主窗口（quota 缺失）回退 provider_warning_level 语义
        let mut no_quota = status_q("CC", Some(balance(3.0, "%")), None);
        no_quota.name = "Claude Code".into();
        assert_eq!(quota_warning_level(&no_quota), 1);
    }

    #[test]
    fn v2_calm_list_percent_first_no_alert_prefix() {
        let mut cc = status_q(
            "CC",
            Some(balance(62.0, "%")),
            Some(vec![quota("5小时", 62.0, None)]),
        );
        cc.name = "Claude Code".into();
        let mut ds = status("DeepSeek", Some(balance(88.0, "CNY")), None);
        ds.name = "DeepSeek".into();
        let s = UsageSummary {
            providers: vec![cc, ds],
            ..Default::default()
        };
        // 无告警：全列表平静展示，percent 型排前；quota 无 reset_at → 主力也无倒计时
        assert_eq!(
            build_tray_title(&config(vec![]), &s).unwrap(),
            "CC 62% · DS ¥88.00"
        );
    }

    #[test]
    fn v2_no_item_carries_countdown_in_status_bar() {
        // 状态条只展示进度：任何项（含第一名与并列者）都不带重置倒计时；
        // 排序 tie-break（重置更近者居首）仍然生效
        let mut cx = status_q(
            "CX",
            Some(balance(35.0, "%")),
            Some(vec![quota("5小时", 35.0, Some("2999-01-01T00:00:00Z"))]),
        );
        cx.name = "Codex".into();
        let mut z = status_q(
            "Z",
            Some(balance(35.0, "%")),
            Some(vec![quota("5小时", 35.0, Some("2999-06-01T00:00:00Z"))]),
        );
        z.name = "Zhipu".into();
        let s = UsageSummary {
            providers: vec![cx, z],
            ..Default::default()
        };
        let title = build_tray_title(&config(vec![]), &s).unwrap();
        assert_eq!(title, "CX 35% · Z 35%");
    }

    #[test]
    fn v2_api_only_falls_back_to_balance() {
        let mut ds = status("工作号", Some(balance(88.0, "CNY")), None);
        ds.name = "DeepSeek".into();
        let s = UsageSummary {
            providers: vec![ds],
            ..Default::default()
        };
        assert_eq!(build_tray_title(&config(vec![]), &s).unwrap(), "DS ¥88.00");
    }
}

#[cfg(test)]
mod countdown_tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};

    fn q(reset_at: Option<&str>, reset_time: Option<&str>) -> crate::models::QuotaInfo {
        crate::models::QuotaInfo {
            name: "5小时".into(),
            used: 38.0,
            total: 100.0,
            current_value: None,
            remaining: Some(62.0),
            remaining_percent: 62.0,
            reset_time: reset_time.map(Into::into),
            plan_label: None,
            reset_at: reset_at.map(Into::into),
        }
    }

    #[test]
    fn compact_format_boundaries() {
        assert_eq!(countdown_compact(Duration::minutes(12)), "12M");
        assert_eq!(countdown_compact(Duration::minutes(60)), "1H");
        assert_eq!(countdown_compact(Duration::minutes(4 * 60 + 23)), "4H23M");
        assert_eq!(countdown_compact(Duration::hours(24)), "1D");
        assert_eq!(
            countdown_compact(Duration::days(3) + Duration::hours(4)),
            "3D4H"
        );
    }

    #[test]
    fn reset_remaining_parse_and_expiry() {
        let now = Utc.with_ymd_and_hms(2026, 10, 31, 12, 0, 0).unwrap();
        assert_eq!(
            reset_remaining("2026-10-31T13:24:00Z", now),
            Some(Duration::minutes(84))
        );
        assert_eq!(reset_remaining("2026-10-31T12:00:00Z", now), None); // 已到期
        assert_eq!(reset_remaining("garbage", now), None);
    }

    #[test]
    fn quota_countdown_prefers_reset_at_then_falls_back() {
        let now = Utc.with_ymd_and_hms(2026, 10, 31, 12, 0, 0).unwrap();
        // reset_at 精确换算
        let a = q(Some("2026-10-31T13:24:00Z"), Some("1H24M"));
        assert_eq!(quota_countdown_compact(&a, now).as_deref(), Some("1H24M"));
        // 无 reset_at 回退 reset_time 文本（provider 层同输出紧凑格式）
        let b = q(None, Some("12M"));
        assert_eq!(quota_countdown_compact(&b, now).as_deref(), Some("12M"));
        // 均无
        assert_eq!(quota_countdown_compact(&q(None, None), now), None);
    }

    #[test]
    fn quota_countdown_expired_reset_at_omits_without_fallback() {
        let now = Utc.with_ymd_and_hms(2026, 10, 31, 12, 0, 0).unwrap();
        // reset_at 已过期：按 spec §5.4「已过期→省略」，不回退陈旧 reset_time 文本
        let expired = q(Some("2026-10-31T11:00:00Z"), Some("12M"));
        assert_eq!(quota_countdown_compact(&expired, now), None);
        // reset_at 解析失败：回退 reset_time 文本（与缺失同路）
        let unparseable = q(Some("garbage"), Some("12M"));
        assert_eq!(
            quota_countdown_compact(&unparseable, now).as_deref(),
            Some("12M")
        );
    }
}

#[cfg(test)]
mod primary_quota_tests {
    use super::*;
    use crate::models::QuotaInfo;

    fn q(name: &str, remaining: f64, reset_at: Option<&str>) -> QuotaInfo {
        QuotaInfo {
            name: name.into(),
            used: 100.0 - remaining,
            total: 100.0,
            current_value: None,
            remaining: Some(remaining),
            remaining_percent: remaining,
            reset_time: None,
            plan_label: None,
            reset_at: reset_at.map(Into::into),
        }
    }

    #[test]
    fn picks_lowest_remaining() {
        let qs = [q("5小时", 62.0, None), q("7天", 81.0, None)];
        assert_eq!(primary_quota(&qs).unwrap().name, "5小时");
    }

    #[test]
    fn near_tie_prefers_sooner_reset() {
        // 差 0.4 < 0.5 并列 → 取重置更近者（5小时虽然剩余略高但 1h 后重置）
        let qs = [
            q("7天", 62.0, Some("2026-11-06T00:00:00Z")),
            q("5小时", 62.4, Some("2026-10-31T13:00:00Z")),
        ];
        assert_eq!(primary_quota(&qs).unwrap().name, "5小时");
    }

    #[test]
    fn missing_reset_at_sorts_last_on_tie() {
        let qs = [
            q("7天", 62.0, None),
            q("5小时", 62.2, Some("2026-10-31T13:00:00Z")),
        ];
        assert_eq!(primary_quota(&qs).unwrap().name, "5小时");
    }

    #[test]
    fn empty_returns_none() {
        assert!(primary_quota(&[]).is_none());
    }
}
