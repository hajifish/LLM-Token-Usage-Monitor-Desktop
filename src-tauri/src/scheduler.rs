use crate::models::{AppConfig, BalanceData, ProviderStatus, UsageSummary};
use crate::providers;
use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    AppHandle, Emitter, Manager, Wry,
};
use tokio::sync::{Notify, RwLock};
use tokio::time::Duration;

/// 生成 Unicode 进度条，如 [████████░░]
fn progress_bar(percent: f64, width: usize) -> String {
    let filled = (percent / 100.0 * width as f64).round() as usize;
    let filled = filled.min(width);
    let empty = width.saturating_sub(filled);
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

/// 根据告警级别返回状态点 emoji
fn status_dot(level: u8) -> &'static str {
    match level {
        2 => "🔴",
        1 => "🟡",
        _ => "🟢",
    }
}

/// 从 BalanceData 计算已用百分比（仅百分比类型有意义）
fn used_percent(b: &BalanceData) -> f64 {
    if b.is_percent() {
        (100.0 - b.available_balance).clamp(0.0, 100.0)
    } else {
        0.0
    }
}

/// 格式化单个供应商为紧凑单行文本
fn format_provider_line(p: &ProviderStatus) -> String {
    let dot = status_dot(crate::tray::provider_warning_level(p));
    if let Some(ref balance) = p.balance {
        if balance.is_percent() {
            let remaining = balance.available_balance;
            let used = used_percent(balance);
            let bar = progress_bar(used, 10);
            format!(
                "{} {}  {:.0}%  {}  剩余 {:.0}%",
                dot, p.alias, used, bar, remaining
            )
        } else if balance.is_usd() {
            format!(
                "{} {}  ${:.2}  本月花费",
                dot, p.alias, balance.available_balance
            )
        } else {
            format!(
                "{} {}  ¥{:.2}  可用",
                dot, p.alias, balance.available_balance
            )
        }
    } else if p.error.is_some() {
        format!("{} {}  获取失败", dot, p.alias)
    } else {
        format!("{} {}  无数据", dot, p.alias)
    }
}

/// 供应商列表签名：name|alias 序列。签名不变时托盘菜单只原地改写文案。
fn provider_signature(summary: &UsageSummary) -> String {
    summary
        .providers
        .iter()
        .map(|p| format!("{}|{}", p.name, p.alias))
        .collect::<Vec<_>>()
        .join(";")
}

/// 「更新于 HH:MM」展示文案（取 last_updated 的时:分部分）。
fn update_time_text(summary: &UsageSummary) -> String {
    summary
        .last_updated
        .split(' ')
        .nth(1)
        .and_then(|t| t.rsplit_once(':'))
        .map(|(h, m)| format!("{}:{}", h, m))
        .unwrap_or_else(|| summary.last_updated.clone())
}

/// 托盘菜单条目句柄。菜单只构建一次，数据轮询仅原地 set_text 改写文案，
/// 不再 set_menu 整体替换菜单对象 —— 替换与用户菜单交互并发时 AppKit
/// 会在动作分发中 panic（06337ea 对「检查更新」条目已用同一模式修复，
/// 本结构把该模式推广到数据轮询的供应商条目与时间条目）。
#[derive(Default)]
pub struct TrayMenuState {
    inner: std::sync::Mutex<Option<TrayMenuHandles>>,
}

#[derive(Default)]
struct TrayMenuHandles {
    provider_items: Vec<MenuItem<Wry>>,
    time_item: Option<MenuItem<Wry>>,
    signature: String,
}

pub struct Scheduler {
    app_handle: AppHandle,
    config: Arc<RwLock<AppConfig>>,
    last_summary: Arc<RwLock<UsageSummary>>,
    notify: Arc<Notify>,
}

impl Scheduler {
    pub fn new_with_notify(
        app_handle: AppHandle,
        config: Arc<RwLock<AppConfig>>,
        last_summary: Arc<RwLock<UsageSummary>>,
        notify: Arc<Notify>,
    ) -> Self {
        Self {
            app_handle,
            config,
            last_summary,
            notify,
        }
    }

    pub fn start(&self) {
        let app_handle = self.app_handle.clone();
        let config = self.config.clone();
        let last_summary = self.last_summary.clone();
        let notify = self.notify.clone();

        tauri::async_runtime::spawn(async move {
            // 启动时立即执行一次
            poll_providers(&app_handle, &config, &last_summary).await;

            // 使用短间隔检查，支持配置变更立即触发
            let mut last_poll = std::time::Instant::now();
            loop {
                // 每 5 秒检查一次
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                    _ = notify.notified() => {
                        // 配置变更，立即轮询
                        last_poll = std::time::Instant::now() - Duration::from_secs(999);
                    }
                }

                let interval_secs = {
                    let cfg = config.read().await;
                    cfg.refresh_interval.max(30)
                };

                if last_poll.elapsed().as_secs() >= interval_secs {
                    poll_providers(&app_handle, &config, &last_summary).await;
                    last_poll = std::time::Instant::now();
                }
            }
        });
    }
}

async fn poll_providers(
    app_handle: &AppHandle,
    config: &Arc<RwLock<AppConfig>>,
    last_summary: &Arc<RwLock<UsageSummary>>,
) {
    log::info!("Starting provider data refresh");
    let cfg = {
        let c = config.read().await;
        c.clone()
    };
    // 读锁已释放，后续网络请求不持锁
    let mut provider_statuses = Vec::new();
    let mut total_balance = 0.0;

    for provider_cfg in &cfg.providers {
        let display_alias = provider_cfg
            .alias
            .clone()
            .filter(|a| !a.trim().is_empty())
            .unwrap_or_else(|| provider_cfg.name.clone());

        // Codex / Claude Code / Kimi Code 无需 API Key（凭证从 CLI 配置文件读取），跳过空 key 检查
        let needs_api_key = !matches!(
            provider_cfg.name.as_str(),
            "Codex" | "Claude Code" | "Kimi Code"
        );
        if !provider_cfg.enabled || (needs_api_key && provider_cfg.api_key.is_empty()) {
            provider_statuses.push(ProviderStatus {
                name: provider_cfg.name.clone(),
                alias: display_alias,
                enabled: provider_cfg.enabled,
                balance: None,
                usage: None,
                error: None,
                quota_infos: None,
            });
            continue;
        }

        if let Some(provider) = providers::create_provider(
            &provider_cfg.name,
            &provider_cfg.api_key,
            provider_cfg.platform_token.as_deref(),
        ) {
            match provider.fetch_balance().await {
                Ok(balance) => {
                    // 正向白名单：仅 CNY 余额计入 ¥ 总额（% 配额与 USD 成本均排除，且防未来 EUR 等被误并入）
                    if balance.is_cny() {
                        total_balance += balance.available_balance;
                    }
                    let quota_infos = provider.fetch_quota_infos().await.ok().flatten();
                    let usage = provider.fetch_usage().await.ok().flatten();
                    provider_statuses.push(ProviderStatus {
                        name: provider.name().to_string(),
                        alias: display_alias.clone(),
                        enabled: true,
                        balance: Some(balance),
                        usage,
                        error: None,
                        quota_infos,
                    });
                }
                Err(e) => {
                    log::warn!("Failed to fetch {} data: {}", provider_cfg.name, e);
                    provider_statuses.push(ProviderStatus {
                        name: provider.name().to_string(),
                        alias: display_alias.clone(),
                        enabled: true,
                        balance: None,
                        usage: None,
                        error: Some(e.to_string()),
                        quota_infos: None,
                    });
                }
            }
            // reqwest 异步 Client 的 Drop 会等待连接池收敛，在代理/热点等
            // 连接僵死的环境下可能无限阻塞（采样证实卡在 ClientRef::drop_slow
            // 的 Condvar 上），且 Drop 不受请求超时保护。卸载到阻塞线程池执行，
            // 调度任务与后续轮询不受池收敛速度影响。
            tauri::async_runtime::spawn_blocking(move || drop(provider));
        }
    }

    let summary = UsageSummary {
        providers: provider_statuses,
        total_balance,
        last_updated: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    };

    // 更新共享状态
    {
        let mut s = last_summary.write().await;
        *s = summary.clone();
    }

    // 更新托盘菜单和 tooltip
    update_tray_menu(app_handle, &summary).await;

    // 更新托盘图标警告打点（余量不足时显示红/黄点）
    crate::tray::update_tray_badge(app_handle, &summary);

    // 推送给前端
    let _ = app_handle.emit("usage-updated", &summary);
}

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
        let mut tooltip_lines = vec!["Token Usage".to_string()];
        for p in &summary.providers {
            if let Some(ref balance) = p.balance {
                if balance.is_percent() {
                    tooltip_lines.push(format!(
                        "{}: 剩余 {:.0}%",
                        p.alias, balance.available_balance
                    ));
                } else if balance.is_usd() {
                    tooltip_lines.push(format!(
                        "{}: 本月 ${:.2}",
                        p.alias, balance.available_balance
                    ));
                } else {
                    tooltip_lines.push(format!(
                        "{}: ¥{:.2} (赠金 ¥{:.2} / 现金 ¥{:.2})",
                        p.alias,
                        balance.available_balance,
                        balance.voucher_balance,
                        balance.cash_balance
                    ));
                }
            } else if p.error.is_some() {
                tooltip_lines.push(format!("{}: 获取失败", p.alias));
            }
        }
        tooltip_lines.push(format!("总余额: ¥{:.2}", summary.total_balance));
        tooltip_lines.push(format!("更新: {}", summary.last_updated));
        let _ = tray.set_tooltip(Some(&tooltip_lines.join("\n")));

        // --- 菜单：签名未变时仅原地改写条目文案，不重建菜单对象 ---
        // set_menu 整体替换与用户菜单交互并发会触发 AppKit 闪退
        // （06337ea 修掉了更新状态触发的重建，数据轮询的重建由本处根治）。
        let signature = provider_signature(summary);
        if let Some(state) = app_handle.try_state::<TrayMenuState>() {
            let mut inner = state.inner.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(handles) = inner.as_mut() {
                if handles.signature == signature {
                    for (item, p) in handles
                        .provider_items
                        .iter()
                        .zip(summary.providers.iter())
                    {
                        let _ = item.set_text(&format_provider_line(p));
                    }
                    if let Some(time_item) = &handles.time_item {
                        let _ = time_item.set_text(&format!("更新于 {}", update_time_text(summary)));
                    }
                    return;
                }
            }
        }

        // --- 整体重建（首次 / 服务商列表变化才走这里） ---
        let Ok(menu) = Menu::new(app_handle) else {
            return;
        };

        // 关键项（refresh/settings/quit）append 必须全部成功才装配菜单，
        // 否则会装上缺少"退出"的菜单（ActivationPolicy::Accessory 下托盘退出是唯一出口）。
        let mut critical_ok = true;

        // 1) 供应商状态条目（disabled，不可点击），上限 50 条；id 加序号防别名重复冲突
        const MAX_PROVIDER_ITEMS: usize = 50;
        let mut provider_items = Vec::new();
        for (i, p) in summary
            .providers
            .iter()
            .take(MAX_PROVIDER_ITEMS)
            .enumerate()
        {
            let text = format_provider_line(p);
            if let Ok(item) = MenuItem::with_id(
                app_handle,
                format!("detail_{}_{}", i, p.alias),
                &text,
                false,
                None::<&str>,
            ) {
                let _ = menu.append(&item); // 供应商展示项失败可忽略
                provider_items.push(item);
            }
        }

        // 2) 超过上限则追加截断提示
        if summary.providers.len() > MAX_PROVIDER_ITEMS {
            let text = format!("…共 {} 个供应商", summary.providers.len());
            if let Ok(item) =
                MenuItem::with_id(app_handle, "provider_overflow", &text, false, None::<&str>)
            {
                let _ = menu.append(&item);
            }
        }

        // 3) 没有任何服务商时放一个占位项
        if summary.providers.is_empty() {
            if let Ok(item) =
                MenuItem::with_id(app_handle, "no_provider", "暂无服务商", false, None::<&str>)
            {
                let _ = menu.append(&item);
            }
        }

        // 4) 更新于 HH:MM（disabled 展示项，紧跟供应商状态）
        let mut time_item_handle = None;
        if let Ok(time_item) = MenuItem::with_id(
            app_handle,
            "update-time",
            format!("更新于 {}", update_time_text(summary)),
            false,
            None::<&str>,
        ) {
            let _ = menu.append(&time_item);
            time_item_handle = Some(time_item);
        }

        // 5) 分隔线 → 打开主页
        let Ok(separator1) = PredefinedMenuItem::separator(app_handle) else {
            return;
        };
        critical_ok &= menu.append(&separator1).is_ok();

        let Ok(open_item) = MenuItem::with_id(
            app_handle,
            "open-main-window",
            "打开主页",
            true,
            None::<&str>,
        ) else {
            return;
        };
        critical_ok &= menu.append(&open_item).is_ok();

        // 5.5) 更新入口：发现新版本时文案变为版本号，点击直达主窗口的更新横幅。
        // 条目经 create_update_item 创建并登记句柄，之后状态变化走原地改文案，
        // 不再触发菜单整体重建（重建与用户菜单交互并发会触发 AppKit panic）。
        let update_label = crate::updater::tray_item_label(app_handle);
        if let Ok(update_item) = crate::tray::create_update_item(app_handle, &update_label) {
            critical_ok &= menu.append(&update_item).is_ok();
        }

        // 6) 分隔线 → 设置 + 退出（同一分隔区）
        let Ok(separator2) = PredefinedMenuItem::separator(app_handle) else {
            return;
        };
        critical_ok &= menu.append(&separator2).is_ok();

        let Ok(settings_item) =
            MenuItem::with_id(app_handle, "settings", "⚙ 设置", true, None::<&str>)
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

        // 仅当关键项全部装配成功才替换菜单，否则保留旧菜单
        if critical_ok {
            let _ = tray.set_menu(Some(menu));
            // 登记条目句柄：此后数据轮询仅原地改写文案，不再走重建路径
            let handles = TrayMenuHandles {
                provider_items,
                time_item: time_item_handle,
                signature,
            };
            match app_handle.try_state::<TrayMenuState>() {
                Some(state) => {
                    *state.inner.lock().unwrap_or_else(|e| e.into_inner()) = Some(handles);
                }
                None => {
                    app_handle.manage(TrayMenuState {
                        inner: std::sync::Mutex::new(Some(handles)),
                    });
                }
            }
        }
    }
    let _ = app_handle;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{BalanceData, ProviderStatus};

    // ---------- provider_signature ----------

    fn status_with(name: &str, alias: &str) -> ProviderStatus {
        ProviderStatus {
            name: name.to_string(),
            alias: alias.to_string(),
            enabled: true,
            balance: None,
            usage: None,
            error: None,
            quota_infos: None,
        }
    }

    #[test]
    fn signature_empty_when_no_providers() {
        let s = UsageSummary::default();
        assert_eq!(provider_signature(&s), "");
    }

    #[test]
    fn signature_distinguishes_name_and_alias() {
        let a = UsageSummary {
            providers: vec![status_with("DeepSeek", "工作号")],
            ..Default::default()
        };
        let b = UsageSummary {
            providers: vec![status_with("DeepSeek", "私人号")],
            ..Default::default()
        };
        assert_ne!(provider_signature(&a), provider_signature(&b), "同名不同别名应视为不同签名");
    }

    #[test]
    fn signature_changes_with_provider_count() {
        let one = UsageSummary {
            providers: vec![status_with("DeepSeek", "DeepSeek")],
            ..Default::default()
        };
        let mut two = one.clone();
        two.providers.push(status_with("Zhipu", "Zhipu"));
        assert_ne!(provider_signature(&one), provider_signature(&two));
    }

    #[test]
    fn signature_stable_for_same_providers() {
        let a = UsageSummary {
            providers: vec![
                status_with("DeepSeek", "DeepSeek"),
                status_with("Zhipu", "智谱"),
            ],
            ..Default::default()
        };
        let b = a.clone();
        assert_eq!(provider_signature(&a), provider_signature(&b));
    }

    // ---------- progress_bar ----------

    #[test]
    fn progress_bar_zero_percent() {
        assert_eq!(progress_bar(0.0, 10), "[░░░░░░░░░░]");
    }

    #[test]
    fn progress_bar_hundred_percent() {
        assert_eq!(progress_bar(100.0, 10), "[██████████]");
    }

    #[test]
    fn progress_bar_fifty_percent() {
        assert_eq!(progress_bar(50.0, 10), "[█████░░░░░]");
    }

    #[test]
    fn progress_bar_over_hundred_clamps_to_full() {
        assert_eq!(progress_bar(150.0, 10), "[██████████]");
    }

    #[test]
    fn progress_bar_negative_clamps_to_empty() {
        assert_eq!(progress_bar(-10.0, 10), "[░░░░░░░░░░]");
    }

    // ---------- status_dot ----------

    #[test]
    fn status_dot_level_2_is_red() {
        assert_eq!(status_dot(2), "🔴");
    }

    #[test]
    fn status_dot_level_1_is_yellow() {
        assert_eq!(status_dot(1), "🟡");
    }

    #[test]
    fn status_dot_level_0_is_green() {
        assert_eq!(status_dot(0), "🟢");
    }

    // ---------- used_percent ----------

    fn pct_balance(available: f64) -> BalanceData {
        BalanceData {
            provider: "test".into(),
            available_balance: available,
            voucher_balance: 0.0,
            cash_balance: 0.0,
            total_balance: 100.0,
            currency: "%".into(),
        }
    }

    #[test]
    fn used_percent_normal() {
        let b = pct_balance(70.0); // 剩余 70% → 已用 30%
        assert!((used_percent(&b) - 30.0).abs() < 1e-9);
    }

    #[test]
    fn used_percent_overused_clamps_to_100() {
        let b = pct_balance(-5.0); // 超额 → clamp 到 100
        assert!((used_percent(&b) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn used_percent_full_remaining_zero() {
        let b = pct_balance(100.0); // 满额 → 已用 0
        assert!((used_percent(&b) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn used_percent_non_percent_returns_zero() {
        let b = BalanceData {
            provider: "test".into(),
            available_balance: 42.0,
            voucher_balance: 0.0,
            cash_balance: 0.0,
            total_balance: 100.0,
            currency: "USD".into(),
        };
        assert!((used_percent(&b) - 0.0).abs() < 1e-9);
    }

    // ---------- format_provider_line ----------

    fn make_status(
        alias: &str,
        balance: Option<BalanceData>,
        error: Option<String>,
    ) -> ProviderStatus {
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

    #[test]
    fn format_percent_provider_contains_bar_and_percent() {
        let p = make_status("智谱", Some(pct_balance(70.0)), None);
        let line = format_provider_line(&p);
        assert!(line.contains("█"), "应包含进度条: {line}");
        assert!(line.contains("30%"), "应包含已用百分比: {line}");
        assert!(line.contains("剩余 70%"), "应包含剩余百分比: {line}");
    }

    #[test]
    fn format_usd_provider_contains_dollar() {
        let b = BalanceData {
            provider: "OpenAI".into(),
            available_balance: 12.34,
            voucher_balance: 0.0,
            cash_balance: 0.0,
            total_balance: 12.34,
            currency: "USD".into(),
        };
        let p = make_status("OpenAI", Some(b), None);
        let line = format_provider_line(&p);
        assert!(line.contains("$12.34"), "应包含 $ 金额: {line}");
    }

    #[test]
    fn format_cny_provider_contains_yen() {
        let b = BalanceData {
            provider: "DeepSeek".into(),
            available_balance: 88.88,
            voucher_balance: 10.0,
            cash_balance: 78.88,
            total_balance: 88.88,
            currency: "CNY".into(),
        };
        let p = make_status("DeepSeek", Some(b), None);
        let line = format_provider_line(&p);
        assert!(line.contains("¥88.88"), "应包含 ¥ 金额: {line}");
    }

    #[test]
    fn format_error_provider_contains_failure() {
        let p = make_status("Kimi", None, Some("network error".into()));
        let line = format_provider_line(&p);
        assert!(line.contains("获取失败"), "应包含获取失败: {line}");
    }

    #[test]
    fn format_no_data_provider_contains_no_data() {
        let p = make_status("Unknown", None, None);
        let line = format_provider_line(&p);
        assert!(line.contains("无数据"), "应包含无数据: {line}");
    }
}
