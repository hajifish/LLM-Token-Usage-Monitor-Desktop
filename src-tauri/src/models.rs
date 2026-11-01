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
            refresh_interval: 60,
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
