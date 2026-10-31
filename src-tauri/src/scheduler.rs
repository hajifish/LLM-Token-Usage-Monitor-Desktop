use crate::config::ConfigHealth;
use crate::models::{AppConfig, ProviderStatus, QuotaInfo, UsageData, UsageSummary};
use crate::providers;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tauri::{
    menu::{
        CheckMenuItem, IconMenuItem, IconMenuItemBuilder, Menu, MenuItem, PredefinedMenuItem,
        Submenu,
    },
    AppHandle, Emitter, Manager, Wry,
};
use tokio::sync::{Notify, RwLock};
use tokio::time::Duration;

/// 生成 Unicode 进度条，如 [■■■■□□□□□□]（实心=已用，空心=未用）。
/// 空位用 □ 而非 ░：░ 在亮色菜单上近乎隐形（低用量时整条进度条消失），
/// □ 的描边在明暗两种外观下均可辨识，且与实心块等宽对齐。
/// 实心格数向上取整：只要有用量就至少亮 1 格（round 会把 <5% 全部舍成 0 格，
/// 用户消耗后看不到任何进度反馈）。
fn progress_bar(percent: f64, width: usize) -> String {
    let filled = (percent / 100.0 * width as f64).ceil() as usize;
    let filled = filled.min(width);
    let empty = width.saturating_sub(filled);
    format!("[{}{}]", "■".repeat(filled), "□".repeat(empty))
}

/// 供应商菜单 logo（64px PNG，经 Google favicon 服务抓取各家官方标识）。
/// 内嵌进二进制；未收录类型返回 None（无图标条目）。身份视觉由菜单条目图标
/// 承载（IconMenuItem），不再依赖文字内 emoji 徽章。
fn provider_logo(name: &str) -> Option<&'static [u8]> {
    let bytes: &[u8] = match name {
        "Claude Code" => include_bytes!("../icons/providers/claude.png"),
        "Codex" => include_bytes!("../icons/providers/codex.png"),
        "Zhipu" => include_bytes!("../icons/providers/zhipu.png"),
        "Kimi Code" => include_bytes!("../icons/providers/kimicode.png"),
        "Kimi API" => include_bytes!("../icons/providers/kimiapi.png"),
        "DeepSeek" => include_bytes!("../icons/providers/deepseek.png"),
        "OpenAI" => include_bytes!("../icons/providers/openai.png"),
        "Anthropic" => include_bytes!("../icons/providers/anthropic.png"),
        "Qoder" => include_bytes!("../icons/providers/qoder.png"),
        _ => return None,
    };
    Some(bytes)
}

/// 标题行告警后缀：与状态栏标题同源（percent 型按主窗口 quota_warning_level）；
/// 数据获取失败（无 balance）同样 ⚠️。级别红/黄由 🚨/⚠️ 区分。
fn alert_suffix(p: &ProviderStatus) -> &'static str {
    let level = if p.balance.is_none() && p.error.is_some() {
        1
    } else {
        crate::tray::quota_warning_level(p)
    };
    match level {
        2 => " 🚨",
        1 => " ⚠️",
        _ => "",
    }
}

/// 一个供应商的托盘菜单块：标题行 + 明细行（全部配额窗口平铺，不折叠）
pub(crate) struct ProviderMenuBlock {
    pub title: String,
    pub details: Vec<String>,
}

/// 明细行缩进（NSMenu 同级项无层级，用文本缩进模拟从属关系）
const DETAIL_INDENT: &str = "  ";

/// 配额窗口名紧凑化：5小时→5H、7天→7D、5小时窗口→5H、7天Opus→7D Opus。
/// 「订阅余额」等无单位词的名称原样保留。
fn short_window(name: &str) -> String {
    name.replace("小时窗口", "H")
        .replace("天窗口", "D")
        .replace("小时", "H")
        .replace("天Opus", "D Opus")
        .replace("天Sonnet", "D Sonnet")
        .replace("天", "D")
        .replace("每周", "W")
        .replace("周", "W")
        .replace("窗口", "")
}

/// 单个 quota 窗口的菜单明细行：进度条前置（多窗口时条形纵向对齐）
fn quota_line(q: &QuotaInfo, now: DateTime<Utc>) -> String {
    // 已用比例：优先 used/total；total 缺失（如智谱不返回 limit 字段，total=0）时
    // 从 remaining_percent 反推，保证进度条仍有意义（used 是绝对量、total=0 时无意义）
    let used_pct = if q.total > 0.0 {
        (q.used / q.total * 100.0).clamp(0.0, 100.0)
    } else {
        (100.0 - q.remaining_percent).clamp(0.0, 100.0)
    };
    let bar = progress_bar(used_pct, 10);
    let name = short_window(&q.name);
    match crate::tray::quota_countdown_compact(q, now) {
        Some(cd) => format!(
            "{DETAIL_INDENT}{} {} · {:.0}% · {}",
            bar, name, q.remaining_percent, cd
        ),
        None => format!(
            "{DETAIL_INDENT}{} {} · {:.0}%",
            bar, name, q.remaining_percent
        ),
    }
}

/// token 数人性化：980 / 9.8k / 1.2M / 2.5B
fn format_tokens(n: u64) -> String {
    let unit = if n >= 1_000_000_000 {
        (1_000_000_000, "B")
    } else if n >= 1_000_000 {
        (1_000_000, "M")
    } else if n >= 1_000 {
        (1_000, "k")
    } else {
        return n.to_string();
    };
    let v = n as f64 / unit.0 as f64;
    if v >= 10.0 {
        format!("{:.0}{}", v, unit.1)
    } else {
        format!("{:.1}{}", v, unit.1)
    }
}

/// 今日用量明细行（花费/次数/token 有其一才返回）
fn usage_line(u: &UsageData) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(cost) = u.today_cost {
        parts.push(format!("{:.2}", cost));
    }
    if let Some(req) = u.today_requests {
        parts.push(format!("{}次", req));
    }
    if u.total_tokens > 0 {
        parts.push(format!("{} tok", format_tokens(u.total_tokens)));
    }
    if parts.is_empty() {
        None
    } else {
        Some(format!("{DETAIL_INDENT}今日 {}", parts.join(" · ")))
    }
}

/// 供应商 → 菜单块。percent 型消费 quota_infos（标题主指标取主窗口 remaining_percent，
/// 全部窗口平铺为明细行、保持接口返回顺序）；API 型标题为余额、有 usage 时加今日明细行
fn provider_menu_block(p: &ProviderStatus, now: DateTime<Utc>) -> ProviderMenuBlock {
    let suffix = alert_suffix(p);
    let percent_quota = p
        .balance
        .as_ref()
        .filter(|b| b.is_percent())
        .and_then(|_| p.quota_infos.as_ref().filter(|qs| !qs.is_empty()));

    if let Some(quotas) = percent_quota {
        if let Some(primary) = crate::tray::primary_quota(quotas) {
            return ProviderMenuBlock {
                title: format!("{} — {:.0}%{}", p.alias, primary.remaining_percent, suffix),
                details: quotas.iter().map(|q| quota_line(q, now)).collect(),
            };
        }
    }

    match &p.balance {
        Some(b) if b.is_percent() => ProviderMenuBlock {
            title: format!("{} — {:.0}%{}", p.alias, b.available_balance, suffix),
            details: vec![],
        },
        Some(b) if b.is_usd() => ProviderMenuBlock {
            title: format!("{} — ${:.2}{}", p.alias, b.available_balance, suffix),
            details: p.usage.as_ref().and_then(usage_line).into_iter().collect(),
        },
        Some(b) => ProviderMenuBlock {
            title: format!("{} — ¥{:.2}{}", p.alias, b.available_balance, suffix),
            details: p.usage.as_ref().and_then(usage_line).into_iter().collect(),
        },
        None if p.error.is_some() => ProviderMenuBlock {
            title: format!("{} — 获取失败{}", p.alias, suffix),
            details: vec![],
        },
        None => ProviderMenuBlock {
            title: format!("{} — 无数据{}", p.alias, suffix),
            details: vec![],
        },
    }
}

/// 供应商列表签名：name|alias|quota名序列。签名不变时托盘菜单只原地改写文案；
/// quota 窗口数量/顺序变化（影响菜单结构）时触发整体重建。
fn provider_signature(summary: &UsageSummary) -> String {
    summary
        .providers
        .iter()
        .map(|p| {
            let quotas = p
                .quota_infos
                .as_ref()
                .map(|qs| {
                    qs.iter()
                        .map(|q| q.name.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            format!("{}|{}|{}", p.name, p.alias, quotas)
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// 「更新于 HH:MM」展示文案（取 last_updated 的时:分部分，去掉秒）。
fn update_time_text(summary: &UsageSummary) -> String {
    summary
        .last_updated
        .split(' ')
        .nth(1)
        .and_then(|t| {
            let mut parts = t.split(':');
            let h = parts.next()?;
            let m = parts.next()?;
            Some(format!("{}:{}", h, m))
        })
        .unwrap_or_else(|| summary.last_updated.clone())
}

/// 托盘「刷新间隔」预设档（最短 1 分钟，避免请求过频）。
const REFRESH_INTERVAL_OPTIONS: &[u64] = &[60, 120, 180, 300, 600, 1800, 3600, 21600];

/// Provider Drop 卸载到阻塞线程池的并发上限（OnceLock 初始化的模块级信号量）。
/// 超限说明阻塞线程池已被连接池收敛长时间占满，再排队只会堆积任务。
static DROP_SEMAPHORE: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> =
    std::sync::OnceLock::new();

fn drop_semaphore() -> Arc<tokio::sync::Semaphore> {
    DROP_SEMAPHORE
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(16)))
        .clone()
}

/// 刷新间隔档位的菜单文案。
fn interval_label(secs: u64) -> String {
    match secs {
        60 => "1 分钟".to_string(),
        120 => "2 分钟".to_string(),
        180 => "3 分钟".to_string(),
        300 => "5 分钟".to_string(),
        600 => "10 分钟".to_string(),
        1800 => "30 分钟".to_string(),
        3600 => "1 小时".to_string(),
        21600 => "6 小时".to_string(),
        other => format!("{} 秒", other),
    }
}

/// 托盘菜单点击「刷新间隔」档位：持久化到配置并唤醒调度器立即生效。
/// 勾选标记由下一次 update_tray_menu 原地同步（配置变更会立即触发轮询）。
pub fn set_refresh_interval(app: &AppHandle, secs: u64) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let (Some(config_state), Some(health_state), Some(notify)) = (
            app.try_state::<Arc<RwLock<AppConfig>>>(),
            app.try_state::<Arc<RwLock<ConfigHealth>>>(),
            app.try_state::<Arc<Notify>>(),
        ) {
            let mut new_config = config_state.read().await.clone();
            new_config.refresh_interval = secs;
            if let Err(e) =
                crate::commands::persist_config(&app, &config_state, &health_state, new_config)
                    .await
            {
                log::error!("Failed to set refresh interval: {}", e);
                // 持久化失败时勾选标记仍停在用户点的档位上，与真实配置脱节；
                // 唤醒一次轮询，fast path 会把勾选标记原地同步回真实配置（可见回弹）
                notify.notify_one();
            }
            log::info!("刷新间隔已设置为 {} 秒", secs);
            notify.notify_one();
        }
    });
}

/// 托盘菜单条目句柄。菜单只构建一次，数据轮询仅原地 set_text 改写文案，
/// 不再 set_menu 整体替换菜单对象 —— 替换与用户菜单交互并发时 AppKit
/// 会在动作分发中 panic（06337ea 对「检查更新」条目已用同一模式修复，
/// 本结构把该模式推广到数据轮询的供应商条目与时间条目）。
#[derive(Default)]
pub struct TrayMenuState {
    inner: std::sync::Mutex<Option<TrayMenuHandles>>,
}

/// 单个供应商块的菜单条目句柄：标题行（带 logo 图标）+ 明细行（全部窗口平铺）
struct ProviderMenuHandles {
    title: IconMenuItem<Wry>,
    details: Vec<MenuItem<Wry>>,
}

#[derive(Default)]
struct TrayMenuHandles {
    provider_blocks: Vec<ProviderMenuHandles>,
    time_item: Option<MenuItem<Wry>>,
    interval_items: Vec<(u64, CheckMenuItem<Wry>)>,
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
            .unwrap_or_else(|| providers::display_name(&provider_cfg.name).to_string());

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
                    // 瞬时拉取失败 → None → 签名变化 → 菜单整体重建（恢复时再建一次）：
                    // 这是「结构如实反映数据」与「避免重建 churn」的既有权衡。quota/usage
                    // 为可选增强（余额仍展示），失败不阻塞主流程；若网络不稳环境出现高频
                    // 重建，可引入上轮 quota 名序列的滞后缓存来抑制抖动。
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
            // 调度任务与后续轮询不受池收敛速度影响。信号量限制在途 Drop 数量，
            // 防止僵死 Drop 堆积耗尽线程池、饿死其他 spawn_blocking 任务。
            match drop_semaphore().try_acquire_owned() {
                Ok(permit) => {
                    tauri::async_runtime::spawn_blocking(move || {
                        drop(provider);
                        drop(permit);
                    });
                }
                Err(_) => {
                    // 排队超限：泄漏一个连接池（进程生命周期内不再收敛），
                    // 保全阻塞线程池供更新安装等关键路径使用——泄漏优于饿死
                    log::warn!("Provider Drop 排队超限，泄漏一个 HTTP 连接池以保全线程池");
                    std::mem::forget(provider);
                }
            }
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

    // 更新托盘菜单、tooltip、图标/文字展示与告警打点
    update_tray_menu(app_handle, &summary).await;

    // 推送给前端
    let _ = app_handle.emit("usage-updated", &summary);
}

async fn update_tray_menu(app_handle: &AppHandle, summary: &UsageSummary) {
    if let Some(tray) = app_handle.tray_by_id("main_tray") {
        // --- 状态栏常显文字（菜单栏图标旁的主指标摘要） ---
        let tray_config = match app_handle.try_state::<Arc<RwLock<AppConfig>>>() {
            Some(state) => state.read().await.clone(),
            None => {
                // 正常启动后必有该状态；走到这里说明时序异常（如状态尚未 manage），
                // 留痕便于排查，不再完全静默
                log::warn!("TrayMenuState: 未找到 AppConfig 状态，回退默认配置");
                AppConfig::default()
            }
        };
        // 有供应商上状态栏时：只显示文字、隐藏软件 logo；没有任何供应商展示时：
        // 只显示软件 logo（带告警打点）、不显示文字，避免菜单栏重复占用
        let title = crate::tray::build_tray_title(&tray_config, summary);
        match title.as_deref() {
            Some(t) => {
                let _ = tray.set_title(Some(t));
                let _ = tray.set_icon(None);
            }
            None => {
                // 注意：tray-icon 的 set_title(None) 是静默无操作（不会清空按钮
                // 标题），必须传空字符串才能真正清掉旧文字
                let _ = tray.set_title(Some(""));
                crate::tray::update_tray_badge(app_handle, summary);
            }
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
                    let now = Utc::now();
                    for (block, p) in handles.provider_blocks.iter().zip(summary.providers.iter()) {
                        let b = provider_menu_block(p, now);
                        let _ = block.title.set_text(b.title);
                        for (item, text) in block.details.iter().zip(b.details.iter()) {
                            let _ = item.set_text(text);
                        }
                        // usage 行出现/消失不改变签名（quota 名序列不变），zip 只覆盖
                        // 交集，剩余旧句柄须显式清空，避免「今日…」行残留陈旧文案
                        for item in block.details.iter().skip(b.details.len()) {
                            let _ = item.set_text("");
                        }
                    }
                    if let Some(time_item) = &handles.time_item {
                        let _ = time_item.set_text(format!("更新于 {}", update_time_text(summary)));
                    }
                    // 同步刷新间隔勾选标记（配置可能在设置页被改动）
                    for (secs, item) in &handles.interval_items {
                        let _ = item.set_checked(*secs == tray_config.refresh_interval);
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

        // 1) 供应商块（标题行 + 明细行全平铺），块上限沿用 50；id 加序号防别名重复冲突。
        //    展示行一律 enabled=true：原生菜单把 disabled 渲染成 50% 灰（用户反馈「字太淡」），
        //    enabled 展示行点击无动作（id 无 on_menu_event 匹配），代价仅是点击后菜单收起。
        const MAX_PROVIDER_BLOCKS: usize = 50;
        let now = Utc::now();
        let mut provider_blocks = Vec::new();
        for (i, p) in summary
            .providers
            .iter()
            .take(MAX_PROVIDER_BLOCKS)
            .enumerate()
        {
            let block = provider_menu_block(p, now);
            // 标题行用 IconMenuItem：供应商官方 logo 作为条目图标（NSMenuItem.setImage，
            // 18pt 渲染）；未收录类型无图标。logo 跟随供应商名（签名含 name），
            // 结构不变时无需 set_icon，仅原地 set_text 刷新文案。
            let mut title_builder =
                IconMenuItemBuilder::with_id(format!("p{i}_title"), &block.title).enabled(true);
            if let Some(logo) = provider_logo(&p.name) {
                if let Ok(img) = tauri::image::Image::from_bytes(logo) {
                    title_builder = title_builder.icon(img);
                }
            }
            let Ok(title) = title_builder.build(app_handle) else {
                // title 创建失败跳过整块：provider_blocks 与 summary.providers 从此失去
                // 1:1 对应，原地刷新的 zip 会整体错位、把文案写到别的供应商行上。该失败
                // 为极端路径（MenuItem 创建几乎不会失败），沿用既有实现「供应商展示项
                // 可忽略」的权衡（spec §6），不做部分降级。
                continue;
            };
            let _ = menu.append(&title);

            let mut detail_items = Vec::new();
            for (j, text) in block.details.iter().enumerate() {
                if let Ok(item) =
                    MenuItem::with_id(app_handle, format!("p{i}_d{j}"), text, true, None::<&str>)
                {
                    let _ = menu.append(&item);
                    detail_items.push(item);
                }
            }

            provider_blocks.push(ProviderMenuHandles {
                title,
                details: detail_items,
            });
        }

        // 2) 超过上限则追加截断提示
        if summary.providers.len() > MAX_PROVIDER_BLOCKS {
            let text = format!("…共 {} 个供应商", summary.providers.len());
            if let Ok(item) =
                MenuItem::with_id(app_handle, "provider_overflow", &text, true, None::<&str>)
            {
                let _ = menu.append(&item);
            }
        }

        // 3) 没有任何服务商时放一个占位项
        if summary.providers.is_empty() {
            if let Ok(item) =
                MenuItem::with_id(app_handle, "no_provider", "暂无服务商", true, None::<&str>)
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
            true,
            None::<&str>,
        ) {
            let _ = menu.append(&time_item);
            time_item_handle = Some(time_item);
        }

        // 4.5) 刷新间隔子菜单（仿 Traffic Monitor 的预设档直选，勾选当前档；
        //      点击经 set_refresh_interval 持久化，勾选标记随下一次轮询原地同步）
        let mut interval_items = Vec::new();
        if let Ok(submenu) = Submenu::new(app_handle, "刷新间隔", true) {
            for secs in REFRESH_INTERVAL_OPTIONS {
                let checked = *secs == tray_config.refresh_interval;
                if let Ok(item) = CheckMenuItem::with_id(
                    app_handle,
                    format!("interval_{}", secs),
                    interval_label(*secs),
                    true,
                    checked,
                    None::<&str>,
                ) {
                    let _ = submenu.append(&item);
                    interval_items.push((*secs, item));
                }
            }
            let _ = menu.append(&submenu);
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

        // 6) 分隔线 → 退出（主窗口头部已有设置入口，托盘不再重复）
        let Ok(separator2) = PredefinedMenuItem::separator(app_handle) else {
            return;
        };
        critical_ok &= menu.append(&separator2).is_ok();

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
                provider_blocks,
                time_item: time_item_handle,
                interval_items,
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
    use crate::models::ProviderStatus;

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
        assert_ne!(
            provider_signature(&a),
            provider_signature(&b),
            "同名不同别名应视为不同签名"
        );
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

    #[test]
    fn signature_changes_when_order_differs() {
        // 原地 zip 对齐不变量：同集合不同顺序必须产生不同签名，
        // 否则签名命中会让 zip 错位、文案写到别的供应商行上
        let a = UsageSummary {
            providers: vec![
                status_with("DeepSeek", "工作号"),
                status_with("Zhipu", "智谱"),
            ],
            ..Default::default()
        };
        let b = UsageSummary {
            providers: vec![
                status_with("Zhipu", "智谱"),
                status_with("DeepSeek", "工作号"),
            ],
            ..Default::default()
        };
        assert_ne!(provider_signature(&a), provider_signature(&b));
    }

    #[test]
    fn signature_changes_when_quota_set_changes() {
        let p1 = status_full(
            "CC",
            "a",
            None,
            Some(vec![quota("5小时", 38.0, None)]),
            None,
        );
        let p2 = status_full(
            "CC",
            "a",
            None,
            Some(vec![quota("5小时", 38.0, None), quota("7天", 19.0, None)]),
            None,
        );
        let s1 = UsageSummary {
            providers: vec![p1],
            ..Default::default()
        };
        let s2 = UsageSummary {
            providers: vec![p2],
            ..Default::default()
        };
        assert_ne!(provider_signature(&s1), provider_signature(&s2));
    }

    #[test]
    fn signature_stable_when_only_values_change() {
        let p1 = status_full(
            "CC",
            "a",
            None,
            Some(vec![quota("5小时", 38.0, None)]),
            None,
        );
        let p2 = status_full(
            "CC",
            "a",
            None,
            Some(vec![quota("5小时", 41.0, None)]),
            None,
        );
        let s1 = UsageSummary {
            providers: vec![p1],
            ..Default::default()
        };
        let s2 = UsageSummary {
            providers: vec![p2],
            ..Default::default()
        };
        assert_eq!(provider_signature(&s1), provider_signature(&s2));
    }

    // ---------- update_time_text ----------

    #[test]
    fn update_time_text_extracts_hour_minute() {
        let s = UsageSummary {
            last_updated: "2026-10-31 08:00:00".to_string(),
            ..Default::default()
        };
        assert_eq!(update_time_text(&s), "08:00");
    }

    #[test]
    fn update_time_text_empty_falls_back_to_raw() {
        let s = UsageSummary {
            last_updated: String::new(),
            ..Default::default()
        };
        assert_eq!(update_time_text(&s), "");
    }

    // ---------- interval_label ----------

    #[test]
    fn interval_label_maps_all_preset_options() {
        let cases = [
            (60, "1 分钟"),
            (120, "2 分钟"),
            (180, "3 分钟"),
            (300, "5 分钟"),
            (600, "10 分钟"),
            (1800, "30 分钟"),
            (3600, "1 小时"),
            (21600, "6 小时"),
        ];
        for (secs, label) in cases {
            assert_eq!(interval_label(secs), label, "{} 秒档文案应正确", secs);
        }
        // 预设档清单与映射表规模一致（8 档全映射）
        assert_eq!(REFRESH_INTERVAL_OPTIONS.len(), cases.len());
    }

    #[test]
    fn interval_label_legacy_30s_falls_back() {
        // 存量旧配置可能仍存有 30 秒档：回退为「30 秒」而不是 panic
        assert_eq!(interval_label(30), "30 秒");
    }

    // ---------- progress_bar ----------

    #[test]
    fn progress_bar_zero_percent() {
        assert_eq!(progress_bar(0.0, 10), "[□□□□□□□□□□]");
    }

    #[test]
    fn progress_bar_hundred_percent() {
        assert_eq!(progress_bar(100.0, 10), "[■■■■■■■■■■]");
    }

    #[test]
    fn progress_bar_fifty_percent() {
        assert_eq!(progress_bar(50.0, 10), "[■■■■■□□□□□]");
    }

    #[test]
    fn progress_bar_over_hundred_clamps_to_full() {
        assert_eq!(progress_bar(150.0, 10), "[■■■■■■■■■■]");
    }

    #[test]
    fn progress_bar_negative_clamps_to_empty() {
        assert_eq!(progress_bar(-10.0, 10), "[□□□□□□□□□□]");
    }

    // ---------- provider_logo ----------

    #[test]
    fn provider_logo_maps_known_types() {
        for name in [
            "Claude Code",
            "Codex",
            "Zhipu",
            "Kimi Code",
            "Kimi API",
            "DeepSeek",
            "OpenAI",
            "Anthropic",
            "Qoder",
        ] {
            assert!(provider_logo(name).is_some(), "{name} 应有 logo");
        }
        assert!(provider_logo("Unknown").is_none());
    }

    // ---------- provider_menu_block ----------

    fn quota(name: &str, used: f64, reset_at: Option<&str>) -> crate::models::QuotaInfo {
        crate::models::QuotaInfo {
            name: name.into(),
            used,
            total: 100.0,
            current_value: None,
            remaining: Some(100.0 - used),
            remaining_percent: 100.0 - used,
            reset_time: None,
            plan_label: None,
            reset_at: reset_at.map(Into::into),
        }
    }

    fn status_full(
        name: &str,
        alias: &str,
        balance: Option<crate::models::BalanceData>,
        quotas: Option<Vec<crate::models::QuotaInfo>>,
        usage: Option<crate::models::UsageData>,
    ) -> ProviderStatus {
        ProviderStatus {
            name: name.into(),
            alias: alias.into(),
            enabled: true,
            balance,
            usage,
            error: None,
            quota_infos: quotas,
        }
    }

    fn fixed_now() -> chrono::DateTime<chrono::Utc> {
        chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 10, 31, 12, 0, 0).unwrap()
    }

    #[test]
    fn percent_multi_window_block_flattens_all_windows() {
        let b = crate::models::BalanceData {
            provider: "Claude Code".into(),
            available_balance: 62.0,
            voucher_balance: 0.0,
            cash_balance: 0.0,
            total_balance: 62.0,
            currency: "%".into(),
        };
        let quotas = vec![
            quota("5小时", 38.0, Some("2026-10-31T13:24:00Z")),
            quota("7天", 19.0, Some("2026-11-04T00:00:00Z")),
            quota("7天Opus", 5.0, None),
        ];
        let p = status_full("Claude Code", "Max工作", Some(b), Some(quotas), None);
        let block = provider_menu_block(&p, fixed_now());
        // 标题 = 身份徽章 + 别名 + 主窗口剩余；明细 = 全部窗口平铺（保持接口顺序）
        assert_eq!(block.title, "Max工作 — 62%");
        assert_eq!(block.details.len(), 3);
        assert_eq!(block.details[0], "  [■■■■□□□□□□] 5H · 62% · 1H24M");
        assert!(block.details[1].starts_with("  [■"));
        assert!(block.details[1].contains("7D · 81% · 3D12H"));
        assert!(block.details[2].contains("7D Opus · 95%"));
    }

    #[test]
    fn percent_without_quota_degrades_to_title_only() {
        let b = crate::models::BalanceData {
            provider: "Zhipu".into(),
            available_balance: 47.0,
            voucher_balance: 0.0,
            cash_balance: 0.0,
            total_balance: 47.0,
            currency: "%".into(),
        };
        let p = status_full("Zhipu", "智谱", Some(b), None, None);
        let block = provider_menu_block(&p, fixed_now());
        assert_eq!(block.title, "智谱 — 47%");
        assert!(block.details.is_empty());
    }

    #[test]
    fn zhipu_style_total_missing_derives_bar_from_remaining() {
        // 智谱 API 不返回 limit（total=0、used 为绝对量）：
        // 进度条须从 remaining_percent 反推已用（剩 35% → 已用 65% → ceil 7 格），
        // 不能算成 0% 全空条
        let b = crate::models::BalanceData {
            provider: "Zhipu".into(),
            available_balance: 35.0,
            voucher_balance: 0.0,
            cash_balance: 0.0,
            total_balance: 35.0,
            currency: "%".into(),
        };
        let quotas = vec![
            {
                let mut q = quota("5H", 12000.0, None);
                q.total = 0.0;
                q.remaining_percent = 35.0;
                q
            },
            {
                let mut q = quota("周", 60000.0, None);
                q.total = 0.0;
                q.remaining_percent = 35.0;
                q
            },
        ];
        let p = status_full("Zhipu", "智谱", Some(b), Some(quotas), None);
        let block = provider_menu_block(&p, fixed_now());
        assert_eq!(block.title, "智谱 — 35%");
        assert_eq!(block.details.len(), 2);
        assert_eq!(block.details[0], "  [■■■■■■■□□□] 5H · 35%");
        assert!(block.details[1].starts_with("  [■■■■■■■□□□] W"));
    }

    #[test]
    fn api_block_with_usage_gets_today_line() {
        let b = crate::models::BalanceData {
            provider: "DeepSeek".into(),
            available_balance: 88.88,
            voucher_balance: 10.0,
            cash_balance: 78.88,
            total_balance: 88.88,
            currency: "CNY".into(),
        };
        let u = crate::models::UsageData {
            provider: "DeepSeek".into(),
            total_tokens: 1_234_567,
            prompt_tokens: 1_000_000,
            completion_tokens: 234_567,
            today_cost: None,
            today_requests: Some(42),
        };
        let p = status_full("DeepSeek", "DeepSeek", Some(b), None, Some(u));
        let block = provider_menu_block(&p, fixed_now());
        assert_eq!(block.title, "DeepSeek — ¥88.88");
        assert_eq!(block.details, vec!["  今日 42次 · 1.2M tok"]);
    }

    #[test]
    fn error_and_no_data_blocks() {
        let mut p = status_full("Kimi", "Kimi", None, None, None);
        p.error = Some("timeout".into());
        // 获取失败：无 balance → 黄档语义保留为行尾 ⚠️；身份徽章未知类型为 ⚪
        assert_eq!(
            provider_menu_block(&p, fixed_now()).title,
            "Kimi — 获取失败 ⚠️"
        );
        let p2 = status_full("X", "X", None, None, None);
        assert_eq!(provider_menu_block(&p2, fixed_now()).title, "X — 无数据");
    }

    #[test]
    fn alert_suffix_follows_primary_window_level() {
        // balance 62% 平静，但主窗口（7天Opus 剩 3%，<5 黄档）才是显示值：
        // 告警后缀须按主窗口判级为 ⚠️，不能再用 balance 判成无告警（与 ⚠️ 同源）
        let b = crate::models::BalanceData {
            provider: "Claude Code".into(),
            available_balance: 62.0,
            voucher_balance: 0.0,
            cash_balance: 0.0,
            total_balance: 62.0,
            currency: "%".into(),
        };
        let quotas = vec![quota("5小时", 38.0, None), quota("7天Opus", 97.0, None)];
        let p = status_full("Claude Code", "Max工作", Some(b), Some(quotas), None);
        assert_eq!(
            provider_menu_block(&p, fixed_now()).title,
            "Max工作 — 3% ⚠️"
        );
    }

    #[test]
    fn signature_includes_quota_names_in_order() {
        let p = status_full(
            "CC",
            "a",
            None,
            Some(vec![quota("5小时", 38.0, None), quota("7天", 19.0, None)]),
            None,
        );
        let s = UsageSummary {
            providers: vec![p],
            ..Default::default()
        };
        assert_eq!(provider_signature(&s), "CC|a|5小时,7天");
        // 无 quota 的供应商签名为空尾巴（与旧行为兼容段）
        let p2 = status_full("DS", "b", None, None, None);
        let s2 = UsageSummary {
            providers: vec![p2],
            ..Default::default()
        };
        assert_eq!(provider_signature(&s2), "DS|b|");
    }

    #[test]
    fn tokens_formatting() {
        assert_eq!(format_tokens(980), "980");
        assert_eq!(format_tokens(9_800), "9.8k");
        assert_eq!(format_tokens(1_234_567), "1.2M");
        assert_eq!(format_tokens(2_500_000_000), "2.5B");
    }
}
