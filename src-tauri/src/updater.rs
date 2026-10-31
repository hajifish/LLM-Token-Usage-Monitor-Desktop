//! 应用内自动更新（检查 / 下载 / 安装）。
//!
//! 设计要点：
//! - 检查、下载、安装全部在 Rust 侧完成：主窗口隐藏时 WKWebView 会节流 JS 定时器，
//!   自动检查与下载进度不能依赖前端计时器；前端只订阅 `update-status` 事件做展示。
//! - 两段式安装：先下载（`download()` 内部完成 minisign 签名校验），再由用户确认安装并重启；
//! - 安全边界是 tauri.conf.json 中配置的公钥：签名不匹配的更新包在下载阶段即被拒绝，
//!   永远到不了安装阶段；
//! - 「自动检查开关」写入 AppConfig，复用 config::save 的加密与备份守卫。

use crate::models::AppConfig;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;
use tokio::sync::RwLock as AsyncRwLock;

/// 启动后首次自动检查的延迟（避开启动时的供应商轮询与网络拥塞）。
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(60);
/// 自动检查周期。
const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// 下载进度事件的最小发送间隔（高频 IPC 会拖慢下载）。
const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(200);

/// 更新阶段 —— 与前端 `src/types.ts` 的 `UpdatePhase` 严格一致（snake_case）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePhase {
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Ready,
    Installing,
    Failed,
}

impl UpdatePhase {
    /// 检查 / 下载 / 安装进行中：此时不应发起新的检查。
    fn is_busy(self) -> bool {
        matches!(
            self,
            UpdatePhase::Checking | UpdatePhase::Downloading | UpdatePhase::Installing
        )
    }
}

/// 更新状态快照 —— 与前端 `src/types.ts` 的 `UpdateStatus` 严格一致（snake_case）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateStatus {
    pub phase: UpdatePhase,
    pub current_version: String,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub date: Option<String>,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub message: Option<String>,
    pub last_checked: Option<String>,
}

impl UpdateStatus {
    fn initial(current_version: String) -> Self {
        Self {
            phase: UpdatePhase::Idle,
            current_version,
            version: None,
            notes: None,
            date: None,
            downloaded: 0,
            total: None,
            message: None,
            last_checked: None,
        }
    }
}

/// 更新模块的托管状态。
///
/// - `status` 用 std `RwLock`：托盘菜单与 tooltip 需要在同步上下文读取；
/// - 锁内只读写结构体字段，绝不跨 await 持有，也绝不在持锁期间 emit
///   （emit 的接收侧与托盘重建都会再读同一把锁）；
/// - `pending` 保存「已下载且已通过签名校验」的更新包，只有安装命令会取走。
pub struct UpdateState {
    status: RwLock<UpdateStatus>,
    pending: Mutex<Option<(String, Vec<u8>)>>,
}

impl UpdateState {
    pub fn new(current_version: String) -> Self {
        Self {
            status: RwLock::new(UpdateStatus::initial(current_version)),
            pending: Mutex::new(None),
        }
    }
}

fn lock_read<T>(lock: &RwLock<T>) -> T
where
    T: Clone,
{
    lock.read().unwrap_or_else(|e| e.into_inner()).clone()
}

fn lock_write<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|e| e.into_inner())
}

/// 当前状态快照；状态未托管时兜底返回初始快照（理论不会发生）。
pub fn snapshot(app: &AppHandle) -> UpdateStatus {
    match app.try_state::<UpdateState>() {
        Some(state) => lock_read(&state.status),
        None => UpdateStatus::initial(app.package_info().version.to_string()),
    }
}

/// 只推送事件（下载进度等高频变更使用，不重建托盘菜单）。
fn emit_status(app: &AppHandle) {
    let _ = app.emit("update-status", &snapshot(app));
}

/// 变更状态：闭包内只允许改字段；释放锁后再广播事件并原地刷新托盘条目文案。
///
/// 注意这里绝不能整体重建托盘菜单：检查通常由用户点击托盘条目发起，
/// 重建与紧接着的菜单交互并发会触发 AppKit panic（panic=abort 即闪退）。
fn mutate_status(app: &AppHandle, f: impl FnOnce(&mut UpdateStatus)) {
    if let Some(state) = app.try_state::<UpdateState>() {
        {
            let mut guard = lock_write(&state.status);
            f(&mut guard);
        }
    }
    emit_status(app);
    crate::tray::refresh_update_item(app);
}

fn fail(app: &AppHandle, message: String) -> UpdateStatus {
    log::warn!("更新: {}", message);
    mutate_status(app, |s| {
        s.phase = UpdatePhase::Failed;
        s.message = Some(message);
    });
    snapshot(app)
}

fn pending_version(app: &AppHandle) -> Option<String> {
    app.try_state::<UpdateState>().and_then(|state| {
        state
            .pending
            .lock()
            .ok()
            .and_then(|p| p.as_ref().map(|(v, _)| v.clone()))
    })
}

fn clear_pending(app: &AppHandle) {
    if let Some(state) = app.try_state::<UpdateState>() {
        let mut guard = state.pending.lock().unwrap_or_else(|e| e.into_inner());
        *guard = None;
    }
}

fn take_pending(app: &AppHandle) -> Option<(String, Vec<u8>)> {
    let state = app.try_state::<UpdateState>()?;
    let mut guard = state.pending.lock().unwrap_or_else(|e| e.into_inner());
    guard.take()
}

fn now_string() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Release 说明：全空白视为无说明。
fn clean_notes(body: Option<&str>) -> Option<String> {
    body.map(str::trim)
        .filter(|b| !b.is_empty())
        .map(str::to_string)
}

/// 把 updater 内部错误映射为中文可读文案（英文技术细节只写日志）。
fn user_error_msg(e: &tauri_plugin_updater::Error) -> String {
    use tauri_plugin_updater::Error as E;
    match e {
        // 签名不匹配是最严重的一类：要么密钥轮换，要么更新源被替换
        E::Minisign(_) | E::Base64(_) | E::SignatureUtf8(_) => {
            "更新包签名校验失败，已放弃本次更新".to_string()
        }
        E::ReleaseNotFound => "未找到更新源（latest.json），请确认已发布正式版本".to_string(),
        E::TargetNotFound(_) | E::TargetsNotFound(_) | E::UnsupportedArch | E::UnsupportedOs => {
            "当前平台暂无可用的更新包".to_string()
        }
        E::EmptyEndpoints | E::InsecureTransportProtocol | E::UrlParse(_) => {
            "更新源配置不正确".to_string()
        }
        E::Network(_) | E::Reqwest(_) | E::Http(_) | E::FormatDate => {
            "网络请求失败，请检查网络后重试".to_string()
        }
        E::Io(_)
        | E::TempDirNotFound
        | E::TempDirNotOnSameMountPoint
        | E::FailedToDetermineExtractPath
        | E::InvalidUpdaterFormat
        | E::BinaryNotFoundInArchive => "读写更新包失败，请检查磁盘空间与权限".to_string(),
        E::Serialization(_) => "更新源返回的数据无法解析".to_string(),
        _ => "更新过程中发生错误，请稍后重试".to_string(),
    }
}

/// 执行一次检查并更新状态；返回检查后的状态快照。
///
/// `manual` 为用户主动触发（托盘条目 / 设置页按钮）：此时无视「忽略此版本」
/// 记录，重新弹出可安装状态，让用户重新选择安装或再次忽略；自动定时检查
/// 仍尊重忽略记录，不重复打扰。
async fn run_check(app: &AppHandle, manual: bool) -> UpdateStatus {
    mutate_status(app, |s| {
        s.phase = UpdatePhase::Checking;
        s.message = Some("正在检查更新…".to_string());
    });
    let result = match app.updater() {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    let now = now_string();

    match result {
        Ok(Some(update)) => {
            let version = update.version.clone();
            let notes = clean_notes(update.body.as_deref());
            let date = update.date.and_then(|d| {
                chrono::DateTime::from_timestamp(d.unix_timestamp(), 0).map(|dt| {
                    dt.with_timezone(&chrono::Local)
                        .format("%Y-%m-%d")
                        .to_string()
                })
            });
            if pending_version(app).as_deref() == Some(version.as_str()) {
                let len = pending_len(app);
                mutate_status(app, |s| {
                    s.phase = UpdatePhase::Ready;
                    s.version = Some(version);
                    s.notes = notes;
                    s.date = date;
                    s.downloaded = len;
                    s.total = Some(len);
                    s.message = Some("更新包已下载完成，可立即安装".to_string());
                    s.last_checked = Some(now);
                });
            } else {
                clear_pending(app);
                mutate_status(app, |s| {
                    s.phase = UpdatePhase::Available;
                    s.version = Some(version);
                    s.notes = notes;
                    s.date = date;
                    s.downloaded = 0;
                    s.total = None;
                    s.message = None;
                    s.last_checked = Some(now);
                });
            }
        }
        Ok(None) => {
            clear_pending(app);
            mutate_status(app, |s| {
                s.phase = UpdatePhase::UpToDate;
                s.version = None;
                s.notes = None;
                s.date = None;
                s.downloaded = 0;
                s.total = None;
                s.message = Some("已是最新版本".to_string());
                s.last_checked = Some(now);
            });
        }
        Err(e) => {
            log::warn!("更新: 检查失败: {}", e);
            let message = user_error_msg(&e);
            mutate_status(app, |s| {
                s.phase = UpdatePhase::Failed;
                s.message = Some(message);
                s.last_checked = Some(now);
            });
        }
    }
    let final_status = snapshot(app);
    notify_result(app, manual, &final_status);
    final_status
}

/// 检查结果系统通知。
///
/// 托盘菜单点击即收起（macOS 系统行为），菜单里的状态文案一闪而过；
/// 通知驻留通知中心，用户可在方便时查看、手动关闭。手动检查始终反馈
/// 结果；自动检查仅在发现新版本时提醒，避免「已是最新」类噪音。
fn notify_result(app: &AppHandle, manual: bool, status: &UpdateStatus) {
    use tauri_plugin_notification::NotificationExt;
    let body = match (&status.phase, status.version.as_deref()) {
        (UpdatePhase::Available | UpdatePhase::Ready, Some(v)) => {
            format!("发现新版本 v{v}，可打开应用安装")
        }
        (UpdatePhase::Failed | UpdatePhase::UpToDate, _) if manual => {
            status.message.clone().unwrap_or_default()
        }
        _ => return,
    };
    if body.is_empty() {
        return;
    }
    if let Err(e) = app
        .notification()
        .builder()
        .title("应用更新")
        .body(body)
        .show()
    {
        log::warn!("更新: 发送通知失败: {}", e);
    }
}

/// 已下载更新包的字节数（用于把「已下载」状态复原到界面上）。
fn pending_len(app: &AppHandle) -> u64 {
    app.try_state::<UpdateState>()
        .and_then(|state| {
            state
                .pending
                .lock()
                .ok()
                .and_then(|p| p.as_ref().map(|(_, bytes)| bytes.len() as u64))
        })
        .unwrap_or(0)
}

/// 启动自动检查循环：启动 60 秒后首查，此后每小时一次；开关关闭时跳过。
pub fn spawn_auto_check(app: AppHandle, config: Arc<AsyncRwLock<AppConfig>>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_DELAY).await;
        loop {
            let enabled = config.read().await.auto_check_update;
            // 开发构建跳过自动检查：latest.json 面向正式构建，调试版本反复弹提示没有意义
            if enabled && !cfg!(debug_assertions) {
                let _ = run_check(&app, false).await;
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

/// 后台触发一次检查（托盘菜单入口，不阻塞调用方）。托盘点击即用户主动检查。
pub fn spawn_check(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        if snapshot(&app).phase.is_busy() {
            return;
        }
        let _ = run_check(&app, true).await;
    });
}

/// 托盘菜单中更新条目的文案：携带最近一次检查结果，再次打开菜单无需
/// 任何操作即可看到（结果同时经系统通知驻留通知中心）。
pub fn tray_item_label(app: &AppHandle) -> String {
    let status = snapshot(app);
    label_for(status.phase, status.version.as_deref())
}

/// `tray_item_label` 的纯函数部分，便于单元测试。
fn label_for(phase: UpdatePhase, version: Option<&str>) -> String {
    match (phase, version) {
        (UpdatePhase::Available, Some(v)) => format!("⬆ 新版本 v{v} 可用"),
        (UpdatePhase::Ready, Some(v)) => format!("⬆ v{v} 待安装"),
        (UpdatePhase::Downloading, Some(v)) => format!("⬇ 正在下载 v{v}"),
        (UpdatePhase::Installing, _) => "正在安装更新…".to_string(),
        (UpdatePhase::Checking, _) => "正在检查更新…".to_string(),
        // UpToDate 带版本号 = 该版本曾被「忽略」；手动检查可重新弹出
        (UpdatePhase::UpToDate, Some(v)) => format!("已忽略 v{v}（点此重新检查）"),
        (UpdatePhase::UpToDate, None) => "✓ 已是最新".to_string(),
        (UpdatePhase::Failed, _) => "⚠ 检查失败（点此重试）".to_string(),
        _ => "检查更新".to_string(),
    }
}

/// 托盘「更新」条目点击：有更新在手时打开主窗口（由横幅引导），否则触发一次检查。
pub fn on_tray_click(app: &AppHandle) {
    let status = snapshot(app);
    if matches!(
        status.phase,
        UpdatePhase::Available
            | UpdatePhase::Ready
            | UpdatePhase::Downloading
            | UpdatePhase::Installing
            | UpdatePhase::Checking
    ) {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
        }
        return;
    }
    spawn_check(app.clone());
}

#[tauri::command]
pub async fn get_update_status(app: AppHandle) -> Result<UpdateStatus, String> {
    Ok(snapshot(&app))
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<UpdateStatus, String> {
    let current = snapshot(&app);
    if current.phase.is_busy() {
        return Ok(current);
    }
    Ok(run_check(&app, true).await)
}

#[tauri::command]
pub async fn download_update(app: AppHandle) -> Result<UpdateStatus, String> {
    let current = snapshot(&app);
    let version = match (current.phase, current.version.clone()) {
        (UpdatePhase::Available, Some(v)) | (UpdatePhase::Failed, Some(v)) => v,
        // 已下载完成或正在进行中：原样返回，避免重复下载
        (UpdatePhase::Ready, _) => return Ok(current),
        (phase, _) if phase.is_busy() => return Ok(current),
        _ => return Err("当前没有可下载的更新，请先检查更新".to_string()),
    };

    mutate_status(&app, |s| {
        s.phase = UpdatePhase::Downloading;
        s.downloaded = 0;
        s.total = None;
        s.message = Some(format!("正在下载 v{}…", version));
    });

    // Update 实例由清单解析而来，无法长期保存，这里在下载前重新构建
    let checked = match app.updater() {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    let update = match checked {
        Ok(Some(update)) => update,
        Ok(None) => {
            return Ok(fail(
                &app,
                "更新已不可用（可能已被撤回），请稍后重新检查".to_string(),
            ))
        }
        Err(e) => return Ok(fail(&app, user_error_msg(&e))),
    };

    let progress_app = app.clone();
    let mut last_emit = Instant::now();
    let bytes = match update
        .download(
            move |chunk_len: usize, content_length: Option<u64>| {
                if let Some(state) = progress_app.try_state::<UpdateState>() {
                    let mut guard = lock_write(&state.status);
                    guard.downloaded += chunk_len as u64;
                    if content_length.is_some() {
                        guard.total = content_length;
                    }
                }
                if last_emit.elapsed() >= PROGRESS_EMIT_INTERVAL {
                    last_emit = Instant::now();
                    emit_status(&progress_app);
                }
            },
            || {},
        )
        .await
    {
        Ok(bytes) => bytes,
        Err(e) => return Ok(fail(&app, user_error_msg(&e))),
    };

    let len = bytes.len() as u64;
    let installed_version = update.version.clone();
    if let Some(state) = app.try_state::<UpdateState>() {
        let mut guard = state.pending.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some((installed_version.clone(), bytes));
    }
    log::info!("更新: v{} 已下载并通过签名校验", installed_version);
    mutate_status(&app, |s| {
        s.phase = UpdatePhase::Ready;
        s.version = Some(installed_version);
        s.downloaded = len;
        s.total = Some(len);
        s.message = Some("更新包已下载完成，可立即安装".to_string());
    });
    Ok(snapshot(&app))
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<UpdateStatus, String> {
    if cfg!(debug_assertions) {
        // 调试构建没有可替换的 .app/.exe 包，安装会把调试产物当应用目录替换，直接拒绝
        return Err("开发构建不支持应用内安装，请使用打包后的应用验证".to_string());
    }
    let current = snapshot(&app);
    if current.phase != UpdatePhase::Ready {
        return Err("请先下载更新包".to_string());
    }
    let Some(expected) = current.version.clone() else {
        return Err("更新包不存在，请重新检查更新".to_string());
    };

    // 安装需要 Update 实例提供的解包上下文，因此安装前再取一次清单
    let checked = match app.updater() {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    let update = match checked {
        Ok(Some(update)) if update.version == expected => update,
        Ok(_) => {
            return Err("更新已不可用（可能已被撤回），请重新检查更新".to_string());
        }
        Err(e) => return Err(user_error_msg(&e)),
    };

    let Some((pending_version, bytes)) = take_pending(&app) else {
        return Err("更新包不存在，请重新下载".to_string());
    };
    if pending_version != expected {
        return Err("更新包与当前版本不匹配，请重新下载".to_string());
    }

    mutate_status(&app, |s| {
        s.phase = UpdatePhase::Installing;
        s.message = Some("正在安装更新…".to_string());
    });
    log::info!("更新: 开始安装 v{}", expected);

    // 安装会解包并替换应用目录，macOS 下还可能弹出管理员授权，放在阻塞线程执行
    let result = tauri::async_runtime::spawn_blocking(move || update.install(&bytes)).await;
    match result {
        Ok(Ok(())) => {
            log::info!("更新: v{} 安装完成，正在重启应用", expected);
            // Windows 上 install() 成功后插件已直接退出进程（不会执行到这里）；
            // macOS / Linux 需要主动重启才能运行新版本
            #[cfg(not(target_os = "windows"))]
            app.request_restart();
            Ok(snapshot(&app))
        }
        Ok(Err(e)) => Ok(fail(&app, user_error_msg(&e))),
        Err(e) => Ok(fail(&app, format!("安装任务异常终止：{}", e))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(phase: UpdatePhase, version: Option<&str>) -> UpdateStatus {
        let mut s = UpdateStatus::initial("0.2.0".to_string());
        s.phase = phase;
        s.version = version.map(str::to_string);
        s
    }

    #[test]
    fn busy_phases_block_new_checks() {
        assert!(UpdatePhase::Checking.is_busy());
        assert!(UpdatePhase::Downloading.is_busy());
        assert!(UpdatePhase::Installing.is_busy());
        assert!(!UpdatePhase::Idle.is_busy());
        assert!(!UpdatePhase::Available.is_busy());
        assert!(!UpdatePhase::Ready.is_busy());
        assert!(!UpdatePhase::UpToDate.is_busy());
        assert!(!UpdatePhase::Failed.is_busy());
    }

    #[test]
    fn phase_serializes_as_snake_case() {
        let json = serde_json::to_string(&UpdatePhase::UpToDate).unwrap();
        assert_eq!(json, "\"up_to_date\"");
        let json = serde_json::to_string(&UpdatePhase::Available).unwrap();
        assert_eq!(json, "\"available\"");
    }

    #[test]
    fn status_snapshot_matches_frontend_field_names() {
        let value = serde_json::to_value(status(UpdatePhase::Available, Some("0.3.0"))).unwrap();
        for key in [
            "phase",
            "current_version",
            "version",
            "notes",
            "date",
            "downloaded",
            "total",
            "message",
            "last_checked",
        ] {
            assert!(value.get(key).is_some(), "缺少字段 {key}");
        }
    }

    #[test]
    fn clean_notes_treats_blank_as_none() {
        assert_eq!(clean_notes(None), None);
        assert_eq!(clean_notes(Some("   \n ")), None);
        assert_eq!(
            clean_notes(Some(" 修复若干问题 ")),
            Some("修复若干问题".to_string())
        );
    }

    #[test]
    fn tray_label_carries_last_check_result() {
        // 检查结果在托盘文案中可见：无需用户再次点开任何界面
        assert_eq!(label_for(UpdatePhase::UpToDate, None), "✓ 已是最新");
        assert_eq!(
            label_for(UpdatePhase::UpToDate, Some("0.2.18")),
            "已忽略 v0.2.18（点此重新检查）"
        );
        assert_eq!(
            label_for(UpdatePhase::Failed, None),
            "⚠ 检查失败（点此重试）"
        );
        // 有更新在手 / 空闲态的既有文案保持不变
        assert_eq!(
            label_for(UpdatePhase::Available, Some("0.2.18")),
            "⬆ 新版本 v0.2.18 可用"
        );
        assert_eq!(label_for(UpdatePhase::Idle, None), "检查更新");
    }
}
