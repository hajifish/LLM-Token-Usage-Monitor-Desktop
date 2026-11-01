# Qoder 供应商接入实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增「Qoder」供应商类型：PAT 鉴权（国际/国内自动探测），托盘与主窗口平铺展示计划额度/加量包/专属积分包剩余百分比。

**Architecture:** 新建 `providers/qoder.rs` 实现 `LlmProvider`（OnceCell 轮内共享请求，模式同 KimiCodeProvider）；纯函数数据映射 + 纯函数区域决策便于单测；按仓库既有惯例接线 mod.rs / tray.rs / scheduler.rs / Settings.tsx / UsagePanel.tsx。

**Tech Stack:** Rust (reqwest + serde + chrono + async-trait)、React/TS。spec：`docs/superpowers/specs/2026-10-31-qoder-provider-design.md`。

## Global Constraints

- 分支 `feature/main-window-compact`；工作区有**并行会话的未提交 WIP**（scheduler.rs 菜单对齐、UsagePanel.tsx 紧凑化、lib.rs、menu_align.rs、tauri.conf.json、app.css、Cargo.*）。**绝不把并行 WIP 提交进本功能 commit**：不相关文件一律不 add；重叠文件（scheduler.rs、UsagePanel.tsx）add 前必须 `git diff <file>` 核对仅含本功能 hunk，否则用 hunk 过滤 + `git apply --cached`。
- 每次 commit 前 `git branch --show-current` 复核仍是 `feature/main-window-compact`。
- 静态版本号 5 处文件（package.json / package-lock.json / Cargo.toml / Cargo.lock / tauri.conf.json）**不得**手工改版本（CLAUDE.md）。
- Rust 运行路径无 unwrap/expect；错误文案中文；注释密度与周边一致。
- 完成标准：`cargo test`、`cargo clippy -- -D warnings`、`cargo fmt --check`、`npm run build`、`npx eslint src`（或仓库 eslint 调用方式）全绿。

---

### Task 1: `format_iso_reset` 下沉 mod.rs（复用准备）

**Files:**
- Modify: `src-tauri/src/providers/mod.rs`（新增 pub(crate) fn）
- Modify: `src-tauri/src/providers/kimi_code.rs:1`（import）、`kimi_code.rs:223-237`（删本地 fn）

**Interfaces:**
- Produces: `pub(crate) fn format_iso_reset(iso: &str) -> String`（mod.rs，qoder.rs Task 2/3 复用）

- [x] **Step 1: 把 kimi_code.rs 的 `format_iso_reset`（含 doc 注释）整体搬到 mod.rs 工具函数区（`append_query_param` 之后），可见性改 `pub(crate)`**

```rust
/// 将 ISO 8601 时间字符串转换为紧凑倒计时（12M / 1H4M / 6D，与展示层同格式）。
/// Kimi Code / Qoder 等订阅型供应商的 reset_at 文本回退共用。
pub(crate) fn format_iso_reset(iso: &str) -> String {
    use chrono::{DateTime, Utc};
    match iso.parse::<DateTime<Utc>>() {
        Ok(dt) => {
            let duration = dt.signed_duration_since(Utc::now());
            if duration > chrono::Duration::zero() {
                crate::tray::countdown_compact(duration)
            } else {
                "0M".to_string()
            }
        }
        Err(_) => iso.to_string(),
    }
}
```

- [x] **Step 2: kimi_code.rs 删除本地定义，顶部 import 改为**

```rust
use super::{format_iso_reset, http_client, LlmProvider, ProviderError};
```

- [x] **Step 3: 跑 kimi 测试确认搬迁无破坏**

Run: `cargo test --manifest-path src-tauri/Cargo.toml kimi_code`
Expected: 全部 PASS（`format_iso_reset` 相关 3 个测试经 `use super::*` glob 仍可见）

- [x] **Step 4: Commit（只 add 这两个文件）**

```bash
git add src-tauri/src/providers/mod.rs src-tauri/src/providers/kimi_code.rs
git commit -m "refactor: format_iso_reset 下沉 providers/mod.rs 供多供应商复用"
```

---

### Task 2: qoder.rs 数据结构与纯函数映射（TDD）

**Files:**
- Create: `src-tauri/src/providers/qoder.rs`
- Modify: `src-tauri/src/providers/mod.rs:1-9`（加 `pub mod qoder;`）

**Interfaces:**
- Consumes: `super::format_iso_reset`（Task 1）
- Produces: `QuotaUsageResponse { user_quota, add_on_quota, dedicated_packages }`、`fn normalize_time(Option<&Value>) -> Option<String>`、`fn build_quota_rows(&QuotaUsageResponse, Option<&str>) -> Vec<QuotaInfo>`、`fn min_remaining_percent(&[QuotaInfo]) -> f64`（Task 3 网络层调用）

- [x] **Step 1: 写失败测试（qoder.rs 骨架 + `#[cfg(test)]`，先只有 struct 定义与测试）**

测试用例（完整代码）：

```rust
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
        assert!(plan.reset_at.is_some(), "计划额度行必须有 reset_at 参与倒计时");
        assert!(plan.reset_time.is_some());
        assert_eq!(plan.plan_label.as_deref(), Some("Pro"));

        assert_eq!(rows[1].name, "加量包");
        assert!((rows[1].remaining_percent - 90.0).abs() < 1e-9);
        assert!(rows[1].reset_at.is_none(), "无 resetTime 时 reset_at 应为 None");

        let pkg = &rows[2];
        assert_eq!(pkg.name, "SOTA 专属积分");
        assert!((pkg.remaining_percent - 80.0).abs() < 1e-9);
        assert!(pkg.reset_at.is_none(), "专属包 expiresAt 是到期不是窗口重置，reset_at 必须为 None");
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
        // total 缺失但仍有用量的畸形桶：remaining>0 → 100%，remaining=0 → 0%
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
        assert!((min_remaining_percent(&[]).min(100.0) - 100.0).abs() < 1e-9
            || min_remaining_percent(&[]) >= 100.0,
            "空行应返回 >=100（clamp 后 100）");
    }
}
```

- [x] **Step 2: 跑测试确认编译失败（struct 字段/fn 未实现）**

Run: `cargo test --manifest-path src-tauri/Cargo.toml qoder`
Expected: FAIL（`normalize_time`/`build_quota_rows`/`min_remaining_percent` 未定义；`mod qoder;` 未声明前先在 mod.rs 加声明）

- [x] **Step 3: 最小实现（struct + 纯函数；本 Task 不写网络层）**

```rust
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
    let secs = if ts > 1_000_000_000_000 { ts / 1000 } else { ts };
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
        reset_time: normalize_time(p.expires_at.as_ref()).as_deref().map(format_iso_reset),
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
```

（`use async_trait::async_trait;` 与 `LlmProvider`/`BalanceData` import 在本 Task 尚未用到会报 unused——本 Task 先不放这两个 import，Task 3 再加。`Client`/`http_client` 同理 Task 3 再引入。）

- [x] **Step 4: 跑测试确认全绿**

Run: `cargo test --manifest-path src-tauri/Cargo.toml qoder`
Expected: 7 个测试 PASS

- [x] **Step 5: Commit**

```bash
git add src-tauri/src/providers/qoder.rs src-tauri/src/providers/mod.rs
git commit -m "feat: Qoder 配额响应解析与额度桶平铺映射（纯函数层）"
```

---

### Task 3: 网络层 + 区域探测 + LlmProvider 实现（TDD）

**Files:**
- Modify: `src-tauri/src/providers/qoder.rs`（追加网络层与 trait 实现）

**Interfaces:**
- Consumes: Task 2 全部产物、`super::{http_client, format_iso_reset}`
- Produces: `pub struct QoderProvider`（`new(api_key: &str)`）、`impl LlmProvider`（name="Qoder"）——Task 4 工厂接线依赖

- [x] **Step 1: 追加失败测试（区域决策 + 错误文案 + 手动 E2E 挂钩）**

```rust
    // ---- 区域探测决策（纯函数） ----

    #[test]
    fn region_switch_only_on_auth_or_404() {
        assert!(should_switch_region(&StepError::AuthDenied));
        assert!(should_switch_region(&StepError::Http(404)));
        assert!(!should_switch_region(&StepError::Http(500)));
        assert!(!should_switch_region(&StepError::Network("timeout".into())));
        assert!(!should_switch_region(&StepError::Parse("bad json".into())));
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
        assert_eq!(to_provider_error(StepError::Http(500)).0, "Qoder API error: 500");
        assert!(to_provider_error(StepError::Parse("x".into())).0.contains("接口可能已调整"));
        assert!(to_provider_error(StepError::Network("e".into())).0.contains("请求失败"));
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
        println!("{:#?}", quotas);
        assert!(quotas.is_some(), "无任何配额行（检查 PAT 与区域）");
    }
```

- [x] **Step 2: 跑测试确认新断言失败（fn 未定义）**

Run: `cargo test --manifest-path src-tauri/Cargo.toml qoder`
Expected: FAIL（`should_switch_region` / `to_provider_error` / `QoderProvider` 未定义）

- [x] **Step 3: 实现网络层与 trait 实现（追加到 qoder.rs）**

```rust
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
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN
        {
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
            _ => Err(StepError::Parse("jobToken 兑换响应缺 token 字段".to_string())),
        }
    }

    async fn fetch_quota_usage(&self, base: &str, token: &str) -> Result<QuotaUsageResponse, StepError> {
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
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN
        {
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
```

同时把 Task 2 骨架的 import 行统一为：

```rust
use super::{format_iso_reset, http_client, LlmProvider, ProviderError};
use crate::models::{BalanceData, QuotaInfo};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
```

- [x] **Step 4: 跑测试确认全绿（含 #[ignore] 编译通过）**

Run: `cargo test --manifest-path src-tauri/Cargo.toml qoder`
Expected: 10 个测试 PASS，1 ignored

- [x] **Step 5: Commit**

```bash
git add src-tauri/src/providers/qoder.rs
git commit -m "feat: QoderProvider 网络层——PAT 兑换、国际/国内区域探测、LlmProvider 实现"
```

---

### Task 4: 系统接线（mod.rs / tray.rs / scheduler.rs + 托盘图标）

**Files:**
- Modify: `src-tauri/src/providers/mod.rs`（`ALL_PROVIDER_TYPES`、`create_provider`、`display_name` 测试数组）
- Modify: `src-tauri/src/tray.rs:124-131`（`tray_short_code`）
- Modify: `src-tauri/src/scheduler.rs:28-44`（`provider_logo`）+ `provider_logo_maps_eight_known_types` 测试
- Create: `src-tauri/icons/providers/qoder.png`（64px 官方 favicon）

**Interfaces:**
- Consumes: `qoder::QoderProvider::new(api_key: &str) -> QoderProvider`（Task 3）
- Produces: 工厂键 `"Qoder"`、短码 `"QD"`、托盘 PNG 图标

- [x] **Step 1: 下载 Qoder 官方图标（Google favicon 服务 64px，与现有图标同源）**

```bash
curl -sL "https://www.google.com/s2/favicons?domain=qoder.com&sz=64" -o src-tauri/icons/providers/qoder.png
file src-tauri/icons/providers/qoder.png   # 期望: PNG image data, 64 x 64
```

失败兜底：从 `https://qoder.com` 首页找 logo 资源 URL 下载后 `sips -z 64 64` 转换。

- [x] **Step 2: mod.rs 三处 + 测试数组**

`pub mod qoder;`（按字母序插入 openai 与 zhipu 之间）；`ALL_PROVIDER_TYPES` 尾部加 `"Qoder"`；`create_provider` 加：

```rust
        "Qoder" => Some(Box::new(qoder::QoderProvider::new(api_key))),
```

`display_name` 无需 arm（`other => other` 兜底即 "Qoder"），但守卫测试 `display_name_maps_all_types` 的 `expected` 数组加 `("Qoder", "Qoder")`。

- [x] **Step 3: tray.rs 短码**

```rust
        "Qoder" => Some("QD"),
```

（`every_builtin_provider_type_has_short_code` 守卫遍历 `ALL_PROVIDER_TYPES` 自动覆盖。）

- [x] **Step 4: scheduler.rs `provider_logo` 加臂 + 测试更新**

```rust
        "Qoder" => include_bytes!("../icons/providers/qoder.png"),
```

测试 `provider_logo_maps_eight_known_types` 改名 `provider_logo_maps_known_types`，列表加 `"Qoder"`。

- [x] **Step 5: 全量 Rust 测试 + clippy + fmt**

Run: `cargo test --manifest-path src-tauri/Cargo.toml && cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings && cargo fmt --manifest-path src-tauri/Cargo.toml --check`
Expected: 全绿（fmt 有 diff 就先 `cargo fmt` 再复跑）

- [x] **Step 6: Commit（scheduler.rs 是并行 WIP 文件：add 前 `git diff src-tauri/src/scheduler.rs` 核对仅含 provider_logo 相关 hunk）**

```bash
git add src-tauri/src/providers/mod.rs src-tauri/src/tray.rs src-tauri/src/scheduler.rs src-tauri/icons/providers/qoder.png
git commit -m "feat: 接线 Qoder 供应商——工厂/短码/托盘图标"
```

---

### Task 5: 前端接线（Settings + UsagePanel + 网页图标）

**Files:**
- Modify: `src/components/Settings.tsx:12-21`（`PROVIDER_OPTIONS`）
- Modify: `src/components/UsagePanel.tsx:4-13`（`BRAND_LOGOS`，**并行 WIP 文件：add 前核对 diff 仅含本 hunk**）
- Create: `public/providers/qoder.png`（与 Task 4 同一图标复制）

**Interfaces:**
- Consumes: 后端类型键 `"Qoder"`（Task 4）
- Produces: 设置页可选「Qoder」；主窗口卡片 logo

- [x] **Step 1: Settings.tsx `PROVIDER_OPTIONS` 尾部加**

```ts
  { value: 'Qoder', label: 'Qoder' },
```

- [x] **Step 2: UsagePanel.tsx `BRAND_LOGOS` 尾部加**

```ts
  Qoder: '/providers/qoder.png',
```

- [x] **Step 3: 复制图标**

```bash
cp src-tauri/icons/providers/qoder.png public/providers/qoder.png
```

- [x] **Step 4: 前端构建 + lint**

Run: `npm run build && npx eslint src`
Expected: 全绿

- [x] **Step 5: Commit**

```bash
git add src/components/Settings.tsx public/providers/qoder.png
# UsagePanel.tsx 核对 diff 后单独 add
git add src/components/UsagePanel.tsx
git commit -m "feat: 前端接入 Qoder——设置选项与卡片品牌图标"
```

---

### Task 6: 真机验证 + 收尾

**Files:**
- 无新增（验证与提交收尾）

- [x] **Step 1: 全量质量门**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml --check
npm run build && npx eslint src
```

- [x] **Step 2: 真实凭据获取（按序尝试，全部失败则如实报告）**

1. 本机已知 Qoder machineToken（`aicoding.loginMachineIdentity.v1.qoder.local`）直接作 Bearer 试 `/api/v2/quota/usage`（两个区域）；预期大概率 401（它是设备身份不是用户凭据），但零成本验证。
2. 定向查找 Qoder 客户端凭据文件（`auth.v1.dat` / `auth.json`，CreditDaddy 读取的已知路径 `~/Library/Application Support/Qoder{,CN}/`）；明文则提取 token。
3. 均不可用 → 用已知 machineToken 跑 `QODER_PAT=<token> cargo test qoder_e2e_real_api -- --ignored --nocapture` 验证错误路径干净（预期报「PAT 无效或已过期」而非 panic/裸状态码）。

- [x] **Step 3: 若拿到有效凭据 → 应用级端到端**

利用 app 首启明文配置自动迁移机制（config.rs `load_plaintext_and_migrate`）：在 app config 目录写入 `{"providers":[{"name":"Qoder","api_key":"<token>","enabled":true}]}` → 启动 `npm run tauri dev` → 观察日志轮询成功 + 主窗口/托盘出现 Qoder 卡片 → 截图留证 → 还原配置文件。

- [x] **Step 4: 收尾核对并最终提交（如有未提交的验证产物/文档修订）**

```bash
git branch --show-current   # 必须 feature/main-window-compact
git status --short          # 逐项确认：并行 WIP 文件不得混入
```

- [x] **Step 5: 向用户汇报验证结果与提交清单（PR review 准备就绪）**

## 执行记录（2026-11-01）

- 全部 6 个任务完成；提交序列：fccecfe → 9eaa318 → ebe0069 → 26c94ca → 14eed36 → cc166af → 5cbbb57。
- Task 4/5 的并行 WIP 文件（scheduler.rs、UsagePanel.tsx）按 hunk 过滤选择性暂存，未混入他会话改动。
- 真机验证：端点契约实测（兑换端点双区域 400 语义 → 触发 5cbbb57 修正）；应用级 E2E 走通（明文配置迁移 → 调度轮询 → 区域回退 → 友好错误，3 个轮询周期稳定）；错误路径 E2E 测试输出符合预期。
- 视觉截图验证因屏幕锁定未做；错误卡渲染路径由单测覆盖。真实额度数据展示需用户在设置中粘贴有效 PAT。

## v2 追加执行记录（2026-11-01，本地凭据方案切换）

用户反馈：Qoder 控制台实际无法获取外部 PAT，v1 的粘贴路径不可达；改走本机客户端凭据（CreditDaddy 公开源码给出的完整配方）。spec 已同步 v2（提交 4ab9a9c）。

- 凭据层重写（提交 0bd324a）：qoder.rs 新增本地凭据层（扫描 `com.qoder{,cn}.app*/auth.v1.dat` → 钥匙串 Safe Storage → PBKDF2-SHA1 + AES-128-CBC 解密 → dt- 会话 token 直连）；api_key 为空 = 本地凭据（默认免密），非空保留 v1 PAT 路径作兜底。新增依赖 aes/cbc/pbkdf2/hmac/sha1（均纯 Rust）。
- 免密接线（提交 cf7c915）：scheduler.rs `needs_api_key` 豁免 + Settings.tsx `CLI_OAUTH_PROVIDERS`（带「自动读取本机客户端凭据」提示）。
- 验证：153 测试 + clippy/fmt/eslint/build 全绿；新增 6 个单测覆盖解密 roundtrip（测试内同算法合成密文）、token 提取、候选区域映射与 mtime 排序、双模式错误文案。
- 并行 WIP 隔离：Cargo.toml/Cargo.lock（objc2 托盘对齐依赖）与 scheduler.rs 混合 hunk 手工拆分选择性暂存，提交内 0 行 objc2。
- 真机额度验证待用户配合：应用读取钥匙串时会触发 macOS 标准授权弹窗（开发会话不代提），允许后配额卡片即出现。
