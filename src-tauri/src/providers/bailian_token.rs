//! 阿里云百炼 Token Plan 订阅配额供应商（免密 / 读本机凭据型）。
//!
//! Token Plan 是百炼三套互不相通系统之一（按量计费 / Coding Plan / Token Plan），
//! 按 Credits 折算 token 计额，窗口为订阅月等（Essential 等档位）。
//! bl 只暴露「已用占比」与重置时刻，不暴露绝对额度，故窗口以百分比表示
//! （used=已用%、total=100），同 Codex 的百分比型配额口径。
//!
//! 通道同 Coding Plan：shell 调 `bl usage token-plan --output json`（控制台登录态）。
//! 归一化输出（见 bailian-cli-commands 的 FT/IT）：
//!   { "per<N><Hour|Day|Week|Month>Percentage": 已用占比(0-1),
//!     "per<N><...>ResetTime": unix 毫秒, "hasQuotaData": bool }
//! 无订阅时 { "hasQuotaData": false }。窗口按跨度升序（bl 同序）。

use super::{format_iso_reset, LlmProvider, ProviderError};
use crate::models::{BalanceData, QuotaInfo};
use async_trait::async_trait;
use serde_json::Value;

pub struct BailianTokenProvider {
    cached_response: tokio::sync::OnceCell<Vec<TokenWindow>>,
}

/// 解析后的单个配额窗口（已用百分比 + RFC3339 重置时刻）。
#[derive(Clone, Debug)]
struct TokenWindow {
    name: String,
    used_percent: f64,
    reset_at: Option<String>,
}

impl TokenWindow {
    fn remaining_percent(&self) -> f64 {
        (100.0 - self.used_percent).clamp(0.0, 100.0)
    }
}

impl BailianTokenProvider {
    pub fn new() -> Self {
        Self {
            cached_response: tokio::sync::OnceCell::new(),
        }
    }

    /// 通过 OnceCell 缓存只调一次 bl，同一轮调度内 fetch_balance 与 fetch_quota_infos 共享结果。
    async fn fetch_windows(&self) -> Result<Vec<TokenWindow>, ProviderError> {
        if let Some(cached) = self.cached_response.get() {
            return Ok(cached.clone());
        }
        let json = super::bailian_cli::usage_json("token-plan").await?;
        let windows = parse_bl_output(&json)?;
        let _ = self.cached_response.set(windows.clone());
        Ok(windows)
    }
}

#[async_trait]
impl LlmProvider for BailianTokenProvider {
    fn name(&self) -> &str {
        "Bailian Token"
    }

    async fn fetch_balance(&self) -> Result<BalanceData, ProviderError> {
        let windows = self.fetch_windows().await?;
        // 主窗口：跨度最短优先（windows 已按跨度升序），同 Coding Plan 口径
        let remaining = windows
            .first()
            .map(|w| w.remaining_percent())
            .unwrap_or(0.0);
        Ok(BalanceData {
            provider: "Bailian Token".to_string(),
            available_balance: remaining,
            voucher_balance: 0.0,
            cash_balance: 0.0,
            total_balance: remaining,
            currency: "%".to_string(),
        })
    }

    async fn fetch_quota_infos(&self) -> Result<Option<Vec<QuotaInfo>>, ProviderError> {
        let windows = self.fetch_windows().await?;
        let quotas: Vec<QuotaInfo> = windows
            .into_iter()
            .map(|w| {
                let remaining = w.remaining_percent();
                QuotaInfo {
                    name: w.name.clone(),
                    used: w.used_percent,
                    total: 100.0,
                    current_value: None,
                    remaining: Some(remaining),
                    remaining_percent: remaining,
                    reset_time: w.reset_at.as_ref().map(|s| format_iso_reset(s)),
                    reset_at: w.reset_at.clone(),
                    plan_label: Some("token plan".to_string()),
                }
            })
            .collect();
        if quotas.is_empty() {
            Ok(None)
        } else {
            Ok(Some(quotas))
        }
    }
}

// --- 纯解析层（单测优先） ---

/// 拆 `per<N><Unit>` 基名 → (跨度小时数, 窗口中文名)；非法键返回 None。
fn window_key(base: &str) -> Option<(f64, String)> {
    let rest = base.strip_prefix("per")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let count: f64 = digits.parse().ok()?;
    let (unit_hours, unit_cn) = match &rest[digits.len()..] {
        "Hour" => (1.0, "小时"),
        "Day" => (24.0, "天"),
        "Week" => (168.0, "周"),
        "Month" => (720.0, "月"),
        _ => return None,
    };
    let name = if count == 1.0 && unit_cn == "月" {
        "订阅月".to_string()
    } else if count == 1.0 {
        format!("每{}", unit_cn)
    } else {
        format!("{}{}", count, unit_cn)
    };
    Some((count * unit_hours, name))
}

fn parse_bl_output(json: &Value) -> Result<Vec<TokenWindow>, ProviderError> {
    let obj = match json.as_object() {
        Some(o) => o,
        None => {
            return Err(ProviderError(
                "未找到有效的 Token Plan 订阅（bl 返回空配额）".to_string(),
            ))
        }
    };
    let mut windows: Vec<(f64, TokenWindow)> = Vec::new();
    for (key, value) in obj {
        let base = match key.strip_suffix("Percentage") {
            Some(b) => b,
            None => continue,
        };
        let (hours, name) = match window_key(base) {
            Some(w) => w,
            None => continue,
        };
        let fraction = match value.as_f64() {
            Some(f) if f.is_finite() && f >= 0.0 => f,
            _ => continue,
        };
        // bl 的 ResetTime 为 unix 毫秒（coding-plan 为秒，两者不同）
        let reset_at = obj
            .get(format!("{}ResetTime", base).as_str())
            .and_then(|v| v.as_f64())
            .filter(|r| r.is_finite() && *r > 1e12)
            .and_then(|r| chrono::DateTime::from_timestamp((r / 1000.0) as i64, 0))
            .map(|dt| dt.to_rfc3339());
        windows.push((
            hours,
            TokenWindow {
                name,
                used_percent: (fraction * 100.0).clamp(0.0, 100.0),
                reset_at,
            },
        ));
    }

    if windows.is_empty() {
        return Err(ProviderError(
            "未找到有效的 Token Plan 订阅（bl 返回空配额）".to_string(),
        ));
    }
    windows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    Ok(windows.into_iter().map(|(_, w)| w).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_live_monthly_shape() {
        let body = json!({
            "per1MonthPercentage": 0.0007932633333333333,
            "per1MonthResetTime": 1798214400000_u64,
            "hasQuotaData": true
        });
        let windows = parse_bl_output(&body).unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].name, "订阅月");
        assert!((windows[0].remaining_percent() - 99.92067366666667).abs() < 1e-6);
        let dt = windows[0]
            .reset_at
            .as_deref()
            .unwrap()
            .parse::<chrono::DateTime<chrono::Utc>>()
            .unwrap();
        assert_eq!(dt.timestamp(), 1798214400);
    }

    #[test]
    fn has_quota_data_false_means_no_subscription() {
        let err = parse_bl_output(&json!({ "hasQuotaData": false })).unwrap_err();
        assert!(err.0.contains("未找到有效的 Token Plan"), "got: {}", err.0);
    }

    #[test]
    fn empty_object_means_no_subscription() {
        assert!(parse_bl_output(&json!({})).is_err());
    }

    #[test]
    fn windows_sorted_by_span() {
        let body = json!({
            "per1MonthPercentage": 0.5,
            "per1MonthResetTime": 1798214400000_u64,
            "per5HourPercentage": 0.2,
            "per5HourResetTime": 1798214400000_u64,
            "per1WeekPercentage": 0.3,
            "per1WeekResetTime": 1798214400000_u64,
            "hasQuotaData": true
        });
        let windows = parse_bl_output(&body).unwrap();
        assert_eq!(
            windows.iter().map(|w| w.name.as_str()).collect::<Vec<_>>(),
            ["5小时", "每周", "订阅月"]
        );
    }

    #[test]
    fn missing_reset_still_parses() {
        let body = json!({ "per1MonthPercentage": 0.1, "hasQuotaData": true });
        let windows = parse_bl_output(&body).unwrap();
        assert!(windows[0].reset_at.is_none());
        assert!((windows[0].remaining_percent() - 90.0).abs() < 1e-9);
    }

    #[test]
    fn reset_only_key_skipped() {
        let body = json!({ "per1MonthResetTime": 1798214400000_u64, "hasQuotaData": false });
        assert!(parse_bl_output(&body).is_err());
    }

    #[test]
    fn malformed_keys_ignored() {
        let body = json!({
            "fooPercentage": 0.5,
            "perXMonthPercentage": 0.5,
            "perMonthPercentage": 0.5,
            "per1YearPercentage": 0.5,
            "hasQuotaData": true
        });
        assert!(parse_bl_output(&body).is_err());
    }

    #[test]
    fn small_reset_timestamp_ignored() {
        let body = json!({ "per1MonthPercentage": 0.1, "per1MonthResetTime": 1798214400 });
        let windows = parse_bl_output(&body).unwrap();
        assert!(windows[0].reset_at.is_none());
    }

    #[test]
    fn over_used_clamped_to_zero_remaining() {
        let body = json!({ "per1MonthPercentage": 1.5, "hasQuotaData": true });
        let windows = parse_bl_output(&body).unwrap();
        assert_eq!(windows[0].used_percent, 100.0);
        assert_eq!(windows[0].remaining_percent(), 0.0);
    }

    #[test]
    fn negative_percentage_skipped() {
        let body = json!({ "per1MonthPercentage": -0.1, "hasQuotaData": true });
        assert!(parse_bl_output(&body).is_err());
    }

    /// 现网探针（手动跑：cargo test --lib live_bl_token_plan -- --ignored --nocapture）。
    /// 依赖本机 bl 控制台登录态，CI 不跑。
    #[test]
    #[ignore]
    fn live_bl_token_plan() {
        let out = std::process::Command::new("bl")
            .args(["usage", "token-plan", "--output", "json", "--quiet"])
            .output()
            .expect("bl CLI 应已安装");
        assert!(out.status.success(), "bl 应已登录: {:?}", out.stderr);
        let json: Value = serde_json::from_slice(&out.stdout).unwrap();
        let windows = parse_bl_output(&json).unwrap();
        for w in &windows {
            println!(
                "{}: 剩余 {:.2}% reset={:?}",
                w.name,
                w.remaining_percent(),
                w.reset_at
            );
        }
        assert!(!windows.is_empty());
    }
}
