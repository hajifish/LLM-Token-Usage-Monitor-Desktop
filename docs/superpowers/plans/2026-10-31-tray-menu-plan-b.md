# 状态栏方案 B 重设计（原生菜单增强 + V3 聚合标题）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 托盘菜单按供应商分块展示（标题行 + 主窗口行 + 折叠子菜单），菜单栏常显文字改为 V3 聚合态（只显最紧张一项 + 紧凑倒计时）。

**Architecture:** 纯 Rust 展示层改造。`QuotaInfo` 新增 `reset_at`（ISO 时刻）支撑精确倒计时；倒计时/主窗口选择为可测纯函数（`now` 参数注入）；菜单构建沿用「一次性构建 + 签名原地 `set_text`」机制，签名扩展含 quota 窗口序列。

**Tech Stack:** Rust / Tauri v2 menu API（`Submenu`/`MenuItem`/`CheckMenuItem`）/ chrono 0.4（已有依赖）。

**Spec:** `docs/superpowers/specs/2026-10-31-tray-menu-plan-b-design.md`（已评审通过）

## Global Constraints

- 工作目录：主工作区 `/Users/shin/dev/Local-LLM-Token-Usage-Monitor-Desktop`，分支 `feat/tray-menu-redesign`（已 rebase 到 origin/develop @ 03c7e55）。**该工作区与其他并行会话共享：每次 git 写操作前先 `git branch --show-current` 确认仍在 `feat/tray-menu-redesign`。**
- 绝不 `set_menu` 替换活动菜单以外的路径改动：菜单重建仅在签名变化时发生（AppKit 并发闪退防线，PR #59 模式）。
- 展示阈值沿用 `provider_warning_level`（<5 黄 / ≤0 红 / CNY <10 黄 / USD 不参与），本次不调。
- 告警字符统一 `⚠️`（PR #61 既有序号），不引入新符号。
- 每个任务结束跑 `cargo fmt` + `cargo clippy -- -D warnings`（CI quality job 会拦）。
- 提交信息中文，末尾 `Co-Authored-By: Claude Code <noreply@anthropic.com>`。
- 测试命令：`cd src-tauri && cargo test`（基线 106 通过）。

---

### Task 1: `QuotaInfo.reset_at` 字段与四家供应商填充

**Files:**
- Modify: `src-tauri/src/models.rs:88-106`（QuotaInfo）
- Modify: `src-tauri/src/providers/claude_code.rs`（4 处构造）
- Modify: `src-tauri/src/providers/kimi_code.rs`（3 处构造）
- Modify: `src-tauri/src/providers/codex.rs`（2 处构造）
- Modify: `src-tauri/src/providers/zhipu.rs`（1 处构造）

**Interfaces:**
- Produces: `QuotaInfo.reset_at: Option<String>`（ISO 8601/RFC3339 UTC，如 `2026-10-31T18:00:00Z`）。后续任务的 `primary_quota`/`reset_remaining` 依赖此字段。

- [x] **Step 1: 写失败测试（models 序列化兼容 + 各家填充）**

`models.rs` 文件末尾追加测试模块（若无）：

```rust
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
```

`claude_code.rs` tests 模块追加（沿用现有 `test_claude_usage_response_with_five_hour_and_seven_day` 的 JSON 样板）：

```rust
#[tokio::test]
async fn quota_infos_carry_raw_iso_reset_at() {
    // provider 环境变量凭证不可用，直接测 fetch_quota_infos 内部逻辑等价物：
    // 构造带 resets_at 的响应 → QuotaInfo.reset_at 应为原始 ISO 串
    let json = r#"{"five_hour":{"utilization":38.0,"resets_at":"2026-10-31T18:00:00Z"},
        "seven_day":{"utilization":19.0,"resets_at":"2026-11-04T00:00:00Z"}}"#;
    let resp: ClaudeUsageResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.five_hour.as_ref().unwrap().resets_at.as_deref(),
        Some("2026-10-31T18:00:00Z"));
}
```

> 注：若 `ClaudeUsageResponse` 字段为私有，直接在同模块测试内可见（Rust 同文件模块可见性），无需改可见性。此测试锚定「原始 ISO 可得」；`reset_at` 填充断言见 Step 3 后的补充测试（fetch_quota_infos 需要 mock HTTP，走构造函数直填验证代替——本任务对 provider 的验证以编译 + 既有测试不破为准，填充正确性由 Task 3 的 `primary_quota` 集成测试覆盖输入端）。

- [x] **Step 2: 跑测试确认失败**

```bash
cd src-tauri && cargo test quota_reset_at
```
预期：编译失败 `no field reset_at on type QuotaInfo`。

- [x] **Step 3: 实现**

`models.rs` `QuotaInfo` 增加字段（放在 `reset_time` 之后）：

```rust
    /// 重置时刻（ISO 8601 / RFC3339 UTC），供展示层精确换算倒计时；
    /// 缺失时回退 reset_time 的本地化文本。向后兼容旧数据（serde default）
    #[serde(default)]
    pub reset_at: Option<String>,
```

四个 provider 的每个 `QuotaInfo {` 构造点，在 `reset_time:` 行后加一行：

claude_code.rs（4 处，`w.resets_at` 为原始 ISO 串）：
```rust
                    reset_at: w.resets_at.clone(),
```

kimi_code.rs（3 处）：
```rust
                    reset_at: r.reset_time.clone(),   // 两处 ratelimit；订阅余额处为 sb.expire_time.clone()
```

codex.rs（2 处，`pw.reset_at` / `sw.reset_at` 为 Unix 秒）：
```rust
                    reset_at: DateTime::<Utc>::from_timestamp(pw.reset_at, 0)
                        .map(|dt| dt.to_rfc3339()),
```
（文件顶部确认已有 `use chrono::{DateTime, Utc};`，codex.rs 现有 `format_reset_time` 已用。）

zhipu.rs（构造点在 `fetch_quota_infos` 内，`l.next_reset_time` 为 epoch 毫秒）：
```rust
                reset_at: l
                    .next_reset_time
                    .and_then(|ms| chrono::DateTime::from_timestamp((ms / 1000) as i64, 0))
                    .map(|dt| dt.to_rfc3339()),
```

- [x] **Step 4: 跑全部测试确认通过（既有测试不破）**

```bash
cd src-tauri && cargo test
```
预期：107+ 通过，0 失败（新增 1-2 个）。

- [x] **Step 5: Commit**

```bash
git add src-tauri/src/models.rs src-tauri/src/providers/
git commit -m "feat: QuotaInfo 新增 reset_at(ISO) 精确重置时刻，四家订阅计划供应商填充

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: 倒计时纯函数（tray.rs）

**Files:**
- Modify: `src-tauri/src/tray.rs`（在 `tray_value` 函数后新增函数组 + 文件顶部 `use chrono::{DateTime, Utc};`）
- Test: `src-tauri/src/tray.rs` tests 模块（`tray_title_tests` 同级新增 `countdown_tests` 模块）

**Interfaces:**
- Consumes: Task 1 的 `QuotaInfo.reset_at`
- Produces（Task 3/5 依赖，均在 tray.rs）:
  - `pub fn reset_remaining(iso: &str, now: DateTime<Utc>) -> Option<chrono::Duration>` — 解析剩余时长；失败/过期 None
  - `pub fn countdown_menu(d: chrono::Duration) -> String` — `12分钟后重置` / `4小时23分后重置` / `3天4小时后重置`
  - `pub fn countdown_compact(d: chrono::Duration) -> String` — `12分` / `4:23` / `3d`
  - `pub fn quota_countdown_menu(q: &QuotaInfo, now: DateTime<Utc>) -> Option<String>` — reset_at 精确换算，回退 reset_time 原文，均无 None
  - `pub fn quota_countdown_compact(q: &QuotaInfo, now: DateTime<Utc>) -> Option<String>`

- [x] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod countdown_tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};

    fn q(reset_at: Option<&str>, reset_time: Option<&str>) -> crate::models::QuotaInfo {
        crate::models::QuotaInfo {
            name: "5小时".into(), used: 38.0, total: 100.0,
            current_value: None, remaining: Some(62.0), remaining_percent: 62.0,
            reset_time: reset_time.map(Into::into), plan_label: None, reset_at: reset_at.map(Into::into),
        }
    }

    #[test]
    fn menu_format_boundaries() {
        assert_eq!(countdown_menu(Duration::minutes(12)), "12分钟后重置");
        assert_eq!(countdown_menu(Duration::minutes(60)), "1小时后重置");
        assert_eq!(countdown_menu(Duration::minutes(4 * 60 + 23)), "4小时23分后重置");
        assert_eq!(countdown_menu(Duration::hours(24)), "1天后重置");
        assert_eq!(countdown_menu(Duration::days(3) + Duration::hours(4)), "3天4小时后重置");
    }

    #[test]
    fn compact_format_boundaries() {
        assert_eq!(countdown_compact(Duration::minutes(12)), "12分");
        assert_eq!(countdown_compact(Duration::minutes(4 * 60 + 23)), "4:23");
        assert_eq!(countdown_compact(Duration::days(3) + Duration::hours(4)), "3d");
    }

    #[test]
    fn reset_remaining_parse_and_expiry() {
        let now = Utc.with_ymd_and_hms(2026, 10, 31, 12, 0, 0).unwrap();
        assert_eq!(reset_remaining("2026-10-31T13:24:00Z", now), Some(Duration::minutes(84)));
        assert_eq!(reset_remaining("2026-10-31T12:00:00Z", now), None); // 已到期
        assert_eq!(reset_remaining("garbage", now), None);
    }

    #[test]
    fn quota_countdown_prefers_reset_at_then_falls_back() {
        let now = Utc.with_ymd_and_hms(2026, 10, 31, 12, 0, 0).unwrap();
        // reset_at 精确换算
        let a = q(Some("2026-10-31T13:24:00Z"), Some("1小时后重置"));
        assert_eq!(quota_countdown_menu(&a, now).as_deref(), Some("1小时24分后重置"));
        assert_eq!(quota_countdown_compact(&a, now).as_deref(), Some("1:24"));
        // 无 reset_at 回退本地化文本；compact 回退同文本
        let b = q(None, Some("12分钟后重置"));
        assert_eq!(quota_countdown_menu(&b, now).as_deref(), Some("12分钟后重置"));
        // 均无
        assert_eq!(quota_countdown_menu(&q(None, None), now), None);
    }
}
```

- [x] **Step 2: 确认失败**

```bash
cd src-tauri && cargo test countdown
```
预期：编译失败（函数不存在）。

- [x] **Step 3: 实现（tray.rs，`tray_value` 之后）**

```rust
use chrono::{DateTime, Utc}; // 文件顶部补充

/// 解析 ISO/RFC3339 重置时刻为距 now 的剩余时长；解析失败或已到期返回 None
pub fn reset_remaining(iso: &str, now: DateTime<Utc>) -> Option<chrono::Duration> {
    let dt = iso.parse::<DateTime<Utc>>().ok()?;
    let d = dt.signed_duration_since(now);
    (d > chrono::Duration::zero()).then_some(d)
}

/// 菜单明细行倒计时（自然语言），输入为正时长
pub fn countdown_menu(d: chrono::Duration) -> String {
    let mins = d.num_minutes().max(1);
    if mins < 60 {
        format!("{}分钟后重置", mins)
    } else if mins < 24 * 60 {
        let (h, m) = (mins / 60, mins % 60);
        if m == 0 { format!("{}小时后重置", h) } else { format!("{}小时{}分后重置", h, m) }
    } else {
        let hours = d.num_hours();
        let (days, h) = (hours / 24, hours % 24);
        if h == 0 { format!("{}天后重置", days) } else { format!("{}天{}小时后重置", days, h) }
    }
}

/// 菜单栏紧凑倒计时，输入为正时长
pub fn countdown_compact(d: chrono::Duration) -> String {
    let mins = d.num_minutes().max(1);
    if mins < 60 {
        format!("{}分", mins)
    } else if mins < 24 * 60 {
        format!("{}:{:02}", mins / 60, mins % 60)
    } else {
        format!("{}d", d.num_hours() / 24)
    }
}

/// QuotaInfo 倒计时（菜单格式）：reset_at 精确换算，缺失/解析失败回退 reset_time 原文
pub fn quota_countdown_menu(q: &crate::models::QuotaInfo, now: DateTime<Utc>) -> Option<String> {
    q.reset_at
        .as_deref()
        .and_then(|iso| reset_remaining(iso, now))
        .map(countdown_menu)
        .or_else(|| q.reset_time.clone())
}

/// QuotaInfo 倒计时（菜单栏紧凑格式）：同上回退策略
pub fn quota_countdown_compact(q: &crate::models::QuotaInfo, now: DateTime<Utc>) -> Option<String> {
    q.reset_at
        .as_deref()
        .and_then(|iso| reset_remaining(iso, now))
        .map(countdown_compact)
        .or_else(|| q.reset_time.clone())
}
```

- [x] **Step 4: 跑测试通过**

```bash
cd src-tauri && cargo test countdown
```
预期：4 个新测试 PASS。

- [x] **Step 5: Commit**

```bash
git add src-tauri/src/tray.rs
git commit -m "feat: 托盘倒计时纯函数（菜单自然语言/菜单栏紧凑双格式，reset_at 精确换算+文本回退）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: 主窗口选择 + 供应商菜单块纯函数（tray.rs + scheduler.rs）

**Files:**
- Modify: `src-tauri/src/tray.rs`（新增 `pub fn primary_quota`）
- Modify: `src-tauri/src/scheduler.rs`（`format_provider_line` 重构为块函数组；`provider_signature` 扩展）
- Test: 两文件 tests 模块

**Interfaces:**
- Consumes: Task 2 的 `quota_countdown_menu`
- Produces:
  - tray.rs `pub fn primary_quota<'a>(quotas: &'a [QuotaInfo]) -> Option<&'a QuotaInfo>` — 剩余% 最低（差 <0.5 并列取重置时刻更近，无 reset_at 排最后）
  - scheduler.rs `pub(crate) struct ProviderMenuBlock { pub title: String, pub details: Vec<String>, pub extra_windows: Vec<String> }`
  - scheduler.rs `fn provider_menu_block(p: &ProviderStatus, now: DateTime<Utc>) -> ProviderMenuBlock`
  - scheduler.rs `fn quota_line(q: &QuotaInfo, now: DateTime<Utc>) -> String`
  - scheduler.rs `fn format_tokens(n: u64) -> String`
  - scheduler.rs `provider_signature` 新签名格式 `name|alias|q1,q2,...`（quota 名有序）

- [x] **Step 1: 写失败测试（tray.rs 追加进 countdown_tests 或新模块）**

```rust
#[cfg(test)]
mod primary_quota_tests {
    use super::*;
    use crate::models::QuotaInfo;

    fn q(name: &str, remaining: f64, reset_at: Option<&str>) -> QuotaInfo {
        QuotaInfo {
            name: name.into(), used: 100.0 - remaining, total: 100.0,
            current_value: None, remaining: Some(remaining), remaining_percent: remaining,
            reset_time: None, plan_label: None, reset_at: reset_at.map(Into::into),
        }
    }

    #[test]
    fn picks_lowest_remaining() {
        let qs = [q("5小时", 62.0, None), q("7天", 81.0, None)];
        assert_eq!(primary_quota(&qs).unwrap().name, "5小时");
    }

    #[test]
    fn near_tie_prefers_sooner_reset() {
        // 差 0.4 < 0.5 并列 → 取重置更近者（5小时虽然剩余略高但 1h 后重置）
        let qs = [
            q("7天", 62.0, Some("2026-11-06T00:00:00Z")),
            q("5小时", 62.4, Some("2026-10-31T13:00:00Z")),
        ];
        assert_eq!(primary_quota(&qs).unwrap().name, "5小时");
    }

    #[test]
    fn missing_reset_at_sorts_last_on_tie() {
        let qs = [q("7天", 62.0, None), q("5小时", 62.2, Some("2026-10-31T13:00:00Z"))];
        assert_eq!(primary_quota(&qs).unwrap().name, "5小时");
    }

    #[test]
    fn empty_returns_none() {
        assert!(primary_quota(&[]).is_none());
    }
}
```

scheduler.rs tests 模块追加（沿用现有 `status_with` 辅助；需补充带 quota 的辅助）：

```rust
    // ---------- provider_menu_block ----------

    fn quota(name: &str, used: f64, reset_at: Option<&str>) -> crate::models::QuotaInfo {
        crate::models::QuotaInfo {
            name: name.into(), used, total: 100.0, current_value: None,
            remaining: Some(100.0 - used), remaining_percent: 100.0 - used,
            reset_time: None, plan_label: None, reset_at: reset_at.map(Into::into),
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
            name: name.into(), alias: alias.into(), enabled: true,
            balance, usage, error: None, quota_infos: quotas,
        }
    }

    fn fixed_now() -> chrono::DateTime<chrono::Utc> {
        chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 10, 31, 12, 0, 0).unwrap()
    }

    #[test]
    fn percent_multi_window_block_has_title_primary_and_extras() {
        let b = crate::models::BalanceData {
            provider: "Claude Code".into(), available_balance: 62.0,
            voucher_balance: 0.0, cash_balance: 0.0, total_balance: 62.0, currency: "%".into(),
        };
        let quotas = vec![
            quota("5小时", 38.0, Some("2026-10-31T13:24:00Z")),
            quota("7天", 19.0, Some("2026-11-04T00:00:00Z")),
            quota("7天Opus", 5.0, None),
        ];
        let p = status_full("Claude Code", "Max工作", Some(b), Some(quotas), None);
        let block = provider_menu_block(&p, fixed_now());
        assert_eq!(block.title, "🟢 Max工作 — 剩 62%");
        assert_eq!(block.details.len(), 1);
        assert_eq!(block.details[0], "  5小时 [████░░░░░░] 剩62% · 1小时24分后重置");
        assert_eq!(block.extra_windows.len(), 2);
        assert!(block.extra_windows[0].starts_with("  7天 [█"));
    }

    #[test]
    fn percent_without_quota_degrades_to_title_only() {
        let b = crate::models::BalanceData {
            provider: "Zhipu".into(), available_balance: 47.0,
            voucher_balance: 0.0, cash_balance: 0.0, total_balance: 47.0, currency: "%".into(),
        };
        let p = status_full("Zhipu", "智谱", Some(b), None, None);
        let block = provider_menu_block(&p, fixed_now());
        assert_eq!(block.title, "🟢 智谱 — 剩 47%");
        assert!(block.details.is_empty() && block.extra_windows.is_empty());
    }

    #[test]
    fn api_block_with_usage_gets_today_line() {
        let b = crate::models::BalanceData {
            provider: "DeepSeek".into(), available_balance: 88.88,
            voucher_balance: 10.0, cash_balance: 78.88, total_balance: 88.88, currency: "CNY".into(),
        };
        let u = crate::models::UsageData {
            provider: "DeepSeek".into(), total_tokens: 1_234_567,
            prompt_tokens: 1_000_000, completion_tokens: 234_567,
            today_cost: None, today_requests: Some(42),
        };
        let p = status_full("DeepSeek", "DeepSeek", Some(b), None, Some(u));
        let block = provider_menu_block(&p, fixed_now());
        assert_eq!(block.title, "🟢 DeepSeek — ¥88.88");
        assert_eq!(block.details, vec!["  今日 42次 · 1.2M tok"]);
    }

    #[test]
    fn error_and_no_data_blocks() {
        let mut p = status_full("Kimi", "Kimi", None, None, None);
        p.error = Some("timeout".into());
        assert_eq!(provider_menu_block(&p, fixed_now()).title, "🟡 Kimi — 获取失败");
        let p2 = status_full("X", "X", None, None, None);
        assert_eq!(provider_menu_block(&p2, fixed_now()).title, "🟢 X — 无数据");
    }

    #[test]
    fn signature_includes_quota_names_in_order() {
        let p = status_full("CC", "a", None,
            Some(vec![quota("5小时", 38.0, None), quota("7天", 19.0, None)]), None);
        let s = UsageSummary { providers: vec![p], ..Default::default() };
        assert_eq!(provider_signature(&s), "CC|a|5小时,7天");
        // 无 quota 的供应商签名为空尾巴（与旧行为兼容段）
        let p2 = status_full("DS", "b", None, None, None);
        let s2 = UsageSummary { providers: vec![p2], ..Default::default() };
        assert_eq!(provider_signature(&s2), "DS|b|");
    }

    #[test]
    fn tokens_formatting() {
        assert_eq!(format_tokens(980), "980");
        assert_eq!(format_tokens(9_800), "9.8k");
        assert_eq!(format_tokens(1_234_567), "1.2M");
        assert_eq!(format_tokens(2_500_000_000), "2.5B");
    }
```

- [x] **Step 2: 确认失败**

```bash
cd src-tauri && cargo test primary_quota && cargo test provider_menu_block
```
预期：编译失败。

- [x] **Step 3: 实现**

tray.rs（`primary_quota` + 私有比较辅助，放在 countdown 函数组后）：

```rust
/// 订阅计划多窗口中的主窗口：剩余百分比最低者；
/// 差 <0.5 视为并列，取重置时刻更近者；无 reset_at 的排最后
pub fn primary_quota<'a>(quotas: &'a [crate::models::QuotaInfo]) -> Option<&'a crate::models::QuotaInfo> {
    quotas.iter().min_by(|a, b| {
        if (a.remaining_percent - b.remaining_percent).abs() >= 0.5 {
            a.remaining_percent
                .partial_cmp(&b.remaining_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        } else {
            reset_timestamp(a).cmp(&reset_timestamp(b))
        }
    })
}

fn reset_timestamp(q: &crate::models::QuotaInfo) -> i64 {
    q.reset_at
        .as_deref()
        .and_then(|s| s.parse::<DateTime<Utc>>().ok())
        .map(|dt| dt.timestamp())
        .unwrap_or(i64::MAX)
}
```

scheduler.rs：`format_provider_line` 整体替换为块函数组；`provider_signature` 替换；顶部 `use chrono::{DateTime, Utc};`、`use crate::models::{QuotaInfo, UsageData};`（并入现有 use）。

```rust
/// 一个供应商的托盘菜单块：标题行 + 明细行 + 折叠窗口行（非空时渲染「其余 N 个窗口」子菜单）
pub(crate) struct ProviderMenuBlock {
    pub title: String,
    pub details: Vec<String>,
    pub extra_windows: Vec<String>,
}

/// 明细行缩进（NSMenu 同级项无层级，用文本缩进模拟从属关系）
const DETAIL_INDENT: &str = "  ";

fn quota_line(q: &QuotaInfo, now: DateTime<Utc>) -> String {
    let used_pct = if q.total > 0.0 { q.used / q.total * 100.0 } else { 0.0 };
    let bar = progress_bar(used_pct, 10);
    match crate::tray::quota_countdown_menu(q, now) {
        Some(cd) => format!("{DETAIL_INDENT}{} {} 剩{:.0}% · {}", q.name, bar, q.remaining_percent, cd),
        None => format!("{DETAIL_INDENT}{} {} 剩{:.0}%", q.name, bar, q.remaining_percent),
    }
}

fn format_tokens(n: u64) -> String {
    let unit = if n >= 1_000_000_000 { (1_000_000_000, "B") }
        else if n >= 1_000_000 { (1_000_000, "M") }
        else if n >= 1_000 { (1_000, "k") }
        else { return n.to_string(); };
    let v = n as f64 / unit.0 as f64;
    if v >= 10.0 { format!("{:.0}{}", v, unit.1) } else { format!("{:.1}{}", v, unit.1) }
}

fn usage_line(u: &UsageData) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(cost) = u.today_cost { parts.push(format!("{:.2}", cost)); }
    if let Some(req) = u.today_requests { parts.push(format!("{}次", req)); }
    if u.total_tokens > 0 { parts.push(format!("{} tok", format_tokens(u.total_tokens))); }
    (parts.is_empty()).not().then(|| format!("{DETAIL_INDENT}今日 {}", parts.join(" · ")))
}

/// 供应商 → 菜单块。percent 型消费 quota_infos（标题主指标取主窗口 remaining_percent，
/// 无 quota 时回退 balance）；API 型标题为余额、有 usage 时加今日明细行
fn provider_menu_block(p: &ProviderStatus, now: DateTime<Utc>) -> ProviderMenuBlock {
    let dot = status_dot(crate::tray::provider_warning_level(p));
    let percent_quota = p.balance.as_ref().filter(|b| b.is_percent()).and_then(|_| {
        p.quota_infos.as_ref().filter(|qs| !qs.is_empty())
    });

    if let Some(quotas) = percent_quota {
        let primary_idx = quotas
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                if (a.remaining_percent - b.remaining_percent).abs() >= 0.5 {
                    a.remaining_percent.partial_cmp(&b.remaining_percent).unwrap_or(std::cmp::Ordering::Equal)
                } else {
                    let ta = a.reset_at.as_deref().and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok()).map(|d| d.timestamp()).unwrap_or(i64::MAX);
                    let tb = b.reset_at.as_deref().and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok()).map(|d| d.timestamp()).unwrap_or(i64::MAX);
                    ta.cmp(&tb)
                }
            })
            .map(|(i, _)| i);
        if let Some(idx) = primary_idx {
            let primary = &quotas[idx];
            return ProviderMenuBlock {
                title: format!("{} {} — 剩 {:.0}%", dot, p.alias, primary.remaining_percent),
                details: vec![quota_line(primary, now)],
                extra_windows: quotas
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != idx)
                    .map(|(_, q)| quota_line(q, now))
                    .collect(),
            };
        }
    }

    match &p.balance {
        Some(b) if b.is_percent() => ProviderMenuBlock {
            title: format!("{} {} — 剩 {:.0}%", dot, p.alias, b.available_balance),
            details: vec![], extra_windows: vec![],
        },
        Some(b) if b.is_usd() => ProviderMenuBlock {
            title: format!("{} {} — ${:.2}", dot, p.alias, b.available_balance),
            details: p.usage.as_ref().and_then(usage_line).into_iter().collect(),
            extra_windows: vec![],
        },
        Some(b) => ProviderMenuBlock {
            title: format!("{} {} — ¥{:.2}", dot, p.alias, b.available_balance),
            details: p.usage.as_ref().and_then(usage_line).into_iter().collect(),
            extra_windows: vec![],
        },
        None if p.error.is_some() => ProviderMenuBlock {
            title: format!("{} {} — 获取失败", dot, p.alias), details: vec![], extra_windows: vec![],
        },
        None => ProviderMenuBlock {
            title: format!("{} {} — 无数据", dot, p.alias), details: vec![], extra_windows: vec![],
        },
    }
}
```

> 说明：`percent_quota` 分支里的索引版比较与 `crate::tray::primary_quota` 逻辑一致但需返回索引（Rust 借用限制），实现为内联；两处行为由各自测试锚定一致（同一并列规则）。`usage_line` 里 `not()` 需要 `use std::ops::Not;`——如不想引入，改写为 `if parts.is_empty() { None } else { Some(...) }`（按此写法落地，避免额外 use）。

`provider_signature` 替换为：

```rust
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
                .map(|qs| qs.iter().map(|q| q.name.as_str()).collect::<Vec<_>>().join(","))
                .unwrap_or_default();
            format!("{}|{}|{}", p.name, p.alias, quotas)
        })
        .collect::<Vec<_>>()
        .join(";")
}
```

同时：本任务先保留旧 `format_provider_line` 供现有 `update_tray_menu` 编译通过（Task 4 移除）；其测试若断言旧行为，标记保留至 Task 4 更新。

- [x] **Step 4: 跑测试通过**

```bash
cd src-tauri && cargo test
```
预期：全部通过（新增约 10 个）。

- [x] **Step 5: Commit**

```bash
git add src-tauri/src/tray.rs src-tauri/src/scheduler.rs
git commit -m "feat: 主窗口选择规则与供应商菜单块纯函数（标题/明细/折叠窗口），签名扩展 quota 序列

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: 菜单构建消费块结构（update_tray_menu + TrayMenuHandles 块状化）

**Files:**
- Modify: `src-tauri/src/scheduler.rs`（`TrayMenuHandles` 结构、原地刷新路径、重建路径；删除旧 `format_provider_line` 及其旧测试）
- Test: 同文件 tests

**Interfaces:**
- Consumes: Task 3 的 `ProviderMenuBlock`/`provider_menu_block`/新签名
- Produces: 菜单最终结构（无函数签名变化；`update_tray_menu` 内部改造）

- [x] **Step 1: 写失败测试（签名驱动重建的判定单元）**

无法对 NSMenu 做纯单测，本任务可测点为「签名比较 + 块句柄结构生成逻辑」。在 tests 模块追加结构级测试：

```rust
    #[test]
    fn signature_changes_when_quota_set_changes() {
        let p1 = status_full("CC", "a", None, Some(vec![quota("5小时", 38.0, None)]), None);
        let p2 = status_full("CC", "a", None,
            Some(vec![quota("5小时", 38.0, None), quota("7天", 19.0, None)]), None);
        let s1 = UsageSummary { providers: vec![p1], ..Default::default() };
        let s2 = UsageSummary { providers: vec![p2], ..Default::default() };
        assert_ne!(provider_signature(&s1), provider_signature(&s2));
    }

    #[test]
    fn signature_stable_when_only_values_change() {
        let p1 = status_full("CC", "a", None, Some(vec![quota("5小时", 38.0, None)]), None);
        let p2 = status_full("CC", "a", None, Some(vec![quota("5小时", 41.0, None)]), None);
        let s1 = UsageSummary { providers: vec![p1], ..Default::default() };
        let s2 = UsageSummary { providers: vec![p2], ..Default::default() };
        assert_eq!(provider_signature(&s1), provider_signature(&s2));
    }
```

（这两个测试其实在 Task 3 后已能通过——它们锚定 Task 4 依赖的重建边界，作为本任务的前置验收跑一遍。）

- [x] **Step 2: 跑测试**

```bash
cd src-tauri && cargo test signature
```
预期：PASS（锚定完成）。

- [x] **Step 3: 实现 `update_tray_menu` 块状化**

3a. 结构替换（`TrayMenuHandles`）：

```rust
struct ProviderMenuHandles {
    title: MenuItem<Wry>,
    details: Vec<MenuItem<Wry>>,
    extra: Option<(Submenu<Wry>, Vec<MenuItem<Wry>>)>,
}

#[derive(Default)]
struct TrayMenuHandles {
    provider_blocks: Vec<ProviderMenuHandles>,
    time_item: Option<MenuItem<Wry>>,
    interval_items: Vec<(u64, CheckMenuItem<Wry>)>,
    signature: String,
}
```

顶部 use 补 `Submenu`：`use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};`（现状已有 CheckMenuItem 等，按需并齐）。

3b. 原地刷新路径（签名相同分支）替换 provider_items 循环为：

```rust
                if handles.signature == signature {
                    let now = Utc::now();
                    for (block, p) in handles
                        .provider_blocks
                        .iter()
                        .zip(summary.providers.iter())
                    {
                        let b = provider_menu_block(p, now);
                        let _ = block.title.set_text(b.title);
                        for (item, text) in block.details.iter().zip(b.details.iter()) {
                            let _ = item.set_text(text);
                        }
                        if let Some((_, extra_items)) = &block.extra {
                            for (item, text) in extra_items.iter().zip(b.extra_windows.iter()) {
                                let _ = item.set_text(text);
                            }
                        }
                    }
                    // time_item / interval_items 勾选同步逻辑保持原样
```

3c. 重建路径：供应商段（原 1) 循环）替换为块创建：

```rust
        // 1) 供应商块（标题行 + 明细行 + 折叠窗口子菜单），块上限沿用 50
        const MAX_PROVIDER_BLOCKS: usize = 50;
        let now = Utc::now();
        let mut provider_blocks = Vec::new();
        for (i, p) in summary.providers.iter().take(MAX_PROVIDER_BLOCKS).enumerate() {
            let block = provider_menu_block(p, now);
            let Ok(title) = MenuItem::with_id(
                app_handle,
                format!("p{i}_title"),
                &block.title,
                false,
                None::<&str>,
            ) else { continue };
            let _ = menu.append(&title);

            let mut detail_items = Vec::new();
            for (j, text) in block.details.iter().enumerate() {
                if let Ok(item) = MenuItem::with_id(
                    app_handle, format!("p{i}_d{j}"), text, false, None::<&str>,
                ) {
                    let _ = menu.append(&item);
                    detail_items.push(item);
                }
            }

            let mut extra_handle = None;
            if !block.extra_windows.is_empty() {
                let label = format!("其余 {} 个窗口", block.extra_windows.len());
                if let Ok(sub) = Submenu::with_id(
                    app_handle, format!("p{i}_extra"), &label, true, None::<&str>,
                ) {
                    let mut sub_items = Vec::new();
                    for (j, text) in block.extra_windows.iter().enumerate() {
                        if let Ok(item) = MenuItem::with_id(
                            app_handle, format!("p{i}_e{j}"), text, false, None::<&str>,
                        ) {
                            let _ = sub.append(&item);
                            sub_items.push(item);
                        }
                    }
                    let _ = menu.append(&sub);
                    extra_handle = Some((sub, sub_items));
                }
            }

            provider_blocks.push(ProviderMenuHandles {
                title,
                details: detail_items,
                extra: extra_handle,
            });
        }
```

溢出提示文案改为按块数：`…共 {} 个供应商`（原样，语义不变）；`TrayMenuHandles { provider_blocks, time_item: …, interval_items, signature }` 装配处同步字段名。删除旧 `format_provider_line`、`used_percent`（若仅它使用）及其旧测试（`format_percent_provider_contains_bar_and_percent` 等 5 个——断言对象已由 Task 3 新测试取代）。

> 子菜单标题必须 enabled（`true`）：NSMenu 中 disabled 的 submenu 无法展开；其 id `p{i}_extra` 无 on_menu_event 匹配，点击无副作用。折叠行内的窗口项 disabled。

- [x] **Step 4: 编译 + 全量测试 + clippy/fmt**

```bash
cd src-tauri && cargo fmt && cargo clippy -- -D warnings && cargo test
```
预期：0 错误，全部通过。

- [x] **Step 5: Commit**

```bash
git add src-tauri/src/scheduler.rs
git commit -m "feat: 托盘菜单按供应商分块（标题/明细/其余窗口子菜单），原地刷新覆盖子菜单条目

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 5: 菜单栏 V3 聚合标题（build_tray_title 重写）

**Files:**
- Modify: `src-tauri/src/tray.rs:155-196`（build_tray_title + 新辅助）
- Test: 同文件 `tray_title_tests`（多个期望值更新 + 新增聚合用例）

**Interfaces:**
- Consumes: Task 1 `reset_at`、Task 2 `countdown_compact`/`reset_remaining`、`primary_quota`（Task 3）
- Produces: V3 聚合格式（外部无新接口）

- [x] **Step 1: 更新/新增测试（期望值先行）**

`tray_title_tests` 内改动——先改辅助 `status()` 支持 quota_infos（复用本文件已有 balance/status 辅助，追加字段参数化辅助 `status_q`）：

```rust
    fn status_q(
        alias: &str,
        balance: Option<BalanceData>,
        quotas: Option<Vec<crate::models::QuotaInfo>>,
    ) -> ProviderStatus {
        ProviderStatus {
            name: alias.to_string(), alias: alias.to_string(), enabled: true,
            balance, usage: None, error: None, quota_infos: quotas,
        }
    }

    fn quota(name: &str, remaining: f64, reset_at: Option<&str>) -> crate::models::QuotaInfo {
        crate::models::QuotaInfo {
            name: name.into(), used: 100.0 - remaining, total: 100.0,
            current_value: None, remaining: Some(remaining), remaining_percent: remaining,
            reset_time: None, plan_label: None, reset_at: reset_at.map(Into::into),
        }
    }
```

既有测试期望值更新（V3 语义：只显第一名）：

| 测试 | 旧期望 | 新期望 |
|---|---|---|
| title_joins_mixed_segments_with_separator | `智谱 70% · DS ¥88.88 · OA $12.34` | `智谱 70%`（无告警→percent 型最低剩余优先） |
| percent_value_formats_as_integer | `智谱 70%` | `智谱 70%`（不变） |
| error_segment_shows_dash | `Kimi –` | `Kimi –`（不变） |
| duplicate_visible_accounts_get_index_suffix | `DS ¥88.00 · DS2 ¥5.00` | `⚠️ DS ¥5.00`（¥5 触发黄档居首；第一名用基础码无序号） |
| same_name_accounts_matched_by_alias | `DS ¥88.00` | `DS ¥88.00`（不变） |
| 隐藏项告警（463 行 private-号关闭场景） | `⚠️ DS ¥88.00` | `DS ¥88.00`（V3 告警只看可见候选；被隐藏项的告警由图标红点承担） |
| 557/567 行（智谱 3% / DS ¥0） | `⚠️ 智谱 3%` / `⚠️ DS ¥0.00` | 不变（第一名自身告警） |

新增聚合测试：

```rust
    #[test]
    fn v3_alert_with_compact_countdown() {
        // Codex 5h 剩 9%（<5%? 9% 未到黄档阈值…… 黄档为 <5，故用 3% 构造）
        let qs = vec![quota("5小时", 3.0, Some("2026-10-31T12:12:00Z"))];
        let mut p = status_q("Codex", Some(balance(3.0, "%")), Some(qs));
        p.name = "Codex".into();
        let s = UsageSummary { providers: vec![p], ..Default::default() };
        // 依赖系统时钟的倒计时断言不稳定 → 断言前缀与主值，倒计时格式由 countdown 测试覆盖：
        let title = build_tray_title(&config(vec![]), &s).unwrap();
        assert!(title.starts_with("⚠️ CX 3%"), "got: {title}");
    }

    #[test]
    fn v3_calm_shows_lowest_percent_no_prefix() {
        let mut cc = status_q("CC", Some(balance(62.0, "%")),
            Some(vec![quota("5小时", 62.0, None)]));
        cc.name = "Claude Code".into();
        let ds = status("DeepSeek", Some(balance(88.0, "CNY")), None); // 现有辅助
        let mut ds = ds; ds.name = "DeepSeek".into();
        let s = UsageSummary { providers: vec![cc, ds], ..Default::default() };
        assert_eq!(build_tray_title(&config(vec![]), &s).unwrap(), "CC 62%");
    }

    #[test]
    fn v3_api_only_falls_back_to_balance() {
        let mut ds = status("工作号", Some(balance(88.0, "CNY")), None);
        ds.name = "DeepSeek".into();
        let s = UsageSummary { providers: vec![ds], ..Default::default() };
        assert_eq!(build_tray_title(&config(vec![]), &s).unwrap(), "DS ¥88.00");
    }
```

（`status` 辅助本文件已存在；若其签名不含第三参 error 则沿用原样。）

- [x] **Step 2: 跑测试确认失败**

```bash
cd src-tauri && cargo test tray_title
```
预期：V3 相关用例 FAIL（旧实现输出全列表）。

- [x] **Step 3: 实现（替换 build_tray_title 主体）**

```rust
/// V3 聚合主值：percent 型取主窗口剩余%（无 quota 回退 balance）；API 型取余额；
/// 无数据返回 f64::MAX（排序垫底）
fn aggregate_value(p: &ProviderStatus) -> f64 {
    let Some(b) = &p.balance else { return f64::MAX };
    if b.is_percent() {
        if let Some(primary) = p.quota_infos.as_ref().and_then(primary_quota) {
            return primary.remaining_percent;
        }
    }
    b.available_balance
}

fn aggregate_is_percent(p: &ProviderStatus) -> bool {
    p.balance.as_ref().map(|b| b.is_percent()).unwrap_or(false)
}

/// 构建状态栏常显文字（V3 聚合态）：只显示最紧张的一项。
/// 排序：告警等级降序 → percent 型在前 → 主值升序 → 主窗口重置更近在前。
/// 告警字符沿用 ⚠️（PR #61 机制）；候选仅含可见供应商，被隐藏项的告警由图标红点承担。
pub fn build_tray_title(config: &AppConfig, summary: &UsageSummary) -> Option<String> {
    if !config.tray_text_enabled {
        return None;
    }
    let now = Utc::now();
    let mut visible: Vec<&ProviderStatus> = summary
        .providers
        .iter()
        .filter(|p| p.enabled && provider_show_in_tray(config, p))
        .collect();
    if visible.is_empty() {
        return None;
    }
    visible.sort_by(|a, b| {
        crate::tray::provider_warning_level(b).cmp(&crate::tray::provider_warning_level(a))
            .then_with(|| aggregate_is_percent(b).cmp(&aggregate_is_percent(a)))
            .then_with(|| aggregate_value(a).partial_cmp(&aggregate_value(b)).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| {
                let ta = p_reset_ts(a); let tb = p_reset_ts(b);
                ta.cmp(&tb)
            })
    });
    let p = visible[0];
    let code = tray_short_code(&p.name)
        .map(str::to_string)
        .unwrap_or_else(|| fallback_short_name(&p.alias));
    let body = match &p.balance {
        Some(b) if b.is_percent() => format!("{} {:.0}%", code, aggregate_value(p)),
        _ => format!("{} {}", code, tray_value(p)),
    };
    if provider_warning_level(p) > 0 {
        let cd = p
            .quota_infos
            .as_ref()
            .and_then(primary_quota)
            .and_then(|q| q.reset_at.as_deref())
            .and_then(|iso| reset_remaining(iso, now))
            .map(countdown_compact);
        Some(match cd {
            Some(c) => format!("⚠️ {} · {}", body, c),
            None => format!("⚠️ {}", body),
        })
    } else {
        Some(body)
    }
}

/// 聚合并列时的主窗口重置时刻（Unix 秒；无则 i64::MAX）
fn p_reset_ts(p: &ProviderStatus) -> i64 {
    p.quota_infos
        .as_ref()
        .and_then(primary_quota)
        .map(reset_timestamp)
        .unwrap_or(i64::MAX)
}
```

同时删除旧的全列表拼装代码（`code_counts` HashMap 段与 `parts.join` 段）及文件头对应 `worst_warning` 的 title 用法（`update_tray_badge` 仍用 `worst_warning`，保留该函数）。

- [x] **Step 4: 全量测试**

```bash
cd src-tauri && cargo fmt && cargo clippy -- -D warnings && cargo test
```
预期：全部通过。

- [x] **Step 5: Commit**

```bash
git add src-tauri/src/tray.rs
git commit -m "feat: 菜单栏常显文字改 V3 聚合态（最紧张一项 + ⚠️ 前缀 + 紧凑倒计时）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 6: 全量验证与收尾

**Files:**
- Modify: `docs/superpowers/specs/2026-10-31-tray-menu-plan-b-design.md`（状态行 → 已实施）
- Modify: `docs/superpowers/plans/2026-10-31-tray-menu-plan-b.md`（勾选完成项）

- [x] **Step 1: 全量验证**

```bash
cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test
npm run lint && npm run build
```
预期：全绿（前端未动，lint/build 应不受影响；build 含 tsc 类型检查）。

- [x] **Step 2: 手动验收（用户执行，macOS）**

`npm run tauri dev` 启动，对照 spec §9 清单验收：多窗口折叠、V3 四态、告警 ⚠️、轮询原地刷新、长菜单滚动。

- [x] **Step 3: spec 状态更新 + Commit**

```bash
git add docs/superpowers/
git commit -m "docs: 状态栏方案B spec 状态更新为已实施

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

## Self-Review 记录

- 覆盖检查：spec §4（reset_at）→ Task 1；§5.1 块结构/降级 → Task 3/4；§5.2 主窗口 → Task 3；§5.3 V3 → Task 5；§5.4 倒计时 → Task 2；§5.5 签名/句柄/50 上限 → Task 3/4；§5.6 阈值不动 → 无任务（约束）；§6 错误处理 → Task 3 降级分支 + Task 2 回退；§7 测试 → 各任务 Step 1；§9 验收 → Task 6。无缺口。
- 类型一致性：`primary_quota(&[QuotaInfo]) -> Option<&QuotaInfo>`（Task 3 定义，Task 5 消费，无 now 参数——比较用绝对时刻）；`quota_countdown_menu/compact(&QuotaInfo, DateTime<Utc>)`（Task 2 定义，Task 3 消费 menu 版）；`ProviderMenuBlock` 三字段全程一致。
- 已知取舍：Task 3 `provider_menu_block` 内联索引版并列比较与 `primary_quota` 重复一段逻辑（借用限制），由两侧测试锚定一致；Task 5 V3 告警仅看可见候选（语义决策已写入测试表格）。
