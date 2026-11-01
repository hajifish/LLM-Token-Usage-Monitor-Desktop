use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceData {
    pub provider: String,
    pub available_balance: f64,
    pub voucher_balance: f64,
    pub cash_balance: f64,
    pub total_balance: f64,
    pub currency: String,
}

impl BalanceData {
    /// 配额百分比型供应商（currency = "%"）
    pub fn is_percent(&self) -> bool {
        self.currency == "%"
    }
    /// 美元成本型供应商（currency = "USD"，如 OpenAI / Anthropic）
    pub fn is_usd(&self) -> bool {
        self.currency == "USD"
    }
    /// 人民币余额型供应商（currency = "CNY"，如 DeepSeek / Kimi），用于 ¥ 总额正向白名单
    pub fn is_cny(&self) -> bool {
        self.currency == "CNY"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageData {
    pub provider: String,
    pub total_tokens: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub today_cost: Option<f64>,
    pub today_requests: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub api_key: String,
    pub enabled: bool,
    #[serde(default)]
    pub platform_token: Option<String>,
    /// 用户自定义别名（用于区分同一服务商的多个账号）；为空时回退到 name
    #[serde(default)]
    pub alias: Option<String>,
    /// 状态栏常显开关：是否在菜单栏托盘文字中展示该服务商的主指标
    #[serde(default = "default_show_in_tray")]
    pub show_in_tray: bool,
}

fn default_show_in_tray() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub providers: Vec<ProviderConfig>,
    /// 刷新间隔（秒）；旧配置文件缺此字段时由 serde default 兜底为 60
    #[serde(default = "default_refresh_interval")]
    pub refresh_interval: u64,
    /// 是否自动检查更新（启动 60 秒后首查，之后每小时一次）
    #[serde(default = "default_auto_check_update")]
    pub auto_check_update: bool,
    /// 托盘文字总开关：关闭后菜单栏仅显示图标
    #[serde(default = "default_tray_text_enabled")]
    pub tray_text_enabled: bool,
    /// 低余额主动系统通知：告警等级恶化时弹系统通知
    #[serde(default = "default_notifications_enabled")]
    pub notifications_enabled: bool,
}

fn default_refresh_interval() -> u64 {
    60
}

fn default_auto_check_update() -> bool {
    true
}

fn default_tray_text_enabled() -> bool {
    true
}

fn default_notifications_enabled() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            providers: Vec::new(),
            refresh_interval: default_refresh_interval(),
            auto_check_update: default_auto_check_update(),
            tray_text_enabled: default_tray_text_enabled(),
            notifications_enabled: default_notifications_enabled(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaInfo {
    /// 限制类型名称（如 "每5小时限额"、"每周限额"）
    pub name: String,
    /// 已使用量
    pub used: f64,
    /// 总量
    pub total: f64,
    /// 当前值（剩余可用）
    pub current_value: Option<f64>,
    /// 剩余量
    pub remaining: Option<f64>,
    /// 剩余百分比
    pub remaining_percent: f64,
    /// 重置时间
    pub reset_time: Option<String>,
    /// 重置时刻（ISO 8601 / RFC3339 UTC），供展示层精确换算倒计时；
    /// 缺失时回退 reset_time 的本地化文本。向后兼容旧数据（serde default）
    #[serde(default)]
    pub reset_at: Option<String>,
    /// 配额类型标签（如 "coding plan"）
    pub plan_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub name: String,
    /// 展示用别名（alias 或回退到 name）
    pub alias: String,
    pub enabled: bool,
    pub balance: Option<BalanceData>,
    pub usage: Option<UsageData>,
    pub error: Option<String>,
    pub quota_infos: Option<Vec<QuotaInfo>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageSummary {
    pub providers: Vec<ProviderStatus>,
    pub total_balance: f64,
    pub last_updated: String,
}

/// 供应商配置行的身份键：name + trim 后别名（空回退统一显示名）。
///
/// 与 `tray::provider_show_in_tray` 的匹配口径一致（含空别名回退 display_name、
/// 两侧 trim）：低余额通知的 provider_key 判定同口径。键撞车时这两处都只能
/// 命中首个账号——第二个账号的托盘开关与告警静默失效，故配置落盘前由
/// [`validate_provider_identities`] 保证该键全局唯一。
pub(crate) fn provider_identity(c: &ProviderConfig) -> (String, String) {
    let alias = c
        .alias
        .as_deref()
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| crate::providers::display_name(&c.name));
    (c.name.clone(), alias.to_string())
}

/// 校验供应商身份键唯一：重复即拒绝。落盘咽喉（commands::persist_config）调用，
/// 错误文案直接面向设置页 toast，须指引用户填写别名。
pub(crate) fn validate_provider_identities(config: &AppConfig) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for c in &config.providers {
        if !seen.insert(provider_identity(c)) {
            return Err(format!(
                "「{}」存在多个同名账号且未用别名区分，请为每个账号填写不同别名后再保存",
                crate::providers::display_name(&c.name)
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod quota_reset_at_tests {
    use super::*;

    #[test]
    fn reset_at_defaults_none_for_legacy_json() {
        // 旧持久化数据无 reset_at 字段 → serde default 兼容
        let json = r#"{"name":"5小时","used":38.0,"total":100.0,
            "current_value":null,"remaining":62.0,"remaining_percent":62.0,
            "reset_time":"1小时后重置","plan_label":null}"#;
        let q: QuotaInfo = serde_json::from_str(json).unwrap();
        assert!(q.reset_at.is_none());
    }
}

#[cfg(test)]
mod notifications_default_tests {
    use super::*;

    #[test]
    fn notifications_enabled_defaults_true_for_legacy_config() {
        // 老配置文件无 notifications_enabled 字段 → serde default 兜底为 true，
        // 缺字段必须仍能加载（存量用户升级后通知默认开启）
        let json = r#"{"providers":[],"refresh_interval":60}"#;
        let cfg: AppConfig = serde_json::from_str(json).unwrap();
        assert!(
            cfg.notifications_enabled,
            "缺省的 notifications_enabled 应默认为 true"
        );
    }
}

#[cfg(test)]
mod provider_identity_tests {
    use super::*;

    fn provider(name: &str, alias: Option<&str>) -> ProviderConfig {
        ProviderConfig {
            name: name.to_string(),
            api_key: String::new(),
            enabled: true,
            platform_token: None,
            alias: alias.map(str::to_string),
            show_in_tray: true,
        }
    }

    fn config(providers: Vec<ProviderConfig>) -> AppConfig {
        AppConfig {
            providers,
            ..Default::default()
        }
    }

    #[test]
    fn duplicate_type_without_alias_rejected() {
        // 两个 DeepSeek 都不填别名 → 身份键撞车，保存必须被拒绝
        let cfg = config(vec![provider("DeepSeek", None), provider("DeepSeek", None)]);
        let err = validate_provider_identities(&cfg).unwrap_err();
        assert!(err.contains("别名"), "错误文案应指引用户填写别名: {err}");
    }

    #[test]
    fn duplicate_type_distinct_alias_allowed() {
        // 填了不同别名即为合法多账号配置
        let cfg = config(vec![
            provider("DeepSeek", Some("个人")),
            provider("DeepSeek", Some("公司")),
        ]);
        assert!(validate_provider_identities(&cfg).is_ok());
    }

    #[test]
    fn alias_matching_default_display_name_still_collides() {
        // 一个显式填「智谱」（= Zhipu 默认显示名）、一个留空：运行时两侧匹配键
        // 同为「智谱」（tray::provider_show_in_tray 口径），仍属撞车
        let cfg = config(vec![
            provider("Zhipu", Some("智谱")),
            provider("Zhipu", None),
        ]);
        assert!(validate_provider_identities(&cfg).is_err());
    }

    #[test]
    fn whitespace_only_alias_treated_as_empty() {
        // 纯空白别名按空处理（与匹配键的 trim 语义一致），与留空账号仍撞车
        let cfg = config(vec![
            provider("DeepSeek", Some("   ")),
            provider("DeepSeek", None),
        ]);
        assert!(validate_provider_identities(&cfg).is_err());
    }

    #[test]
    fn same_alias_different_types_allowed() {
        // 身份键含 name：不同类型账号共用同一别名不冲突
        let cfg = config(vec![
            provider("DeepSeek", Some("主号")),
            provider("Kimi API", Some("主号")),
        ]);
        assert!(validate_provider_identities(&cfg).is_ok());
    }
}
