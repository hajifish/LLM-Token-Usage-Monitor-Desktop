//! 百炼官方 `bl` CLI 的控制台只读通道（Coding Plan / Token Plan 共用）。
//!
//! 控制台网关忽略 API-Key  header（现网实测真/假/无 Key 一律 ConsoleNeedLogin），
//! 唯一可用通道是 bl 的本机控制台登录态（`bl auth login --console` 一次性浏览器登录）。
//! 安全边界：只经 bl 自身读登录态，不接触/不落盘/不外传任何凭据。

use super::ProviderError;
use serde_json::Value;

/// shell 调 `bl usage <subcommand> --output json --quiet` 并解析 stdout。
pub(super) async fn usage_json(subcommand: &str) -> Result<Value, ProviderError> {
    let sub = subcommand.to_string();
    // bl 是 node CLI，启动约 1s；放 blocking 池避免阻塞 async runtime。
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("bl")
            .args(["usage", sub.as_str(), "--output", "json", "--quiet"])
            .output()
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
        let stderr = String::from_utf8_lossy(&output.stderr);
        let low = stderr.to_lowercase();
        if low.contains("console")
            || low.contains("login")
            || low.contains("auth")
            || stderr.contains("登录")
        {
            return Err(ProviderError(
                "百炼控制台未登录，请先运行 bl auth login --console".to_string(),
            ));
        }
        return Err(ProviderError(format!(
            "bl usage {} 失败: {}",
            subcommand,
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim())
        .map_err(|e| ProviderError(format!("bl 输出解析失败: {}", e)))
}
