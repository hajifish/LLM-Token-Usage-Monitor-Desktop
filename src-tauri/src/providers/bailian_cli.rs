//! 百炼官方 `bl` CLI 的控制台只读通道（Coding Plan / Token Plan 共用）。
//!
//! 控制台网关忽略 API-Key header（现网实测真/假/无 Key 一律 ConsoleNeedLogin），
//! 唯一可用通道是 bl 的本机控制台登录态（`bl auth login --console` 一次性浏览器登录）。
//! 安全边界：只经 bl 自身读登录态，不接触/不落盘/不外传任何凭据。

use super::ProviderError;
use serde_json::Value;

/// GUI/launchd 精简 PATH 不含 homebrew（同 secrets.rs ioreg 先例）：绝对路径优先，回退 PATH。
const BL_CANDIDATES: [&str; 3] = ["/opt/homebrew/bin/bl", "/usr/local/bin/bl", "bl"];

/// shell 调 `bl usage <subcommand> --output json --quiet` 并解析 stdout。
pub(super) async fn usage_json(subcommand: &str) -> Result<Value, ProviderError> {
    let sub = subcommand.to_string();
    // bl 是 node CLI，启动约 1s；放 blocking 池避免阻塞 async runtime。
    let output = tokio::task::spawn_blocking(move || {
        let args = ["usage", sub.as_str(), "--output", "json", "--quiet"];
        let mut not_found = None;
        for path in BL_CANDIDATES {
            match std::process::Command::new(path).args(args).output() {
                Ok(o) => return Ok(o),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => not_found = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(not_found.expect("BL_CANDIDATES 非空"))
    })
    .await
    .map_err(|e| ProviderError(format!("百炼查询任务失败: {}", e)))?
    .map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ProviderError(
                "未找到 bl CLI，请先安装百炼 CLI 并运行 bl auth login --console".to_string(),
            )
        } else {
            ProviderError(format!("调用 bl CLI 失败: {}", e))
        }
    })?;

    if !output.status.success() {
        return Err(map_bl_failure(
            subcommand,
            &String::from_utf8_lossy(&output.stderr),
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(stdout.trim())
        .map_err(|e| ProviderError(format!("bl 输出解析失败: {}", e)))?;
    // bl 个别路径以 exit 0 + stdout 错误对象返回（dry-run/网关异常），同口径映射
    if let Some(err) = json.get("error") {
        let msg = err.get("message").and_then(|v| v.as_str()).unwrap_or("");
        let hint = err.get("hint").and_then(|v| v.as_str()).unwrap_or("");
        return Err(map_bl_failure(subcommand, &format!("{} {}", msg, hint)));
    }
    Ok(json)
}

/// 失败文案映射：登录缺失给引导；其余提取人话（网络超时等 stderr 含 console 域名不算未登录）。
fn map_bl_failure(subcommand: &str, stderr: &str) -> ProviderError {
    let low = stderr.to_lowercase();
    if low.contains("auth login --console")
        || low.contains("console access token")
        || low.contains("consoleneedlogin")
        || stderr.contains("请登录")
        || stderr.contains("未登录")
    {
        return ProviderError("百炼控制台未登录，请先运行 bl auth login --console".to_string());
    }
    ProviderError(format!(
        "bl usage {} 失败: {}",
        subcommand,
        human_line(stderr)
    ))
}

/// stderr 首条人话：优先 error JSON 的 message+hint，否则跳过 node 警告行取首行，截断 200 字符。
fn human_line(stderr: &str) -> String {
    for line in stderr.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("(node:") || t.starts_with("(Use `node") {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(t) {
            if let Some(e) = v.get("error") {
                let m = e.get("message").and_then(|x| x.as_str()).unwrap_or("");
                let h = e.get("hint").and_then(|x| x.as_str()).unwrap_or("");
                let s = format!("{} {}", m, h).trim().to_string();
                if !s.is_empty() {
                    return truncate(&s);
                }
            }
        }
        return truncate(t);
    }
    "未知错误".to_string()
}

fn truncate(s: &str) -> String {
    if s.chars().count() > 200 {
        format!("{}…", s.chars().take(200).collect::<String>())
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 现网实测（隔离 HOME 探针）：未登录 exit 3，stderr 为 node 警告 + 错误 JSON。
    const NOT_LOGGED: &str = "(node:60558) [UNDICI-EHPA] Warning: EnvHttpProxyAgent is experimental, expect them to change at any time.\n(Use `node --trace-warnings ...` to show where the warning was created)\n{\"error\":{\"code\":3,\"message\":\"No console access token found.\",\"hint\":\"Run `bl auth login --console`.\"}}";

    #[test]
    fn not_logged_stderr_maps_to_login_guidance() {
        let err = map_bl_failure("token-plan", NOT_LOGGED);
        assert_eq!(err.0, "百炼控制台未登录，请先运行 bl auth login --console");
    }

    #[test]
    fn network_error_mentioning_console_domain_is_not_login() {
        let err = map_bl_failure(
            "token-plan",
            "fetch failed: connect ETIMEDOUT bailian.console.aliyun.com:443",
        );
        assert!(err.0.contains("bl usage token-plan 失败"), "got: {}", err.0);
        assert!(!err.0.contains("未登录"), "got: {}", err.0);
    }

    #[test]
    fn error_json_message_and_hint_extracted() {
        let err = map_bl_failure(
            "coding-plan",
            "{\"error\":{\"code\":9,\"message\":\"Quota query rejected\",\"hint\":\"retry later\"}}",
        );
        assert!(
            err.0.contains("Quota query rejected retry later"),
            "got: {}",
            err.0
        );
    }

    #[test]
    fn node_warnings_stripped_from_plain_stderr() {
        let err = map_bl_failure(
            "token-plan",
            "(node:1) [UNDICI-EHPA] Warning: experimental\n(Use `node --trace-warnings ...`)\nplain boom",
        );
        assert!(err.0.contains("plain boom"), "got: {}", err.0);
        assert!(!err.0.contains("UNDICI"), "got: {}", err.0);
    }

    #[test]
    fn long_stderr_truncated() {
        assert_eq!(truncate(&"x".repeat(500)).chars().count(), 201);
        let err = map_bl_failure("token-plan", &"x".repeat(500));
        assert!(err.0.ends_with('…'), "got: {}", err.0);
    }

    #[test]
    fn empty_stderr_falls_back() {
        assert!(map_bl_failure("token-plan", "").0.contains("未知错误"));
    }
}
