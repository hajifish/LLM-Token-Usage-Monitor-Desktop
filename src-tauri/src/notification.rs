//! 低余额主动系统通知：告警等级恶化时从 Rust 侧弹系统通知（不依赖 webview 打开）。
//!
//! 判定模型（纯函数 [`transition_notification`] / [`detect_alert_transitions`]）：
//! 每个供应商在内存基线表里记录上一轮告警等级（0=正常 / 1=紧张 / 2=耗尽，语义与
//! [`crate::tray::quota_warning_level`] 一致），仅等级恶化（正常→紧张、正常→耗尽、
//! 紧张→耗尽）时发通知；恢复到正常后基线归 0，再次恶化自然重新武装；等级不变或
//! 好转不发。应用启动后基线表为空，首轮只记基线不发 —— 避免每次重启对存量告警
//! 重复轰炸。基线仅存内存、不持久化：重启后不对存量告警补发，也免去为旧状态
//! 做持久化与跨版本迁移的负担。
//!
//! 无数据轮（balance 缺失：禁用 / 获取失败）不参与判定也不更新基线，
//! 避免网络抖动造成「假恢复 → 再恶化」的重复提醒。

use crate::models::{ProviderStatus, UsageSummary};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

/// macOS 播放系统默认通知音的名字。特判发生在链路末端：tauri-plugin-notification
/// → notify-rust **默认**（未启用 preview-macos-un feature）NSUserNotifications 后端
/// → mac-notification-sys 的 objc 桥对该字符串特判映射为
/// NSUserNotificationDefaultSoundName——上游若改走其他后端 feature，该特判会悄悄消失。
/// Windows 上该名字解析失败被 notify-rust 静默忽略（`Sound::from_str(..).ok()`）；
/// Linux（D-Bus）后端不读取音名、直接忽略，无副作用
const DEFAULT_SOUND: &str = "NSUserNotificationDefaultSoundName";

/// 单供应商迁移判定（纯函数）：prev 为 None 表示无基线（首轮 / 新出现的供应商）。
/// 仅等级恶化返回本轮等级；首轮只记基线；等级不变或好转不发。
pub fn transition_notification(prev: Option<u8>, current: u8) -> Option<u8> {
    match prev {
        None => None,
        Some(prev) if current > prev => Some(current),
        _ => None,
    }
}

/// 通知文案（纯函数，按等级分级）：返回 (标题, 正文)。
/// 1=紧张（黄）用 ⚠️，2=耗尽（红）用 🚨——与托盘 ⚠️ 前缀同风格，耗尽档用 🚨 加强
/// （托盘不分级，红黄区分靠图标圆点）。
///
/// 注意：对 level 的 match 以 `_` 兜底为紧张档——未来新增等级 3 时会**静默**按
/// 紧张档渲染而非编译期暴露，届时需与 tray.rs 的显示前缀人工联动调整。
pub(crate) fn notification_text(level: u8, display_name: &str, progress: &str) -> (String, String) {
    match level {
        2 => (
            format!("🚨 {display_name} 余量已耗尽"),
            format!("剩余 {progress}，请及时充值"),
        ),
        _ => (
            format!("⚠️ {display_name} 余量不足"),
            format!("剩余 {progress}，请留意用量"),
        ),
    }
}

/// 进度文案（与菜单状态条同源）：percent 型取主窗口剩余%（无 quota 时
/// [`crate::tray::aggregate_value`] 回退 balance），API 型按币种余额。
/// 「无数据为 –」分支在通知链路不可达——balance 为 None 的供应商在
/// [`detect_alert_transitions`] 已被 continue 跳过；该兜底仅供托盘侧与测试复用。
pub(crate) fn progress_text(p: &ProviderStatus) -> String {
    match &p.balance {
        Some(b) if b.is_percent() => format!("{:.0}%", crate::tray::aggregate_value(p)),
        _ => crate::tray::tray_value(p),
    }
}

/// 供应商判定键：name + alias（scheduler 的 alias 已回退统一显示名，如 Zhipu→智谱）。
/// 与前端 providerKey **并非同口径**：本键 = name|display_name 回退且不 trim，前端键 =
/// name|name 回退且 trim，两套键空间互不相交、各自独立演化（本表仅基线自用，前端键
/// 只做配置行定位）；口径真正相近的是 tray::provider_show_in_tray（本侧仍差一个 trim）。
/// 同类型多账号须 alias 唯一：不填别名时两账号产生同一键 → 碰撞，仅首个出现的
/// 供应商参与判定，后续同键跳过（不发事件、不覆盖基线），与前端「targets !== 1
/// 即拒绝执行」的保守取向一致。
fn provider_key(p: &ProviderStatus) -> String {
    format!("{}|{}", p.name, p.alias)
}

/// 一轮轮询产生的待发通知
pub(crate) struct AlertEvent {
    pub level: u8,
    pub display_name: String,
    pub progress: String,
}

/// 迁移检测（纯函数）：上轮基线 + 本轮 summary → (待发通知列表, 新基线表)。
/// balance 缺失（禁用 / 获取失败 / 无数据）的供应商跳过判定且不更新基线。
///
/// enabled 作为签名参数是刻意设计（非胶水层细节）：enabled = false 时待发事件
/// 过滤为空、基线照常刷新——「关开关期间仍记基线、重开不对存量告警补发」这一
/// 核心不变量由此成为纯函数契约，可直接单测，防止「提前退出上提」类重构悄悄破坏。
pub(crate) fn detect_alert_transitions(
    prev: &HashMap<String, u8>,
    summary: &UsageSummary,
    enabled: bool,
) -> (Vec<AlertEvent>, HashMap<String, u8>) {
    let mut next = prev.clone();
    let mut events = Vec::new();
    // 同类型多账号不填别名时 provider_key 碰撞：仅首个参与判定，后续同键跳过
    // （否则后遍历者覆盖前者基线，前者耗尽告警被永久吞掉）
    let mut seen = HashSet::new();
    for p in &summary.providers {
        if p.balance.is_none() {
            continue;
        }
        let level = crate::tray::quota_warning_level(p);
        let key = provider_key(p);
        if !seen.insert(key.clone()) {
            continue;
        }
        let event = transition_notification(prev.get(&key).copied(), level).map(|level| {
            let display_name = p.alias.clone();
            let progress = progress_text(p);
            AlertEvent {
                level,
                display_name,
                progress,
            }
        });
        next.insert(key, level);
        events.extend(event);
    }
    // 关开关期间事件过滤为空、基线照记（见函数文档）：重开开关后同等级不补发
    let pending = if enabled { events } else { Vec::new() };
    (pending, next)
}

/// 各供应商上一轮告警等级基线（内存态；重启归零 → 启动首轮只记基线不发）。
#[derive(Default)]
pub struct AlertBaseline(Mutex<HashMap<String, u8>>);

/// 轮询收口点：更新基线并按需发送系统通知。enabled 门控下沉在
/// [`detect_alert_transitions`] 内（关开关期间基线照记、事件过滤为空），
/// 避免重新打开开关后对存量告警补发旧状态。
pub fn on_summary_refreshed(app: &AppHandle, enabled: bool, summary: &UsageSummary) {
    // 基线状态首次使用时登记（参照 tray::create_update_item 的 manage-on-demand 模式）
    let baseline = match app.try_state::<AlertBaseline>() {
        Some(slot) => slot,
        None => {
            app.manage(AlertBaseline::default());
            app.state::<AlertBaseline>()
        }
    };
    let events = {
        let mut inner = baseline.0.lock().unwrap_or_else(|e| e.into_inner());
        let (events, next) = detect_alert_transitions(&inner, summary, enabled);
        *inner = next;
        events
    };
    for event in &events {
        send(app, event);
    }
}

/// 发送单条系统通知（macOS 上 app 未打包时由插件归属到 com.apple.Terminal，
/// 打包后归属应用自身；不打断轮询）。
///
/// 失败语义（如实）：tauri-plugin-notification 2.4.0 桌面端 show() 把真实投递丢进
/// spawn 任务吞掉并无条件返回 Ok，下方 Err 分支不可达、投递失败与权限拒绝均不可检测，
/// 通知会静默消失。「发送失败即丢一条（基线不回滚，等下次等级变化再发）」是有意权衡：
/// 不回滚基线可避免每轮对同一告警重复轰炸；用户可在设置页「发送测试通知」自证链路。
fn send(app: &AppHandle, event: &AlertEvent) {
    // 投递结果不可检测（见上），至少让「尝试过发送」可追溯：
    // 事件只在等级恶化迁移时产生（每轮每供应商至多一条），无日志风暴风险
    log::info!(
        "发送低余额通知：{}（等级 {}）",
        event.display_name,
        event.level
    );
    let (title, body) = notification_text(event.level, &event.display_name, &event.progress);
    if let Err(e) = app
        .notification()
        .builder()
        .title(title)
        .body(body)
        .sound(DEFAULT_SOUND)
        .show()
    {
        // 死代码：桌面端插件无条件返回 Ok；保留分支仅防御上游后端行为变化
        log::warn!("系统通知发送失败: {}", e);
    }
}

/// 发送一条固定文案的测试通知（设置页「发送测试通知」按钮入口）。
///
/// 桌面端 API 无法检测投递失败与权限拒绝（见 send 的失败语义），用户在设置页
/// 点按钮自证链路是否畅通是唯一可靠反馈；发送成功仅代表调用链路走通，
/// 能否真正收到取决于系统通知权限。
pub(crate) fn send_test_notification(app: &AppHandle) {
    log::info!("发送测试通知（用户自证通知链路）");
    if let Err(e) = app
        .notification()
        .builder()
        .title("🔔 测试通知")
        .body("低余额通知链路正常；若未收到本条，请检查系统通知权限。")
        .sound(DEFAULT_SOUND)
        .show()
    {
        // 死代码：桌面端插件无条件返回 Ok；保留分支仅防御上游后端行为变化
        log::warn!("测试通知发送失败: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{BalanceData, QuotaInfo};

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

    fn status(name: &str, alias: &str, balance: Option<BalanceData>) -> ProviderStatus {
        ProviderStatus {
            name: name.to_string(),
            alias: alias.to_string(),
            enabled: true,
            balance,
            usage: None,
            error: None,
            quota_infos: None,
        }
    }

    fn quota(name: &str, remaining: f64) -> QuotaInfo {
        QuotaInfo {
            name: name.into(),
            used: 100.0 - remaining,
            total: 100.0,
            current_value: None,
            remaining: Some(remaining),
            remaining_percent: remaining,
            reset_time: None,
            plan_label: None,
            reset_at: None,
        }
    }

    fn summary(providers: Vec<ProviderStatus>) -> UsageSummary {
        UsageSummary {
            providers,
            ..Default::default()
        }
    }

    // ---------- transition_notification：迁移判定规则 ----------

    #[test]
    fn first_round_records_baseline_without_notifying() {
        // 首轮无基线：无论本轮什么等级都只记基线不发（避免每次重启轰炸）
        for current in 0..=2 {
            assert_eq!(
                transition_notification(None, current),
                None,
                "首轮等级 {} 不应发通知",
                current
            );
        }
    }

    #[test]
    fn only_escalation_notifies() {
        // 仅三级恶化发通知：正常→紧张、正常→耗尽、紧张→耗尽
        assert_eq!(transition_notification(Some(0), 1), Some(1));
        assert_eq!(transition_notification(Some(0), 2), Some(2));
        assert_eq!(transition_notification(Some(1), 2), Some(2));
    }

    #[test]
    fn unchanged_or_improving_stays_silent() {
        // 等级不变或好转不发
        assert_eq!(transition_notification(Some(0), 0), None);
        assert_eq!(transition_notification(Some(1), 1), None);
        assert_eq!(transition_notification(Some(2), 2), None);
        assert_eq!(transition_notification(Some(1), 0), None);
        assert_eq!(transition_notification(Some(2), 1), None);
        assert_eq!(transition_notification(Some(2), 0), None);
    }

    // ---------- detect_alert_transitions：整体检测与基线维护 ----------

    #[test]
    fn detects_escalation_per_provider() {
        // 两个供应商独立判定：一个恶化、一个平静 → 只有一个事件
        let prev = HashMap::from([
            ("DeepSeek|工作号".to_string(), 0u8),
            ("Zhipu|智谱".to_string(), 0u8),
        ]);
        let s = summary(vec![
            status("DeepSeek", "工作号", Some(balance(5.0, "CNY"))), // 0→1 恶化
            status("Zhipu", "智谱", Some(balance(88.0, "%"))),       // 0→0 平静
        ]);
        let (events, next) = detect_alert_transitions(&prev, &s, true);
        assert_eq!(events.len(), 1, "仅恶化的供应商应产生通知");
        assert_eq!(events[0].level, 1);
        assert_eq!(events[0].display_name, "工作号");
        assert_eq!(events[0].progress, "¥5.00");
        // 基线整体刷新为本轮等级
        assert_eq!(next.get("DeepSeek|工作号"), Some(&1u8));
        assert_eq!(next.get("Zhipu|智谱"), Some(&0u8));
    }

    #[test]
    fn same_type_accounts_judged_independently() {
        // 同类型两个账号：一个已耗尽（2）、另一个紧张（1），各自恶化各自发
        let prev = HashMap::from([
            ("DeepSeek|工作号".to_string(), 1u8),
            ("DeepSeek|私人号".to_string(), 0u8),
        ]);
        let s = summary(vec![
            status("DeepSeek", "工作号", Some(balance(0.0, "CNY"))), // 1→2 恶化
            status("DeepSeek", "私人号", Some(balance(3.0, "CNY"))), // 0→1 恶化
        ]);
        let (events, next) = detect_alert_transitions(&prev, &s, true);
        assert_eq!(events.len(), 2);
        assert_eq!(next.get("DeepSeek|工作号"), Some(&2u8));
        assert_eq!(next.get("DeepSeek|私人号"), Some(&1u8));
    }

    #[test]
    fn duplicate_provider_keys_only_first_is_judged() {
        // 同类型两账号均不填别名 → provider_key 碰撞：仅首个参与判定
        // （后续同键跳过：不发事件、不覆盖基线，基线取首个账号）
        let prev = HashMap::from([("DeepSeek|DeepSeek".to_string(), 0u8)]);
        let s = summary(vec![
            status("DeepSeek", "DeepSeek", Some(balance(50.0, "CNY"))), // 首个：正常
            status("DeepSeek", "DeepSeek", Some(balance(0.0, "CNY"))),  // 同键：耗尽，被跳过
        ]);
        let (events, next) = detect_alert_transitions(&prev, &s, true);
        assert!(
            events.is_empty(),
            "同键碰撞时后续账号不应产生通知（否则基线被互覆、告警被吞）"
        );
        assert_eq!(
            next.get("DeepSeek|DeepSeek"),
            Some(&0u8),
            "基线应取首个账号"
        );
    }

    #[test]
    fn duplicate_key_holder_escalates_next_round() {
        // 碰撞轮基线归首个账号持有：下轮首个恶化 → 正常发出
        let prev = HashMap::from([("DeepSeek|DeepSeek".to_string(), 0u8)]);
        let s = summary(vec![
            status("DeepSeek", "DeepSeek", Some(balance(50.0, "CNY"))),
            status("DeepSeek", "DeepSeek", Some(balance(0.0, "CNY"))),
        ]);
        let (_, next) = detect_alert_transitions(&prev, &s, true);
        let worsened = summary(vec![status(
            "DeepSeek",
            "DeepSeek",
            Some(balance(5.0, "CNY")),
        )]);
        let (events, _) = detect_alert_transitions(&next, &worsened, true);
        assert_eq!(events.len(), 1, "键持有者下轮恶化应正常发出");
        assert_eq!(events[0].level, 1);
    }

    #[test]
    fn first_round_only_sets_baseline() {
        // 启动首轮：基线为空 → 全部只记基线，不发通知
        let s = summary(vec![status(
            "DeepSeek",
            "DeepSeek",
            Some(balance(0.0, "CNY")),
        )]);
        let (events, next) = detect_alert_transitions(&HashMap::new(), &s, true);
        assert!(events.is_empty(), "首轮不应发通知");
        assert_eq!(next.get("DeepSeek|DeepSeek"), Some(&2u8));
    }

    #[test]
    fn recovery_rearms_next_escalation() {
        // 恢复到正常后基线归 0：再次恶化重新武装（重新发通知）
        let prev = HashMap::from([("DeepSeek|DeepSeek".to_string(), 1u8)]);
        let recovered = summary(vec![status(
            "DeepSeek",
            "DeepSeek",
            Some(balance(50.0, "CNY")),
        )]);
        let (events, next) = detect_alert_transitions(&prev, &recovered, true);
        assert!(events.is_empty(), "好转不发");
        assert_eq!(next.get("DeepSeek|DeepSeek"), Some(&0u8));
        // 恢复后再恶化 → 重新发
        let worsened = summary(vec![status(
            "DeepSeek",
            "DeepSeek",
            Some(balance(0.0, "CNY")),
        )]);
        let (events, _) = detect_alert_transitions(&next, &worsened, true);
        assert_eq!(events.len(), 1, "恢复后再次恶化应重新发通知");
        assert_eq!(events[0].level, 2);
    }

    #[test]
    fn no_data_round_freezes_baseline() {
        // balance 缺失（禁用 / 获取失败）：不判定也不更新基线，避免「假恢复 → 再恶化」重复轰炸
        let prev = HashMap::from([("DeepSeek|DeepSeek".to_string(), 1u8)]);
        let mut errored = status("DeepSeek", "DeepSeek", None);
        errored.error = Some("timeout".into());
        let s = summary(vec![errored]);
        let (events, next) = detect_alert_transitions(&prev, &s, true);
        assert!(events.is_empty());
        assert_eq!(
            next.get("DeepSeek|DeepSeek"),
            Some(&1u8),
            "无数据轮基线应保持不变"
        );
    }

    // ---------- enabled 门控：关开关期间基线照记、重开不补发 ----------

    #[test]
    fn disabled_round_filters_events_but_refreshes_baseline() {
        // 关开关期间 0→2 恶化：无事件但基线照常刷新为 2——
        // 若基线不记，重开开关那轮会把存量告警判为恶化 → 补发轰炸
        let prev = HashMap::from([("DeepSeek|DeepSeek".to_string(), 0u8)]);
        let s = summary(vec![status(
            "DeepSeek",
            "DeepSeek",
            Some(balance(0.0, "CNY")),
        )]);
        let (events, next) = detect_alert_transitions(&prev, &s, false);
        assert!(events.is_empty(), "关开关期间不应产生待发事件");
        assert_eq!(
            next.get("DeepSeek|DeepSeek"),
            Some(&2u8),
            "基线应照常刷新（不变量：enabled 门控只过滤事件，不冻结基线）"
        );
    }

    #[test]
    fn reenabling_at_same_level_does_not_replay() {
        // 重开开关后同等级：基线已在上轮（关开关期间）刷新 → 不补发旧状态
        let prev = HashMap::from([("DeepSeek|DeepSeek".to_string(), 0u8)]);
        let s = summary(vec![status(
            "DeepSeek",
            "DeepSeek",
            Some(balance(0.0, "CNY")),
        )]);
        let (_, next) = detect_alert_transitions(&prev, &s, false);
        let (events, _) = detect_alert_transitions(&next, &s, true);
        assert!(events.is_empty(), "重开开关后同等级不应补发通知");
    }

    // ---------- progress_text：进度文案与状态条同源 ----------

    #[test]
    fn progress_text_percent_uses_primary_window() {
        // percent 型：显示值取主窗口剩余%（balance 62% 平静但 7天Opus 剩 3% 才是显示值）
        let mut p = status("Claude Code", "CC", Some(balance(62.0, "%")));
        p.quota_infos = Some(vec![quota("5小时", 62.0), quota("7天Opus", 3.0)]);
        assert_eq!(progress_text(&p), "3%");
    }

    #[test]
    fn progress_text_formats_by_currency() {
        assert_eq!(
            progress_text(&status("DeepSeek", "DS", Some(balance(5.0, "CNY")))),
            "¥5.00"
        );
        assert_eq!(
            progress_text(&status("OpenAI", "OA", Some(balance(12.34, "USD")))),
            "$12.34"
        );
        assert_eq!(
            progress_text(&status("Zhipu", "智谱", Some(balance(4.4, "%")))),
            "4%"
        );
        assert_eq!(progress_text(&status("X", "X", None)), "–");
    }

    // ---------- notification_text：分级文案 ----------

    #[test]
    fn notification_text_distinguishes_levels() {
        let (warn_title, warn_body) = notification_text(1, "工作号", "¥5.00");
        assert_eq!(warn_title, "⚠️ 工作号 余量不足");
        assert_eq!(warn_body, "剩余 ¥5.00，请留意用量");
        let (crit_title, crit_body) = notification_text(2, "工作号", "¥0.00");
        assert_eq!(crit_title, "🚨 工作号 余量已耗尽");
        assert_eq!(crit_body, "剩余 ¥0.00，请及时充值");
    }
}
