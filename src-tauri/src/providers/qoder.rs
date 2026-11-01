//! Qoder（阿里 Qwen 系 agentic IDE）订阅 Credits 配额供应商。
//!
//! 凭据两条路径（见 spec「v2 方案切换」）：
//! - **本地凭据（默认，api_key 为空）**：读取本机 Qoder / Qoder CN 桌面客户端
//!   的登录凭据 `auth.v1.dat`（Electron safeStorage：`v10` + AES-128-CBC，密钥
//!   在用户钥匙串 `<App> Safe Storage`），`dt-` 会话 token 直接作 Bearer；区域
//!   按凭据文件路径判定，不盲探。客户端每 60±5 分钟自刷 token，每轮重读即跟进。
//! - **PAT（兜底，api_key 非空）**：`pt-` 前缀先兑换短期 job token 再查询；
//!   国际/国内账号体系独立、凭据不通，鉴权类失败（401/403）或 404 切换端点重试。
//!
//! 接口形态来自社区逆向成果（CreditDaddy / qoder2api 实测可用），非官方文档
//! 承诺；服务端调整时本供应商显式报错，不静默给出错误数值。
//! 安全边界：只读用户自己的凭据文件与钥匙串项；token 不写日志、不落盘、不外传。

use super::{format_iso_reset, http_client, LlmProvider, ProviderError};
use crate::models::{BalanceData, QuotaInfo};
use aes::Aes128;
use async_trait::async_trait;
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use chrono::{DateTime, Utc};
use pbkdf2::pbkdf2_hmac;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use sha1::Sha1;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

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

fn to_provider_error(e: StepError, local: bool) -> ProviderError {
    match e {
        StepError::AuthDenied => ProviderError(if local {
            // dt- 会话 token 被拒 = 客户端登录态过期（客户端重登即可刷新）
            "Qoder 本地凭据已失效，请打开 Qoder / Qoder CN 客户端重新登录后重试".to_string()
        } else {
            "Qoder PAT 无效或已过期，请在 qoder.com / qoder.cn 控制台 Settings → Personal Access Tokens 重新创建"
                .to_string()
        }),
        StepError::Http(s) => ProviderError(format!("Qoder API error: {}", s)),
        StepError::Network(m) => ProviderError(format!("Qoder 请求失败: {}", m)),
        StepError::Parse(m) => {
            ProviderError(format!("Qoder 响应解析失败: {}（接口可能已调整）", m))
        }
    }
}

// --- 本地凭据层（auth.v1.dat，macOS Electron safeStorage） ---

/// security 子进程总超时：未授权钥匙串项会弹 macOS 授权框并阻塞到用户应答，
/// 超时后 kill 子进程再报错——blocking 线程不泄漏、弹窗进程不堆积（issue #89）
const SECURITY_QUERY_TIMEOUT: Duration = Duration::from_secs(60);
/// try_wait 轮询间隔
const SECURITY_POLL_INTERVAL: Duration = Duration::from_millis(50);
/// 钥匙串负缓存 TTL：被拒/空结果的查询静默 90s，过期重查（issue #90）。
/// 与调度间隔（默认 120s）同量级：既避免每轮重复弹窗，也让「之后才登录」
/// 或「改点允许」的用户无需重启应用即可恢复。
const KEYCHAIN_NEG_TTL: Duration = Duration::from_secs(90);

/// 钥匙串查询结果缓存条目
enum KeychainCacheEntry {
    /// 成功查到的密码：进程级永久缓存（Safe Storage 密码在客户端登录期内不变）
    Password(String),
    /// 负缓存：记录写入时刻，TTL 过期后重查
    Missed(Instant),
}

/// 钥匙串 Safe Storage 服务名 → 查询结果缓存：
/// - 成功（`Password`）：进程级永久缓存——用户在授权弹窗点「允许」后避免
///   每个调度轮（默认 120s）重复弹窗；
/// - 失败（`Missed` 负缓存）：只静默 TTL 90s，过期重查——弹窗被拒、超时、
///   或应用先装客户端后装的用户，都无需重启应用即可恢复（issue #90）。
///
/// 仅存内存，不落盘；重启应用后重新授权。
static KEYCHAIN_CACHE: OnceLock<Mutex<HashMap<String, KeychainCacheEntry>>> = OnceLock::new();

/// 读干子进程管道输出（输出量级很小，且进程已退出后读取不会阻塞）
fn read_pipe_output(pipe: Option<impl Read>) -> String {
    let mut buf = String::new();
    if let Some(mut p) = pipe {
        let _ = p.read_to_string(&mut buf);
    }
    buf
}

/// `security find-generic-password -w`（与 CreditDaddy 同查询；绝对路径防 PATH
/// 劫持，issue #97）。阻塞模型（issue #89）：未授权项会弹 macOS 授权框并阻塞
/// 到用户应答，故不走 `.output()` 死等，而是 spawn 后以 50ms 间隔轮询
/// `try_wait`，总超时 60s，超时 `kill()` 子进程再报错。
/// 返回：`Ok(Some)` 密码；`Ok(None)` 查不到（项不存在/被拒/空密码，均留 warn
/// 日志）；`Err` 授权等待超时（文案直接面向用户，调用方短路直报）。
fn security_find_generic_password(service: &str) -> Result<Option<String>, String> {
    let mut child = match Command::new("/usr/bin/security")
        .args(["find-generic-password", "-w", "-s", service])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Qoder 钥匙串查询无法启动 /usr/bin/security: {e}");
            return Ok(None);
        }
    };
    let deadline = Instant::now() + SECURITY_QUERY_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    break None;
                }
                std::thread::sleep(SECURITY_POLL_INTERVAL);
            }
            Err(e) => {
                log::warn!("Qoder 钥匙串查询 security 子进程状态不可读: {e}");
                let _ = child.kill();
                let _ = child.wait();
                return Ok(None);
            }
        }
    };
    let Some(status) = status else {
        // 超时：kill 后 reap，授权弹窗进程随之回收，不留悬挂进程
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!(
            "Qoder 钥匙串授权等待超时（{}s），请重试或在弹窗中点击允许",
            SECURITY_QUERY_TIMEOUT.as_secs()
        ));
    };
    if !status.success() {
        // 只留服务名与失败原因（stderr 首行，如 errAuthorizationDenied /
        // item not found），不含任何密码/token（issue #91）
        let stderr = read_pipe_output(child.stderr.take());
        log::warn!(
            "Qoder 钥匙串项 {service} 查询失败（{status}）: {}",
            stderr.lines().next().unwrap_or("")
        );
        return Ok(None);
    }
    let pw = read_pipe_output(child.stdout.take());
    let pw = pw.trim().to_string();
    if pw.is_empty() {
        log::warn!("Qoder 钥匙串项 {service} 返回空密码，按查询失败处理");
        return Ok(None);
    }
    Ok(Some(pw))
}

/// 服务名 → 密码（含缓存）。命中缓存直接返回；未命中才走子进程查询，
/// 且查询期间不持锁——一次授权等待可长达 60s，持锁会串行阻塞其他查询。
/// `Err` 仅在授权等待超时时出现（不入缓存：下一轮调度重新弹窗给用户应答机会）。
fn keychain_password(service: &str) -> Result<Option<String>, String> {
    let cache = KEYCHAIN_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(hit) = cache.lock().expect("keychain cache poisoned").get(service) {
        match hit {
            KeychainCacheEntry::Password(pw) => return Ok(Some(pw.clone())),
            // 负缓存 TTL 内静默：弹窗被拒后不每轮重复弹
            KeychainCacheEntry::Missed(at) if at.elapsed() < KEYCHAIN_NEG_TTL => {
                return Ok(None);
            }
            // 负缓存过期：走真实重查
            KeychainCacheEntry::Missed(_) => {}
        }
    }
    let pw = security_find_generic_password(service)?;
    let entry = match &pw {
        Some(p) => KeychainCacheEntry::Password(p.clone()),
        None => KeychainCacheEntry::Missed(Instant::now()),
    };
    cache
        .lock()
        .expect("keychain cache poisoned")
        .insert(service.to_string(), entry);
    Ok(pw)
}

type Aes128CbcDec = cbc::Decryptor<Aes128>;

/// 解密失败分类（issue #92）：决定「换一把钥匙」是否有意义
#[derive(Debug, PartialEq, Eq)]
enum DecryptError {
    /// 与钥匙无关（未知加密格式前缀 / 文件过小）：换钥匙串名无意义，
    /// 调用方应短路并把它作为最终错误上报
    Format(String),
    /// 与钥匙相关（PKCS7 去填充失败 / 非 UTF-8）：换下一把钥匙继续试
    KeyMismatch(String),
}

/// 解密 auth.v1.dat：`v10` + AES-128-CBC（IV = 16 个空格），
/// 密钥 = PBKDF2-SHA1(密码, "saltysalt", 1003 轮, 16 字节)，PKCS7 填充。
/// 配方来自 CreditDaddy（qoderApp.js safeStorageCodec），与客户端实测一致。
fn decrypt_auth_dat(data: &[u8], password: &str) -> Result<String, DecryptError> {
    if data.len() < 3 + 16 {
        return Err(DecryptError::Format(format!(
            "文件过小（{} 字节）",
            data.len()
        )));
    }
    if &data[..3] != b"v10" {
        return Err(DecryptError::Format(format!(
            "未知加密格式前缀 {:?}",
            String::from_utf8_lossy(&data[..3])
        )));
    }
    let mut key = [0u8; 16];
    pbkdf2_hmac::<Sha1>(password.as_bytes(), b"saltysalt", 1003, &mut key);
    let iv = [0x20u8; 16];
    let mut buf = data[3..].to_vec();
    let dec = Aes128CbcDec::new_from_slices(&key, &iv)
        .map_err(|e| DecryptError::KeyMismatch(format!("密钥/IV 长度错误: {e}")))?;
    let plain = dec.decrypt_padded_mut::<Pkcs7>(&mut buf).map_err(|e| {
        DecryptError::KeyMismatch(format!("PKCS7 去填充失败（密钥不匹配或数据损坏）: {e}"))
    })?;
    String::from_utf8(plain.to_vec())
        .map_err(|_| DecryptError::KeyMismatch("解密结果非 UTF-8".to_string()))
}

/// 从解密后的 JSON 提取会话 token（dt-…）；只取 token，其余字段不外泄
fn parse_auth_token(plain: &str) -> Option<String> {
    let obj: Value = serde_json::from_str(plain).ok()?;
    let token = obj.get("token")?.as_str()?.trim();
    (!token.is_empty()).then(|| token.to_string())
}

/// 一份客户端登录凭据：文件路径 + 由此判定的区域 base + 钥匙串服务名候选
struct AuthCandidate {
    path: PathBuf,
    base: &'static str,
    keychain_names: &'static [&'static str],
}

/// 扫描 userData 目录下的客户端凭据候选（抽目录参数便于测试）。
/// `com.qoder.app*` → 国际、`com.qodercn.app*` → 国内；同区域多版本目录
/// 与跨区域候选混排，按 mtime 降序（最近活跃登录优先）。
///
/// 钥匙串服务名候选按真机实测排序：本机同存 `Qoder CN App` / `Qoder CN` /
/// `Qoder` 三个 Safe Storage 项（登录壳 / IDE / 国际版各一把钥匙），只有
/// `<Electron app.name> Safe Storage` 才是加密对应 auth.v1.dat 的那把；
/// 候选只是顺序，最终以「解密成功」为准（见 read_local_credential）。
fn auth_candidates_under(app_support: &Path) -> Vec<AuthCandidate> {
    const CN_KEYCHAIN: &[&str] = &["Qoder CN App", "Qoder CN", "Qoder"];
    const GLOBAL_KEYCHAIN: &[&str] = &["Qoder App", "Qoder", "Qoder CN App"];
    let mut out: Vec<AuthCandidate> = Vec::new();
    let Ok(entries) = std::fs::read_dir(app_support) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let (base, keychain_names) = if name.starts_with("com.qodercn.app") {
            (CN_API_BASE, CN_KEYCHAIN)
        } else if name.starts_with("com.qoder.app") {
            (GLOBAL_API_BASE, GLOBAL_KEYCHAIN)
        } else {
            continue;
        };
        let path = entry.path().join("auth.v1.dat");
        if !path.is_file() {
            continue;
        }
        out.push(AuthCandidate {
            path,
            base,
            keychain_names,
        });
    }
    out.sort_by_key(|c| {
        std::cmp::Reverse(
            c.path
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .unwrap_or(std::time::UNIX_EPOCH),
        )
    });
    out
}

fn auth_candidates() -> Vec<AuthCandidate> {
    dirs::home_dir()
        .map(|h| auth_candidates_under(&h.join("Library/Application Support")))
        .unwrap_or_default()
}

/// 候选遍历中的失败留痕（issue #92）：全部组合失败时只上报信息量最高的一类，
/// 而非「最后一个」。优先级：Format（短路直报，不参与比较）> 钥匙串不可用
/// > 钥匙不匹配 > 缺 token > 文件读取失败。
#[derive(Default)]
enum FailureNote {
    /// 候选为空（客户端未安装）
    #[default]
    None,
    /// 文件读取失败（权限/损坏）
    FileRead(String),
    /// 解密成功但内容缺 token
    MissingToken(String),
    /// 所有拿到的钥匙都解不开本文件
    KeyMismatch(String),
    /// 钥匙串查询不可用（弹窗被拒/项不存在）
    KeychainUnavailable(String),
}

impl FailureNote {
    fn rank(&self) -> u8 {
        match self {
            FailureNote::None => 0,
            FailureNote::FileRead(_) => 1,
            FailureNote::MissingToken(_) => 2,
            FailureNote::KeyMismatch(_) => 3,
            FailureNote::KeychainUnavailable(_) => 4,
        }
    }

    /// 只保留信息量更高者；同级保留先出现者（先出现的候选 mtime 更新、更相关）
    fn record(&mut self, next: Self) {
        if next.rank() > self.rank() {
            *self = next;
        }
    }

    fn into_provider_error(self) -> ProviderError {
        match self {
            FailureNote::None => ProviderError(
                "未找到 Qoder 客户端登录凭据（auth.v1.dat），请先安装并登录 Qoder / Qoder CN 桌面客户端"
                    .to_string(),
            ),
            FailureNote::FileRead(msg)
            | FailureNote::MissingToken(msg)
            | FailureNote::KeyMismatch(msg)
            | FailureNote::KeychainUnavailable(msg) => ProviderError(msg),
        }
    }
}

/// 钥匙串整体不可用（弹窗被拒/项不存在/空密码）的最终文案。
/// #90：指引不再死胡同——点不了允许的话，明说重启应用才是出路。
const KEYCHAIN_UNAVAILABLE_MSG: &str = "无法读取 Qoder 钥匙串密钥（Safe Storage），请在 macOS 授权弹窗中点击允许（已暂时跳过后续弹窗，若始终失败请重启本应用）";

/// 多钥匙试解决策核心（可注入，issue #100）：候选列表 × 钥匙串名 × 解密判定
/// × last_err 决策全部收敛在此纯逻辑，真实钥匙串查找由 `read_local_credential`
/// 注入，测试可注入 fake 覆盖各失败组合。
/// - `lookup`：服务名 → 密码。`Ok(None)` = 项不存在/被拒/空密码（换下一个候选名）；
///   `Err` = 授权等待超时（用户多半不在场，继续换名只会连环弹窗，短路整轮直报）
/// - `decrypt`：密文 + 密码 → 明文。`Format`（与钥匙无关）短路整轮直报；
///   `KeyMismatch` 换下一把钥匙
fn resolve_local_credential_with(
    candidates: &[AuthCandidate],
    mut lookup: impl FnMut(&str) -> Result<Option<String>, String>,
    decrypt: impl Fn(&[u8], &str) -> Result<String, DecryptError>,
) -> Result<(String, &'static str), ProviderError> {
    let mut note = FailureNote::None;
    for c in candidates {
        let data = match std::fs::read(&c.path) {
            Ok(d) => d,
            Err(e) => {
                log::debug!(
                    "Qoder 候选凭据 {} 读取失败（FileRead），跳过该候选: {e}",
                    c.path.display()
                );
                note.record(FailureNote::FileRead(format!(
                    "读取 {} 失败: {e}",
                    c.path.display()
                )));
                continue;
            }
        };
        let path = c.path.display().to_string();
        for name in c.keychain_names {
            let service = format!("{name} Safe Storage");
            let password = match lookup(&service) {
                Ok(Some(pw)) => pw,
                Ok(None) => {
                    log::debug!(
                        "Qoder 候选 {path} × 钥匙串 {service} 不可用（KeychainUnavailable），换下一个候选名"
                    );
                    note.record(FailureNote::KeychainUnavailable(
                        KEYCHAIN_UNAVAILABLE_MSG.to_string(),
                    ));
                    continue;
                }
                Err(timeout_msg) => {
                    log::debug!(
                        "Qoder 候选 {path} × 钥匙串 {service} 授权等待超时，短路整轮不再弹窗"
                    );
                    return Err(ProviderError(timeout_msg));
                }
            };
            let plain = match decrypt(&data, &password) {
                Ok(plain) => plain,
                Err(DecryptError::Format(msg)) => {
                    // 与钥匙无关的格式问题：试其余钥匙串名没有意义，
                    // 短路并直接上报真实原因，不误导用户去查钥匙串（issue #92）
                    log::debug!(
                        "Qoder 候选 {path} × 钥匙串 {service} 格式不符（Format），短路整轮: {msg}"
                    );
                    return Err(ProviderError(format!(
                        "Qoder 本地凭据格式无法识别（{msg}），客户端加密格式可能已变更，需等待本应用更新跟进"
                    )));
                }
                Err(DecryptError::KeyMismatch(msg)) => {
                    // 该钥匙串项不是加密本文件的密钥，换下一个候选名
                    log::debug!(
                        "Qoder 候选 {path} × 钥匙串 {service} 钥匙不匹配（KeyMismatch），换下一个候选名: {msg}"
                    );
                    note.record(FailureNote::KeyMismatch(
                        "Qoder 本地凭据解密失败（钥匙串密钥均不匹配），客户端存储格式可能已变更"
                            .to_string(),
                    ));
                    continue;
                }
            };
            match parse_auth_token(&plain) {
                Some(t) => return Ok((t, c.base)),
                None => {
                    // 解密成功 = 这把钥匙就是对的，只是内容缺 token：
                    // 同候选其余名字只会产生无关噪音，直接续下一候选
                    log::debug!(
                        "Qoder 候选 {path} × 钥匙串 {service} 解密成功但缺 token（MissingToken），换下一个候选"
                    );
                    note.record(FailureNote::MissingToken(
                        "Qoder 本地凭据解析失败（解密内容缺 token），客户端存储格式可能已变更"
                            .to_string(),
                    ));
                    break;
                }
            }
        }
    }
    Err(note.into_provider_error())
}

/// 读取本地凭据：候选按 mtime 降序 × 钥匙串名候选逐组合试，
/// **以「解密成功且含 token」为准**——机器上同存多个 Qoder 系 Safe Storage
/// 项（登录壳 / IDE / 国际版），先拿到的密码未必是加密本文件的那把。
/// 首次命中未授权项触发钥匙串授权弹窗（最长等 60s）；成功密码进程内永久
/// 缓存，失败走 90s 负缓存。
/// 同步签名（文件 IO + 子进程 + 解密全程阻塞）便于测试；async 调用方必须包
/// `spawn_blocking`（见 `fetch_snapshot`），避免冻结调度数据面（issue #89）。
fn read_local_credential() -> Result<(String, &'static str), ProviderError> {
    let candidates = auth_candidates();
    resolve_local_credential_with(&candidates, keychain_password, decrypt_auth_dat)
}

/// 轮内快照：fetch_balance 与 fetch_quota_infos 共享一次网络链路
#[derive(Clone)]
struct QoderSnapshot {
    rows: Vec<QuotaInfo>,
}

pub struct QoderProvider {
    client: Client,
    api_key: String,
    /// true = 本地凭据模式（api_key 为空）：决定 AuthDenied 的错误文案口径
    local: bool,
    cached: tokio::sync::OnceCell<QoderSnapshot>,
}

impl QoderProvider {
    pub fn new(api_key: &str) -> Self {
        let api_key = api_key.trim().to_string();
        let local = api_key.is_empty();
        Self {
            client: http_client(),
            api_key,
            local,
            cached: tokio::sync::OnceCell::new(),
        }
    }

    async fn snapshot(&self) -> Result<&QoderSnapshot, ProviderError> {
        self.cached
            .get_or_try_init(|| async { self.fetch_snapshot().await })
            .await
    }

    /// 本地凭据模式：按凭据文件来源直连对应区域（不盲探）。
    /// PAT 模式：先国际后国内；国际失败且可切换时重试国内，否则直接短路。
    /// 不缓存「区域归属」：每轮最多多一组请求（默认 120s 间隔），换取无全局状态。
    async fn fetch_snapshot(&self) -> Result<QoderSnapshot, ProviderError> {
        if self.local {
            // 每轮重读文件：客户端每 60±5 分钟自刷 token，重读即自动跟进。
            // 文件 IO + security 子进程（授权弹窗最长等 60s）+ 解密全程阻塞，
            // 丢进 blocking 线程池执行，不占死 async worker、不冻结调度数据面
            //（issue #89）
            let (token, base) = tokio::task::spawn_blocking(read_local_credential)
                .await
                .map_err(|e| ProviderError(format!("Qoder 本地凭据读取任务异常终止: {e}")))??;
            let rows = self
                .fetch_from_base(base, &token)
                .await
                .map_err(|e| to_provider_error(e, self.local))?;
            return Ok(QoderSnapshot { rows });
        }
        match self.fetch_from_base(GLOBAL_API_BASE, &self.api_key).await {
            Ok(rows) => Ok(QoderSnapshot { rows }),
            Err(e) if should_switch_region(&e) => {
                let rows = self
                    .fetch_from_base(CN_API_BASE, &self.api_key)
                    .await
                    .map_err(|e| to_provider_error(e, self.local))?;
                Ok(QoderSnapshot { rows })
            }
            Err(e) => Err(to_provider_error(e, self.local)),
        }
    }

    /// 在指定区域完成 token 解析（pt- 兑换）→ 配额查询 → 计划名（尽力）→ 行映射
    async fn fetch_from_base(
        &self,
        base: &str,
        raw_token: &str,
    ) -> Result<Vec<QuotaInfo>, StepError> {
        let token = self.resolve_token(base, raw_token).await?;
        let resp = self.fetch_quota_usage(base, &token).await?;
        let plan = self.fetch_plan_label(base, &token).await;
        Ok(build_quota_rows(&resp, plan.as_deref()))
    }

    /// `pt-` 个人令牌先兑换短期 job token；其余原样使用（本地 dt- 会话 token、jt 联调）。
    async fn resolve_token(&self, base: &str, raw_token: &str) -> Result<String, StepError> {
        if !raw_token.starts_with("pt-") {
            return Ok(raw_token.to_string());
        }
        let url = format!("{}/api/v1/jobToken/exchange", base);
        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("User-Agent", "qodercli/1.0.0")
            .header("Cosy-Version", "1.0.0")
            .header("Cosy-ClientType", "5")
            .body(serde_json::json!({ "personal_token": raw_token }).to_string())
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
            .header("Cosy-Version", "0.3.3")
            .header("Cosy-MachineOS", std::env::consts::OS)
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
            .header("Cosy-Version", "0.3.3")
            .header("Cosy-MachineOS", std::env::consts::OS)
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
        let msg = to_provider_error(StepError::AuthDenied, false).0;
        assert!(msg.contains("PAT"), "got: {msg}");
        assert!(msg.contains("Personal Access Tokens"), "got: {msg}");
    }

    #[test]
    fn local_auth_error_points_to_client_relogin() {
        let msg = to_provider_error(StepError::AuthDenied, true).0;
        assert!(msg.contains("本地凭据"), "got: {msg}");
        assert!(msg.contains("重新登录"), "got: {msg}");
    }

    #[test]
    fn http_error_keeps_status_and_parse_mentions_adjustment() {
        assert_eq!(
            to_provider_error(StepError::Http(500), false).0,
            "Qoder API error: 500"
        );
        assert!(to_provider_error(StepError::Parse("x".to_string()), false)
            .0
            .contains("接口可能已调整"));
        assert!(
            to_provider_error(StepError::Network("e".to_string()), false)
                .0
                .contains("请求失败")
        );
    }

    // ---- 本地凭据层（auth.v1.dat 解密 / 候选扫描） ----

    /// 造一份 auth.v1.dat 密文（与客户端同算法：v10 + AES-128-CBC + PKCS7）
    fn sealed_auth_dat(plain: &str, password: &str) -> Vec<u8> {
        sealed_auth_dat_bytes(plain.as_bytes(), password)
    }

    /// 同上，明文可为任意字节（覆盖「解密成功但非 UTF-8」分类）
    fn sealed_auth_dat_bytes(plain: &[u8], password: &str) -> Vec<u8> {
        use cbc::cipher::BlockEncryptMut;
        type Aes128CbcEnc = cbc::Encryptor<Aes128>;
        let mut key = [0u8; 16];
        pbkdf2_hmac::<Sha1>(password.as_bytes(), b"saltysalt", 1003, &mut key);
        let iv = [0x20u8; 16];
        let enc = Aes128CbcEnc::new_from_slices(&key, &iv).unwrap();
        let msg_len = plain.len();
        let mut buf = plain.to_vec();
        buf.resize(msg_len + 16, 0); // 预留 PKCS7 填充块
        let ct = enc.encrypt_padded_mut::<Pkcs7>(&mut buf, msg_len).unwrap();
        let mut data = b"v10".to_vec();
        data.extend_from_slice(ct);
        data
    }

    /// tempdir（无 tempfile dev-dep：手写唯一目录，Drop 时清理）
    struct ScratchDir(PathBuf);
    impl ScratchDir {
        fn new(tag: &str) -> Self {
            let p =
                std::env::temp_dir().join(format!("llmtm-qoder-test-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            ScratchDir(p)
        }
    }
    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn touch(path: &Path, modified: std::time::SystemTime) {
        std::fs::File::options()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
    }

    #[test]
    fn decrypt_auth_dat_roundtrips_synthetic_client_format() {
        let plain = r#"{"token":"dt-abcdef1234567890abcdef","refreshToken":"rt-x","expiresAt":"2026-11-01T00:00:00Z","user":{"id":"u1"}}"#;
        let data = sealed_auth_dat(plain, "keychain-secret");
        assert_eq!(decrypt_auth_dat(&data, "keychain-secret").unwrap(), plain);
    }

    #[test]
    fn decrypt_auth_dat_rejects_wrong_key_and_unknown_prefix() {
        let data = sealed_auth_dat(r#"{"token":"dt-x"}"#, "right");
        let err = decrypt_auth_dat(&data, "wrong").unwrap_err();
        assert!(
            matches!(err, DecryptError::KeyMismatch(ref m) if m.contains("PKCS7")),
            "got: {err:?}"
        );
        let mut v11 = sealed_auth_dat(r#"{"token":"dt-x"}"#, "k");
        v11[..3].copy_from_slice(b"v11");
        let err = decrypt_auth_dat(&v11, "k").unwrap_err();
        assert!(
            matches!(err, DecryptError::Format(ref m) if m.contains("未知加密格式")),
            "got: {err:?}"
        );
        let err = decrypt_auth_dat(b"v1", "k").unwrap_err();
        assert!(
            matches!(err, DecryptError::Format(ref m) if m.contains("文件过小")),
            "got: {err:?}"
        );
    }

    #[test]
    fn decrypt_auth_dat_classifies_non_utf8_plaintext_as_key_mismatch() {
        let data = sealed_auth_dat_bytes(&[0xff, 0xfe, 0x00, 0x01], "k");
        let err = decrypt_auth_dat(&data, "k").unwrap_err();
        assert!(
            matches!(err, DecryptError::KeyMismatch(ref m) if m.contains("UTF-8")),
            "got: {err:?}"
        );
    }

    #[test]
    fn parse_auth_token_extracts_non_empty_token_only() {
        assert_eq!(
            parse_auth_token(r#"{"token":"dt-abc123","refreshToken":"rt","user":{"name":"n"}}"#),
            Some("dt-abc123".to_string())
        );
        assert_eq!(parse_auth_token(r#"{"refreshToken":"rt"}"#), None);
        assert_eq!(parse_auth_token(r#"{"token":""}"#), None);
        assert_eq!(parse_auth_token("not json"), None);
    }

    #[test]
    fn auth_candidates_under_map_regions_and_prefer_recent_login() {
        let scratch = ScratchDir::new("cand");
        let cn = scratch.0.join("com.qodercn.app.stable");
        let intl = scratch.0.join("com.qoder.app");
        std::fs::create_dir_all(&cn).unwrap();
        std::fs::create_dir_all(&intl).unwrap();
        std::fs::create_dir_all(scratch.0.join("Unrelated.app")).unwrap();
        let base = std::time::SystemTime::UNIX_EPOCH;
        touch(
            &cn.join("auth.v1.dat"),
            base + std::time::Duration::from_secs(1000),
        );
        touch(
            &intl.join("auth.v1.dat"),
            base + std::time::Duration::from_secs(5000),
        );

        let cands = auth_candidates_under(&scratch.0);
        assert_eq!(cands.len(), 2, "无关目录与缺失 auth.v1.dat 的目录要跳过");
        assert_eq!(cands[0].base, GLOBAL_API_BASE, "mtime 新者在前");
        assert_eq!(
            cands[0].keychain_names,
            ["Qoder App", "Qoder", "Qoder CN App"]
        );
        assert_eq!(cands[1].base, CN_API_BASE);
        assert_eq!(
            cands[1].keychain_names,
            ["Qoder CN App", "Qoder CN", "Qoder"]
        );
    }

    #[test]
    fn auth_candidates_under_skip_dirs_without_auth_file_and_missing_root() {
        let scratch = ScratchDir::new("skip");
        std::fs::create_dir_all(scratch.0.join("com.qodercn.app.stable")).unwrap();
        assert!(auth_candidates_under(&scratch.0).is_empty());
        assert!(auth_candidates_under(&scratch.0.join("no-such-dir")).is_empty());
    }

    // ---- 多钥匙试解决策（可注入纯逻辑，issue #100） ----

    fn candidate_at(
        path: PathBuf,
        base: &'static str,
        names: &'static [&'static str],
    ) -> AuthCandidate {
        AuthCandidate {
            path,
            base,
            keychain_names: names,
        }
    }

    /// (a) 某候选解密成功但缺 token：不再试同候选其余钥匙串名（钥匙已对，
    /// 余下只是噪音），直接续下一候选并成功返回
    #[test]
    fn resolve_continues_to_next_candidate_when_decrypted_content_lacks_token() {
        let scratch = ScratchDir::new("resolve-token");
        let c1 = scratch.0.join("c1-auth.dat");
        std::fs::write(&c1, sealed_auth_dat(r#"{"refreshToken":"rt"}"#, "k1")).unwrap();
        let c2 = scratch.0.join("c2-auth.dat");
        std::fs::write(&c2, sealed_auth_dat(r#"{"token":"dt-good"}"#, "k2")).unwrap();
        let candidates = vec![
            candidate_at(c1, CN_API_BASE, &["A1", "A2"]),
            candidate_at(c2, GLOBAL_API_BASE, &["B1"]),
        ];
        let mut queries: Vec<String> = Vec::new();
        let result = resolve_local_credential_with(
            &candidates,
            |service| {
                queries.push(service.to_string());
                Ok(Some(match service {
                    "A1 Safe Storage" => "k1".to_string(),
                    "B1 Safe Storage" => "k2".to_string(),
                    _ => return Ok(None),
                }))
            },
            decrypt_auth_dat,
        )
        .unwrap();
        assert_eq!(result, ("dt-good".to_string(), GLOBAL_API_BASE));
        assert_eq!(queries, ["A1 Safe Storage", "B1 Safe Storage"]);
    }

    /// (b) Format 类错误（未知加密格式前缀，与钥匙无关）：只试第一把钥匙即
    /// 短路整轮，该错误直接作为最终错误，不再误报「钥匙串不匹配」
    #[test]
    fn resolve_short_circuits_on_format_error() {
        let scratch = ScratchDir::new("resolve-format");
        let mut v11 = sealed_auth_dat(r#"{"token":"dt-x"}"#, "k1");
        v11[..3].copy_from_slice(b"v11");
        let c1 = scratch.0.join("c1-auth.dat");
        std::fs::write(&c1, v11).unwrap();
        let c2 = scratch.0.join("c2-auth.dat");
        std::fs::write(&c2, sealed_auth_dat(r#"{"token":"dt-good"}"#, "k2")).unwrap();
        let candidates = vec![
            candidate_at(c1, CN_API_BASE, &["A1", "A2"]),
            candidate_at(c2, GLOBAL_API_BASE, &["B1"]),
        ];
        let mut queries: Vec<String> = Vec::new();
        let err = resolve_local_credential_with(
            &candidates,
            |service| {
                queries.push(service.to_string());
                Ok(Some("k1".to_string()))
            },
            decrypt_auth_dat,
        )
        .unwrap_err();
        assert!(err.0.contains("格式无法识别"), "got: {}", err.0);
        assert!(err.0.contains("未知加密格式"), "got: {}", err.0);
        assert_eq!(
            queries,
            ["A1 Safe Storage"],
            "短路：其余钥匙串名与候选不再试"
        );
    }

    /// (c) 全组合失败：last_err 取信息量最高的一类（钥匙串不可用 > 钥匙不匹配
    /// > 文件读取失败），而非最后发生的那个
    #[test]
    fn resolve_reports_most_informative_failure_when_all_combinations_fail() {
        let scratch = ScratchDir::new("resolve-priority");
        // 候选 1：文件读取失败（路径指向目录）
        let c1 = candidate_at(scratch.0.clone(), CN_API_BASE, &["A1"]);
        // 候选 2：文件完好但钥匙串查询被拒
        let c2_path = scratch.0.join("c2-auth.dat");
        std::fs::write(&c2_path, sealed_auth_dat(r#"{"token":"dt-x"}"#, "k2")).unwrap();
        let c2 = candidate_at(c2_path, GLOBAL_API_BASE, &["B1"]);
        // 候选 3：拿得到密码但解不开（PKCS7）
        let c3_path = scratch.0.join("c3-auth.dat");
        std::fs::write(&c3_path, sealed_auth_dat(r#"{"token":"dt-x"}"#, "k3")).unwrap();
        let c3 = candidate_at(c3_path, GLOBAL_API_BASE, &["C1"]);
        let candidates = vec![c1, c2, c3];

        let err = resolve_local_credential_with(
            &candidates,
            |service| {
                Ok(match service {
                    "C1 Safe Storage" => Some("wrong-key".to_string()),
                    _ => None,
                })
            },
            decrypt_auth_dat,
        )
        .unwrap_err();
        assert!(
            err.0.contains("授权弹窗") && err.0.contains("重启本应用"),
            "应取「钥匙串不可用」而非后发生的 KeyMismatch，got: {}",
            err.0
        );
    }

    /// 授权等待超时（lookup Err）：短路整轮直接上报超时文案，不连环弹窗
    #[test]
    fn resolve_short_circuits_when_keychain_authorization_times_out() {
        let scratch = ScratchDir::new("resolve-timeout");
        let path = scratch.0.join("auth.dat");
        std::fs::write(&path, sealed_auth_dat(r#"{"token":"dt-x"}"#, "k")).unwrap();
        let candidates = vec![candidate_at(path, CN_API_BASE, &["A1", "A2"])];
        let mut queries: Vec<String> = Vec::new();
        let err = resolve_local_credential_with(
            &candidates,
            |service| {
                queries.push(service.to_string());
                Err("Qoder 钥匙串授权等待超时（60s），请重试或在弹窗中点击允许".to_string())
            },
            decrypt_auth_dat,
        )
        .unwrap_err();
        assert!(err.0.contains("超时（60s）"), "got: {}", err.0);
        assert_eq!(queries.len(), 1, "超时后不再对后续钥匙串名连环弹窗");
    }

    /// 无候选（客户端未安装）：专门的「未找到」文案而非空口径
    #[test]
    fn resolve_without_candidates_reports_missing_client() {
        let err = resolve_local_credential_with(&[], |_| Ok(None), decrypt_auth_dat).unwrap_err();
        assert!(
            err.0.contains("未找到 Qoder 客户端登录凭据"),
            "got: {}",
            err.0
        );
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
