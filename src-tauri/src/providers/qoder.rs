//! Qoder（阿里 Qwen 系 agentic IDE）订阅 Credits 配额供应商。
//!
//! 鉴权走官方 Personal Access Token（`pt-` 前缀，qoder.com / qoder.cn 控制台
//! Settings → Personal Access Tokens 创建）：先兑换短期 job token，再查询
//! openapi 配额接口。国际版与国内版账号体系独立、凭据不通，采用「先国际后
//! 国内」顺序探测：鉴权类失败（401/403）或 404 切换端点重试，均失败报 PAT 无效。
//!
//! 接口形态来自社区逆向成果（CreditDaddy / qoder2api 实测可用），非官方文档
//! 承诺；服务端调整时本供应商显式报错，不静默给出错误数值。

use super::{format_iso_reset, http_client, LlmProvider, ProviderError};
use crate::models::{BalanceData, QuotaInfo};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

/// 国际版 openapi 基址（qoder.com 账号）
pub(crate) const GLOBAL_API_BASE: &str = "https://openapi.qoder.sh";
/// 国内版 openapi 基址（qoder.cn 账号）
pub(crate) const CN_API_BASE: &str = "https://openapi.qoder.com.cn";

// --- 响应数据结构（字段宽容：缺失一律 default，避免服务端加字段即解析失败） ---

#[derive(Clone, Deserialize, Default)]
struct QuotaUsageResponse {
    #[serde(rename = "userQuota", default)]
    user_quota: Option<QuotaBucket>,
    #[serde(rename = "addOnQuota", default)]
    add_on_quota: Option<QuotaBucket>,
    #[serde(rename = "dedicatedResourcePackages", default)]
    dedicated_packages: Vec<DedicatedPackage>,
}

#[derive(Clone, Deserialize, Default)]
struct QuotaBucket {
    #[serde(default)]
    used: f64,
    #[serde(default)]
    total: f64,
    #[serde(default)]
    remaining: f64,
    /// ISO 字符串或 epoch 数值两种形态，经 normalize_time 归一
    #[serde(rename = "resetTime", default)]
    reset_time: Option<Value>,
}

#[derive(Clone, Deserialize, Default)]
struct DedicatedPackage {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    used: f64,
    #[serde(default)]
    total: f64,
    #[serde(default)]
    remaining: f64,
    #[serde(rename = "expiresAt", default)]
    expires_at: Option<Value>,
    #[serde(default)]
    available: bool,
}

/// 把 resetTime / expiresAt 的宽容形态（ISO 字符串 / epoch 毫秒或秒，含数字字符串）
/// 归一化为 RFC3339 UTC；解析不了返回 None（展示层自行回退）。
fn normalize_time(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => {
            if let Ok(dt) = s.parse::<DateTime<Utc>>() {
                return Some(dt.to_rfc3339());
            }
            s.parse::<i64>().ok().and_then(epoch_to_rfc3339)
        }
        Value::Number(n) => n.as_i64().and_then(epoch_to_rfc3339),
        _ => None,
    }
}

/// epoch 秒/毫秒 → RFC3339；Qoder 实测下发毫秒，<10^12 防御性按秒解释
fn epoch_to_rfc3339(ts: i64) -> Option<String> {
    let secs = if ts > 1_000_000_000_000 {
        ts / 1000
    } else {
        ts
    };
    DateTime::from_timestamp(secs, 0).map(|dt| dt.to_rfc3339())
}

/// 剩余百分比：total 缺失（=0）时 remaining>0 视为满、否则 0
fn remaining_percent_of(total: f64, remaining: f64) -> f64 {
    if total > 0.0 {
        (remaining / total * 100.0).clamp(0.0, 100.0)
    } else if remaining > 0.0 {
        100.0
    } else {
        0.0
    }
}

/// 计划内额度 / 加量包 → 配额行；三角全零视为无数据不生成行
fn bucket_row(name: &str, b: &QuotaBucket, plan_label: &str) -> Option<QuotaInfo> {
    if b.used == 0.0 && b.total == 0.0 && b.remaining == 0.0 {
        return None;
    }
    let reset_at = normalize_time(b.reset_time.as_ref());
    let reset_time = reset_at.as_deref().map(format_iso_reset);
    Some(QuotaInfo {
        name: name.to_string(),
        used: b.used,
        total: b.total,
        current_value: Some(b.remaining),
        remaining: Some(b.remaining),
        remaining_percent: remaining_percent_of(b.total, b.remaining),
        reset_time,
        reset_at,
        plan_label: Some(plan_label.to_string()),
    })
}

/// 专属积分包 → 配额行。expiresAt 是「资源包到期」而非「窗口重置」：
/// 沿用 Kimi Code 订阅余额行先例，reset_at 留空（不参与倒计时/主窗口并列排序），
/// reset_time 只留到期文本展示。available=false 或 total=0 的包跳过。
fn package_row(p: &DedicatedPackage, plan_label: &str) -> Option<QuotaInfo> {
    if !p.available || p.total == 0.0 {
        return None;
    }
    let name = match p.name.as_deref() {
        Some(n) if !n.trim().is_empty() => n.to_string(),
        _ => "专属积分包".to_string(),
    };
    Some(QuotaInfo {
        name,
        used: p.used,
        total: p.total,
        current_value: Some(p.remaining),
        remaining: Some(p.remaining),
        remaining_percent: remaining_percent_of(p.total, p.remaining),
        reset_time: normalize_time(p.expires_at.as_ref())
            .as_deref()
            .map(format_iso_reset),
        reset_at: None,
        plan_label: Some(plan_label.to_string()),
    })
}

/// 全部额度桶平铺为配额行；plan_label 缺省「Qoder」
fn build_quota_rows(resp: &QuotaUsageResponse, plan_label: Option<&str>) -> Vec<QuotaInfo> {
    let label = plan_label.unwrap_or("Qoder");
    let mut rows = Vec::new();
    if let Some(b) = resp.user_quota.as_ref() {
        rows.extend(bucket_row("计划额度", b, label));
    }
    if let Some(b) = resp.add_on_quota.as_ref() {
        rows.extend(bucket_row("加量包", b, label));
    }
    for p in &resp.dedicated_packages {
        rows.extend(package_row(p, label));
    }
    rows
}

/// 主值：全部额度行中最紧张的剩余百分比；无行时 100（满额缺省，同 Kimi Code）
fn min_remaining_percent(rows: &[QuotaInfo]) -> f64 {
    rows.iter()
        .map(|q| q.remaining_percent)
        .fold(f64::INFINITY, f64::min)
        .clamp(0.0, 100.0)
}

// --- 网络层 ---

/// 网络链路分步错误：保留状态码类别供区域探测决策，出口统一转 ProviderError
enum StepError {
    /// 401/403：凭据被拒（PAT 无效/过期，或该凭据不属于此区域）
    AuthDenied,
    /// 其他非 2xx（404 参与区域决策：该区域网关无此路径）
    Http(u16),
    Network(String),
    Parse(String),
}

/// 区域探测决策：仅鉴权类失败或 404 切国内重试——5xx/网络/解析错误与账号
/// 区域无关，切换只会把真实故障掩盖成「两边都失败」。
fn should_switch_region(e: &StepError) -> bool {
    matches!(e, StepError::AuthDenied | StepError::Http(404))
}

fn to_provider_error(e: StepError) -> ProviderError {
    match e {
        StepError::AuthDenied => ProviderError(
            "Qoder PAT 无效或已过期，请在 qoder.com / qoder.cn 控制台 Settings → Personal Access Tokens 重新创建"
                .to_string(),
        ),
        StepError::Http(s) => ProviderError(format!("Qoder API error: {}", s)),
        StepError::Network(m) => ProviderError(format!("Qoder 请求失败: {}", m)),
        StepError::Parse(m) => ProviderError(format!("Qoder 响应解析失败: {}（接口可能已调整）", m)),
    }
}

/// 轮内快照：fetch_balance 与 fetch_quota_infos 共享一次网络链路
#[derive(Clone)]
struct QoderSnapshot {
    rows: Vec<QuotaInfo>,
}

pub struct QoderProvider {
    client: Client,
    api_key: String,
    cached: tokio::sync::OnceCell<QoderSnapshot>,
}

impl QoderProvider {
    pub fn new(api_key: &str) -> Self {
        Self {
            client: http_client(),
            api_key: api_key.trim().to_string(),
            cached: tokio::sync::OnceCell::new(),
        }
    }

    async fn snapshot(&self) -> Result<&QoderSnapshot, ProviderError> {
        self.cached
            .get_or_try_init(|| async { self.fetch_snapshot().await })
            .await
    }

    /// 先国际后国内；国际失败且可切换时重试国内，否则直接短路。
    /// 不缓存「区域归属」：每轮最多多一组请求（默认 120s 间隔），换无全局状态。
    async fn fetch_snapshot(&self) -> Result<QoderSnapshot, ProviderError> {
        match self.fetch_from_base(GLOBAL_API_BASE).await {
            Ok(rows) => Ok(QoderSnapshot { rows }),
            Err(e) if should_switch_region(&e) => {
                let rows = self
                    .fetch_from_base(CN_API_BASE)
                    .await
                    .map_err(to_provider_error)?;
                Ok(QoderSnapshot { rows })
            }
            Err(e) => Err(to_provider_error(e)),
        }
    }

    /// 在指定区域完成 token 解析（pt- 兑换）→ 配额查询 → 计划名（尽力）→ 行映射
    async fn fetch_from_base(&self, base: &str) -> Result<Vec<QuotaInfo>, StepError> {
        let token = self.resolve_token(base).await?;
        let resp = self.fetch_quota_usage(base, &token).await?;
        let plan = self.fetch_plan_label(base, &token).await;
        Ok(build_quota_rows(&resp, plan.as_deref()))
    }

    /// `pt-` 个人令牌先兑换短期 job token；其余原样使用（兼容直接粘贴 jt 联调）。
    async fn resolve_token(&self, base: &str) -> Result<String, StepError> {
        if !self.api_key.starts_with("pt-") {
            return Ok(self.api_key.clone());
        }
        let url = format!("{}/api/v1/jobToken/exchange", base);
        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("User-Agent", "qodercli/1.0.0")
            .header("Cosy-Version", "1.0.0")
            .header("Cosy-ClientType", "5")
            .body(serde_json::json!({ "personal_token": self.api_key }).to_string())
            .send()
            .await
            .map_err(|e| StepError::Network(e.to_string()))?;

        let status = resp.status();
        // 兑换端点对无效/不属于本区域的 PAT 一律 400 BadRequest（实测两个区域一致），
        // 401/403 见于过期；这里统一按「凭据被拒」处理——触发区域探测，双失败时报
        // PAT 无效文案而非裸状态码。
        if matches!(status.as_u16(), 400 | 401 | 403) {
            return Err(StepError::AuthDenied);
        }
        if !status.is_success() {
            return Err(StepError::Http(status.as_u16()));
        }
        let data: Value = resp
            .json()
            .await
            .map_err(|e| StepError::Parse(e.to_string()))?;
        match data.get("token").and_then(|v| v.as_str()) {
            Some(t) if !t.is_empty() => Ok(t.to_string()),
            _ => Err(StepError::Parse(
                "jobToken 兑换响应缺 token 字段".to_string(),
            )),
        }
    }

    async fn fetch_quota_usage(
        &self,
        base: &str,
        token: &str,
    ) -> Result<QuotaUsageResponse, StepError> {
        let url = format!("{}/api/v2/quota/usage", base);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Cosy-ClientType", "10")
            .header("User-Agent", "Qoder")
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| StepError::Network(e.to_string()))?;

        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(StepError::AuthDenied);
        }
        if !status.is_success() {
            return Err(StepError::Http(status.as_u16()));
        }
        resp.json()
            .await
            .map_err(|e| StepError::Parse(e.to_string()))
    }

    /// 订阅计划名（plan_tier_name），仅作 plan_label；任何失败都不阻断配额展示
    async fn fetch_plan_label(&self, base: &str, token: &str) -> Option<String> {
        let url = format!("{}/api/v2/user/plan", base);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Cosy-ClientType", "10")
            .header("User-Agent", "Qoder")
            .header("Accept", "application/json")
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let data: Value = resp.json().await.ok()?;
        data.get("plan_tier_name")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .filter(|s| !s.is_empty())
    }
}

#[async_trait]
impl LlmProvider for QoderProvider {
    fn name(&self) -> &str {
        "Qoder"
    }

    async fn fetch_balance(&self) -> Result<BalanceData, ProviderError> {
        let snap = self.snapshot().await?;
        let remaining = min_remaining_percent(&snap.rows);
        Ok(BalanceData {
            provider: "Qoder".to_string(),
            available_balance: remaining,
            voucher_balance: 0.0,
            cash_balance: 0.0,
            total_balance: remaining,
            currency: "%".to_string(),
        })
    }

    async fn fetch_quota_infos(&self) -> Result<Option<Vec<QuotaInfo>>, ProviderError> {
        let snap = self.snapshot().await?;
        if snap.rows.is_empty() {
            Ok(None)
        } else {
            Ok(Some(snap.rows.clone()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ---- normalize_time：ISO 字符串 / epoch 毫秒 / epoch 秒 / 数字字符串 / 非法 ----

    #[test]
    fn normalize_time_accepts_iso_string() {
        let out = normalize_time(Some(&json!("2026-11-05T00:00:00Z"))).unwrap();
        assert!(out.starts_with("2026-11-05T00:00:00"), "got: {out}");
    }

    #[test]
    fn normalize_time_accepts_epoch_ms_number_and_numeric_string() {
        let from_num = normalize_time(Some(&json!(1_772_956_800_000_i64))).unwrap();
        let from_str = normalize_time(Some(&json!("1772956800000"))).unwrap();
        assert_eq!(from_num, from_str);
        assert!(from_num.starts_with("2026-"), "got: {from_num}");
    }

    #[test]
    fn normalize_time_accepts_epoch_seconds() {
        let out = normalize_time(Some(&json!(1_772_956_800_i64))).unwrap();
        assert!(out.starts_with("2026-"), "got: {out}");
    }

    #[test]
    fn normalize_time_rejects_garbage_and_null() {
        assert!(normalize_time(Some(&json!("not-a-date"))).is_none());
        assert!(normalize_time(Some(&json!(null))).is_none());
        assert!(normalize_time(None).is_none());
    }

    // ---- build_quota_rows：桶平铺、空桶过滤、专属包到期语义 ----

    #[test]
    fn build_rows_maps_all_buckets_with_percent_and_reset() {
        let resp: QuotaUsageResponse = serde_json::from_value(json!({
            "userQuota": { "used": 200.0, "total": 1000.0, "remaining": 800.0,
                           "resetTime": "2026-11-05T00:00:00Z" },
            "addOnQuota": { "used": 10.0, "total": 100.0, "remaining": 90.0 },
            "dedicatedResourcePackages": [
                { "id": "p1", "name": "SOTA 专属积分", "used": 20.0, "total": 100.0,
                  "remaining": 80.0, "unit": "Credits", "expiresAt": 1_772_956_800_000_i64,
                  "available": true },
                { "id": "p2", "name": "过期包", "used": 0.0, "total": 50.0,
                  "remaining": 0.0, "available": false }
            ]
        }))
        .unwrap();
        let rows = build_quota_rows(&resp, Some("Pro"));
        assert_eq!(rows.len(), 3, "过期包(available=false)应被跳过");

        let plan = &rows[0];
        assert_eq!(plan.name, "计划额度");
        assert!((plan.remaining_percent - 80.0).abs() < 1e-9);
        assert_eq!(plan.current_value, Some(800.0));
        assert!(
            plan.reset_at.is_some(),
            "计划额度行必须有 reset_at 参与倒计时"
        );
        assert!(plan.reset_time.is_some());
        assert_eq!(plan.plan_label.as_deref(), Some("Pro"));

        assert_eq!(rows[1].name, "加量包");
        assert!((rows[1].remaining_percent - 90.0).abs() < 1e-9);
        assert!(
            rows[1].reset_at.is_none(),
            "无 resetTime 时 reset_at 应为 None"
        );

        let pkg = &rows[2];
        assert_eq!(pkg.name, "SOTA 专属积分");
        assert!((pkg.remaining_percent - 80.0).abs() < 1e-9);
        assert!(
            pkg.reset_at.is_none(),
            "专属包 expiresAt 是到期不是窗口重置，reset_at 必须为 None"
        );
        assert!(pkg.reset_time.is_some(), "到期文本走 reset_time 展示");
    }

    #[test]
    fn build_rows_skips_all_zero_buckets_and_uses_default_labels() {
        let resp: QuotaUsageResponse = serde_json::from_value(json!({
            "userQuota": { "used": 0.0, "total": 0.0, "remaining": 0.0 },
            "dedicatedResourcePackages": [
                { "name": "", "total": 0.0, "available": true },
                { "total": 10.0, "remaining": 10.0, "available": true }
            ]
        }))
        .unwrap();
        let rows = build_quota_rows(&resp, None);
        assert_eq!(rows.len(), 1, "全零桶与 total=0 的包不生成行");
        assert_eq!(rows[0].name, "专属积分包", "无名包用缺省名");
        assert_eq!(rows[0].plan_label.as_deref(), Some("Qoder"));
    }

    #[test]
    fn bucket_percent_falls_back_when_total_missing() {
        // total 缺失但仍有用量的畸形桶：remaining=0 → 0%
        let resp: QuotaUsageResponse = serde_json::from_value(json!({
            "userQuota": { "used": 5.0, "total": 0.0, "remaining": 0.0 }
        }))
        .unwrap();
        let rows = build_quota_rows(&resp, None);
        assert_eq!(rows.len(), 1);
        assert!((rows[0].remaining_percent - 0.0).abs() < 1e-9);
    }

    // ---- 主值：最紧张桶 ----

    #[test]
    fn min_remaining_takes_tightest_row_and_defaults_full() {
        let resp: QuotaUsageResponse = serde_json::from_value(json!({
            "userQuota": { "used": 200.0, "total": 1000.0, "remaining": 800.0 },
            "addOnQuota": { "used": 70.0, "total": 100.0, "remaining": 30.0 }
        }))
        .unwrap();
        let rows = build_quota_rows(&resp, None);
        assert!((min_remaining_percent(&rows) - 30.0).abs() < 1e-9);
        assert!(
            (min_remaining_percent(&[]) - 100.0).abs() < 1e-9,
            "空行 clamp 后应为满额 100"
        );
    }

    // ---- 区域探测决策（纯函数） ----

    #[test]
    fn region_switch_only_on_auth_or_404() {
        assert!(should_switch_region(&StepError::AuthDenied));
        assert!(should_switch_region(&StepError::Http(404)));
        assert!(!should_switch_region(&StepError::Http(500)));
        assert!(!should_switch_region(&StepError::Network(
            "timeout".to_string()
        )));
        assert!(!should_switch_region(&StepError::Parse(
            "bad json".to_string()
        )));
    }

    // ---- 错误文案可操作 ----

    #[test]
    fn auth_error_message_names_pat_and_console() {
        let msg = to_provider_error(StepError::AuthDenied).0;
        assert!(msg.contains("PAT"), "got: {msg}");
        assert!(msg.contains("Personal Access Tokens"), "got: {msg}");
    }

    #[test]
    fn http_error_keeps_status_and_parse_mentions_adjustment() {
        assert_eq!(
            to_provider_error(StepError::Http(500)).0,
            "Qoder API error: 500"
        );
        assert!(to_provider_error(StepError::Parse("x".to_string()))
            .0
            .contains("接口可能已调整"));
        assert!(to_provider_error(StepError::Network("e".to_string()))
            .0
            .contains("请求失败"));
    }

    // ---- 手动 E2E：CI 不跑（无凭据） ----
    // QODER_PAT=<pt-...> cargo test qoder_e2e_real_api -- --ignored --nocapture

    #[test]
    #[ignore]
    fn qoder_e2e_real_api() {
        let pat = std::env::var("QODER_PAT").expect("设 QODER_PAT 后运行");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let provider = QoderProvider::new(&pat);
        let quotas = rt
            .block_on(provider.fetch_quota_infos())
            .expect("拉取配额失败");
        println!("{quotas:#?}");
        assert!(quotas.is_some(), "无任何配额行（检查 PAT 与区域）");
    }
}
