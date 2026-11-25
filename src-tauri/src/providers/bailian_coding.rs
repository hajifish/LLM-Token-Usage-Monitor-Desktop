//! 阿里云百炼 Coding Plan 订阅配额供应商（免密 / 读本机凭据型）。
//!
//! Coding Plan 是百炼三套互不相通系统之一（按量计费 / Coding Plan / Token Plan），
//! 按「模型调用次数」计额，分 5 小时滚动 / 每周 / 账单月三窗口。
//!
//! v2 转向（2026-11-25，见 spec）：纯 `sk-sp-` Key 直连控制台网关被现网实测否定
//! （真/假 Key、无鉴权一律 `ConsoleNeedLogin`，三 header 被忽略）。改走百炼官方
//! `bl` CLI 的控制台通道：用户一次性 `bl auth login --console` 后，本供应商 shell
//! 调 `bl usage coding-plan --output json` 并解析 stdout，由 bl 负责网关/续期/区域。
//! 架构同 Kimi Code / Qoder 的免密读本机凭据型，不再收 api_key。
//! 安全边界：只读 bl 的本机登录态（经 bl 自身），不接触/不落盘/不外传任何凭据。

use super::{format_iso_reset, LlmProvider, ProviderError};
use crate::models::{BalanceData, QuotaInfo};
use async_trait::async_trait;
use serde_json::Value;

pub struct BailianCodingProvider {
    cached_response: tokio::sync::OnceCell<Vec<BailianWindow>>,
}

/// 解析后的单个配额窗口（原始调用次数 + RFC3339 重置时刻）。
#[derive(Clone, Debug)]
struct BailianWindow {
    name: String,
    used: f64,
    total: f64,
    reset_at: Option<String>,
}

impl BailianWindow {
    fn remaining_percent(&self) -> f64 {
        ((self.total - self.used) / self.total * 100.0).clamp(0.0, 100.0)
    }
}

impl BailianCodingProvider {
    pub fn new() -> Self {
        Self {
            cached_response: tokio::sync::OnceCell::new(),
        }
    }

    /// 通过 OnceCell 缓存只调一次 bl，同一轮调度内 fetch_balance 与 fetch_quota_infos 共享结果。
    async fn fetch_windows(&self) -> Result<Vec<BailianWindow>, ProviderError> {
        if let Some(cached) = self.cached_response.get() {
            return Ok(cached.clone());
        }
        let json = super::bailian_cli::usage_json("coding-plan").await?;
        let windows = parse_bl_output(&json)?;
        let _ = self.cached_response.set(windows.clone());
        Ok(windows)
    }
}

#[async_trait]
impl LlmProvider for BailianCodingProvider {
    fn name(&self) -> &str {
        "Bailian Coding"
    }

    async fn fetch_balance(&self) -> Result<BalanceData, ProviderError> {
        let windows = self.fetch_windows().await?;
        // 主窗口：5h 优先，回退周 → 月（windows 按此顺序排列，缺窗已跳过，首项即最高优先级可用窗口）
        let remaining = windows
            .first()
            .map(|w| w.remaining_percent())
            .unwrap_or(0.0);
        Ok(BalanceData {
            provider: "Bailian Coding".to_string(),
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
                let remaining = (w.total - w.used).max(0.0);
                QuotaInfo {
                    name: w.name.clone(),
                    used: w.used,
                    total: w.total,
                    current_value: Some(remaining),
                    remaining: Some(remaining),
                    remaining_percent: w.remaining_percent(),
                    reset_time: w.reset_at.as_ref().map(|s| format_iso_reset(s)),
                    reset_at: w.reset_at.clone(),
                    plan_label: Some("coding plan".to_string()),
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
//
// bl `usage coding-plan --output json` 的归一化输出（见 bailian-cli-commands 的 WT/UT）：
//   { "per5Hour": {usedQuota,totalQuota,resetTime,percentage}, "perWeek": {...},
//     "perBillMonth": {...}, "instanceType": "..." }
// 无 VALID 订阅时输出 {}；某窗口缺席时为 {}。resetTime 为 unix 毫秒（bl 渲染器 new Date(ms)，同 token-plan）。

fn parse_bl_output(json: &Value) -> Result<Vec<BailianWindow>, ProviderError> {
    let mut windows = Vec::new();
    for (key, name) in [
        ("per5Hour", "5小时"),
        ("perWeek", "每周"),
        ("perBillMonth", "账单月"),
    ] {
        let w = match json.get(key).filter(|v| v.is_object()) {
            Some(w) => w,
            None => continue,
        };
        let used = match w.get("usedQuota").and_then(|v| v.as_f64()) {
            Some(u) => u,
            None => continue,
        };
        let total = match w.get("totalQuota").and_then(|v| v.as_f64()) {
            Some(t) => t,
            None => continue,
        };
        if !used.is_finite() || !total.is_finite() || total <= 0.0 {
            continue;
        }
        let reset_at = w
            .get("resetTime")
            .and_then(|v| v.as_f64())
            .filter(|r| r.is_finite() && *r > 1e12)
            .and_then(|r| chrono::DateTime::from_timestamp((r / 1000.0) as i64, 0))
            .map(|dt| dt.to_rfc3339());
        windows.push(BailianWindow {
            name: name.to_string(),
            used,
            total,
            reset_at,
        });
    }

    if windows.is_empty() {
        return Err(ProviderError(
            "未找到有效的 Coding Plan 订阅（bl 返回空配额）".to_string(),
        ));
    }
    Ok(windows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn window(used: f64, total: f64, reset: u64) -> Value {
        json!({ "usedQuota": used, "totalQuota": total, "resetTime": reset, "percentage": used / total })
    }

    #[test]
    fn parses_full_three_windows() {
        let body = json!({
            "per5Hour": window(123.0, 6000.0, 1780000000000),
            "perWeek": window(1000.0, 45000.0, 1780500000000),
            "perBillMonth": window(9000.0, 90000.0, 1781000000000),
            "instanceType": "pro"
        });
        let windows = parse_bl_output(&body).unwrap();
        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0].name, "5小时");
        assert_eq!(windows[1].name, "每周");
        assert_eq!(windows[2].name, "账单月");
        assert!((windows[0].remaining_percent() - 97.95).abs() < 0.01);
        let dt = windows[0]
            .reset_at
            .as_deref()
            .unwrap()
            .parse::<chrono::DateTime<chrono::Utc>>()
            .unwrap();
        assert_eq!(dt.timestamp(), 1780000000);
    }

    #[test]
    fn empty_object_means_no_subscription() {
        let err = parse_bl_output(&json!({})).unwrap_err();
        assert!(err.0.contains("未找到有效的 Coding Plan"), "got: {}", err.0);
    }

    #[test]
    fn empty_window_skipped() {
        let body = json!({
            "per5Hour": window(10.0, 6000.0, 1780000000000),
            "perWeek": {},
            "perBillMonth": {}
        });
        let windows = parse_bl_output(&body).unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].name, "5小时");
    }

    #[test]
    fn partial_windows_render_available_only() {
        let body = json!({
            "per5Hour": window(10.0, 6000.0, 1780000000000),
            "perBillMonth": window(9000.0, 90000.0, 1781000000000)
        });
        let windows = parse_bl_output(&body).unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[1].name, "账单月");
    }

    #[test]
    fn zero_total_window_skipped() {
        let body = json!({
            "per5Hour": window(0.0, 0.0, 0),
            "perWeek": window(1000.0, 45000.0, 1780500000000)
        });
        let windows = parse_bl_output(&body).unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].name, "每周");
    }

    #[test]
    fn all_windows_empty_errors() {
        let body = json!({ "per5Hour": {}, "perWeek": {}, "perBillMonth": {} });
        assert!(parse_bl_output(&body).is_err());
    }

    #[test]
    fn small_reset_timestamp_ignored() {
        let body = json!({ "per5Hour": window(1.0, 6000.0, 0) });
        let windows = parse_bl_output(&body).unwrap();
        assert!(windows[0].reset_at.is_none());
    }

    #[test]
    fn missing_reset_still_parses() {
        let body = json!({ "per5Hour": { "usedQuota": 1, "totalQuota": 6000 } });
        let windows = parse_bl_output(&body).unwrap();
        assert!(windows[0].reset_at.is_none());
    }

    #[test]
    fn remaining_percent_clamped() {
        let w = BailianWindow {
            name: "5小时".into(),
            used: 7000.0,
            total: 6000.0,
            reset_at: None,
        };
        assert_eq!(w.remaining_percent(), 0.0);
    }
}
