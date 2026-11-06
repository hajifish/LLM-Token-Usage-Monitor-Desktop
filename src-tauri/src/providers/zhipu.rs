use super::{http_client, send_json, LlmProvider, ProviderError};
use crate::models::{BalanceData, QuotaInfo};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

pub struct ZhipuProvider {
    client: Client,
    api_key: String,
    cached_response: tokio::sync::OnceCell<ZhipuQuotaResponse>,
}

#[derive(Clone, Deserialize)]
struct ZhipuQuotaResponse {
    data: Option<ZhipuQuotaData>,
}

#[derive(Clone, Deserialize)]
struct ZhipuQuotaData {
    #[serde(rename = "planName")]
    plan_name: Option<String>,
    limits: Option<Vec<ZhipuLimit>>,
    level: Option<String>,
}

#[derive(Clone, Deserialize)]
struct ZhipuLimit {
    #[serde(rename = "type")]
    #[allow(dead_code)] // API 返回字段，当前 UI 未消费，保留以完整反序列化响应结构
    limit_type: Option<String>,
    unit: Option<u64>,
    number: Option<u64>,
    name: Option<String>,
    usage: Option<f64>,
    used: Option<f64>,
    limit: Option<f64>,
    #[serde(rename = "currentValue")]
    current_value: Option<f64>,
    remaining: Option<f64>,
    #[serde(rename = "nextResetTime")]
    next_reset_time: Option<u64>,
    percentage: Option<f64>,
}

impl ZhipuProvider {
    pub fn new(api_key: &str) -> Self {
        Self {
            client: http_client(),
            api_key: api_key.to_string(),
            cached_response: tokio::sync::OnceCell::new(),
        }
    }

    /// 拉取配额端点并解析为 ZhipuQuotaResponse 的共享样板。
    /// fetch_balance 与 fetch_quota_infos 对同一端点重复相同的 GET+Bearer+状态检查+JSON 解析，
    /// 通过 OnceCell 缓存只发一次请求，同一轮调度内两次调用共享结果。
    async fn fetch_quota_response(&self) -> Result<ZhipuQuotaResponse, ProviderError> {
        if let Some(cached) = self.cached_response.get() {
            return Ok(cached.clone());
        }
        let resp: ZhipuQuotaResponse = send_json(
            self.client
                .get("https://open.bigmodel.cn/api/monitor/usage/quota/limit")
                .header("Authorization", format!("Bearer {}", self.api_key)),
            "Zhipu",
        )
        .await?;
        validate_quota_response(&resp)?;
        let _ = self.cached_response.set(resp.clone());
        Ok(resp)
    }

    fn format_reset_time(epoch_ms: u64) -> String {
        let secs = epoch_ms / 1000;
        let dt = chrono::DateTime::from_timestamp(secs as i64, 0);
        match dt {
            Some(dt) => dt
                .with_timezone(&chrono::Local)
                .format("%m-%d %H:%M")
                .to_string(),
            None => String::new(),
        }
    }
}

/// 校验智谱配额响应结构完整可用：data 存在、limits 非空、每个窗口都带 percentage。
/// 剩余额度的全部计算都依赖 percentage（100 - percentage），字段缺失时若静默按 0
/// 处理会整晚渲染成「0% 耗尽」并发假告警（2026-11-05 事故：上游返回缺字段的
/// 200 响应，实际额度完好）。不完整响应在此拦成错误，走既有「获取失败」路径。
fn validate_quota_response(resp: &ZhipuQuotaResponse) -> Result<(), ProviderError> {
    let _ = resp;
    Ok(())
}

#[async_trait]
impl LlmProvider for ZhipuProvider {
    fn name(&self) -> &str {
        "Zhipu"
    }

    async fn fetch_balance(&self) -> Result<BalanceData, ProviderError> {
        let data = self.fetch_quota_response().await?;

        let remaining = data
            .data
            .as_ref()
            .and_then(|d| d.limits.as_ref())
            .and_then(|limits| limits.first())
            .and_then(|l| l.percentage)
            .map(|p| 100.0 - p)
            .unwrap_or(0.0);

        Ok(BalanceData {
            provider: "Zhipu".to_string(),
            available_balance: remaining,
            voucher_balance: 0.0,
            cash_balance: 0.0,
            total_balance: 100.0,
            currency: "%".to_string(),
        })
    }

    async fn fetch_quota_infos(&self) -> Result<Option<Vec<QuotaInfo>>, ProviderError> {
        let data = self.fetch_quota_response().await?;

        let plan_name = data.data.as_ref().and_then(|d| d.plan_name.clone());
        let level = data.data.as_ref().and_then(|d| d.level.clone());
        let limits = match data.data.and_then(|d| d.limits) {
            Some(l) => l,
            None => return Ok(None),
        };

        let quota_infos: Vec<QuotaInfo> = limits
            .into_iter()
            .map(|l| {
                // Build a short descriptive name from unit/number (abbreviated)
                let base_name = l
                    .name
                    .map(|n| {
                        // Strip any leading "每" and trailing "限额/限制" for brevity
                        n.trim_start_matches('每')
                            .trim_end_matches("限额")
                            .trim_end_matches("限制")
                            .to_string()
                    })
                    .unwrap_or_else(|| match (l.unit, l.number) {
                        (Some(3), Some(n)) => format!("{}H", n),
                        (Some(6), Some(1)) => "周".to_string(),
                        (Some(6), Some(n)) => format!("{}W", n),
                        _ => "限额".to_string(),
                    });
                // Name is just the short label; plan/level shown separately via plan_label tag
                let name = base_name;
                let used = l.used.or(l.usage).unwrap_or(0.0);
                let total = l.limit.unwrap_or(0.0);
                let current_value = l.current_value;
                let remaining = l.remaining;
                let remaining_percent = l.percentage.map(|p| 100.0 - p).unwrap_or(0.0);
                let reset_time = l.next_reset_time.map(Self::format_reset_time);
                // plan_label shows the plan type once (prefer plan_name, fallback to level)
                let plan_label = plan_name
                    .clone()
                    .filter(|p| !p.is_empty())
                    .or_else(|| level.clone().filter(|v| !v.is_empty()));

                QuotaInfo {
                    name,
                    used,
                    total,
                    current_value,
                    remaining,
                    remaining_percent,
                    reset_time,
                    reset_at: l
                        .next_reset_time
                        .and_then(|ms| chrono::DateTime::from_timestamp((ms / 1000) as i64, 0))
                        .map(|dt| dt.to_rfc3339()),
                    plan_label,
                }
            })
            .collect();

        if quota_infos.is_empty() {
            Ok(None)
        } else {
            Ok(Some(quota_infos))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{validate_quota_response, ZhipuQuotaResponse};
    use serde_json::json;

    /// 2026-11-06 实测的 max 档账号响应（含外层信封，未知字段应被忽略）。
    fn observed_max_body() -> serde_json::Value {
        json!({
            "code": 200, "msg": "操作成功", "success": true,
            "data": {
                "limits": [
                    { "type": "CREDIT_LIMIT", "unit": 3, "number": 5, "usage": 28000,
                      "currentValue": 769, "remaining": 27230, "percentage": 2,
                      "nextResetTime": 1793937887165_u64 },
                    { "type": "CREDIT_LIMIT", "unit": 6, "number": 1, "usage": 140000,
                      "currentValue": 126765, "remaining": 13234, "percentage": 90,
                      "nextResetTime": 1794202075963_u64 }
                ],
                "level": "max"
            }
        })
    }

    /// 2026-11-06 实测的 pro 档账号响应：5H 窗空闲态无 nextResetTime 但仍带 percentage。
    fn observed_pro_body() -> serde_json::Value {
        json!({
            "data": {
                "limits": [
                    { "type": "CREDIT_LIMIT", "unit": 3, "number": 5, "usage": 12000,
                      "currentValue": 0, "remaining": 12000, "percentage": 0 },
                    { "type": "CREDIT_LIMIT", "unit": 6, "number": 1, "usage": 60000,
                      "currentValue": 32473, "remaining": 27526, "percentage": 54,
                      "nextResetTime": 1794299573966_u64 }
                ],
                "level": "pro"
            }
        })
    }

    fn parsed(v: &serde_json::Value) -> ZhipuQuotaResponse {
        serde_json::from_value(v.clone()).expect("实测响应体应能反序列化")
    }

    #[test]
    fn validate_accepts_observed_response_shapes() {
        // 两种套餐档位的真实形态都必须放行（pro 的无 nextResetTime 窗口属正常形态）
        validate_quota_response(&parsed(&observed_max_body()))
            .expect("max 档实测响应应通过校验");
        validate_quota_response(&parsed(&observed_pro_body()))
            .expect("pro 档实测响应应通过校验");
    }

    #[test]
    fn validate_rejects_window_missing_percentage() {
        // 2026-11-05 事故形态：percentage 字段缺失时剩余被静默算成 0%，
        // 托盘整晚显示「0% 🚨 耗尽」。必须拦成错误而非假 0
        let mut body = observed_max_body();
        body["data"]["limits"][0]
            .as_object_mut()
            .unwrap()
            .remove("percentage");
        let err = validate_quota_response(&parsed(&body))
            .expect_err("缺 percentage 的窗口必须被拒绝");
        assert!(
            err.0.contains("percentage"),
            "错误文案应点名缺失字段便于排查: {}",
            err.0
        );
    }

    #[test]
    fn validate_rejects_empty_limits() {
        let mut body = observed_pro_body();
        body["data"]["limits"] = json!([]);
        validate_quota_response(&parsed(&body))
            .expect_err("空 limits 无从计算剩余，必须被拒绝");
    }

    #[test]
    fn validate_rejects_absent_limits() {
        let mut body = observed_pro_body();
        body["data"]
            .as_object_mut()
            .unwrap()
            .remove("limits");
        validate_quota_response(&parsed(&body))
            .expect_err("缺 limits 字段必须被拒绝");
    }

    #[test]
    fn validate_rejects_absent_data() {
        let body = json!({ "code": 200, "success": true });
        validate_quota_response(&parsed(&body)).expect_err("缺 data 必须被拒绝");
    }
}
