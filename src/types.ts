/**
 * Shared type definitions — single source of truth.
 * 字段名与类型须与 Rust models.rs serde 输出严格一致（snake_case）。
 * 此文件保持零依赖，避免循环导入。
 */

export interface BalanceData {
  provider: string;
  available_balance: number;
  voucher_balance: number;
  cash_balance: number;
  total_balance: number;
  currency: string;
}

export interface QuotaInfo {
  name: string;
  used: number;
  total: number;
  current_value: number | null;
  remaining: number | null;
  remaining_percent: number;
  reset_time: string | null;
  plan_label: string | null;
}

export interface UsageData {
  provider: string;
  total_tokens: number;
  prompt_tokens: number;
  completion_tokens: number;
  today_cost: number | null;
  today_requests: number | null;
}

export interface ProviderStatus {
  name: string;
  alias: string;
  enabled: boolean;
  balance: BalanceData | null;
  usage: UsageData | null;
  error: string | null;
  quota_infos: QuotaInfo[] | null;
}

export interface UsageSummary {
  providers: ProviderStatus[];
  total_balance: number;
  last_updated: string;
}

export interface ProviderConfig {
  name: string;
  api_key: string;
  enabled: boolean;
  platform_token?: string | null;
  alias?: string | null;
}

export interface AppConfig {
  providers: ProviderConfig[];
  refresh_interval: number;
  /** 是否自动检查更新（启动后 60 秒首查，之后每 24 小时一次）。 */
  auto_check_update: boolean;
  /** 用户「忽略此版本」记录的新版本号；出现更新的版本时自动失效。 */
  skipped_update_version: string | null;
}

/** 应用内更新的阶段 —— 与 Rust `updater::UpdatePhase` 严格一致（snake_case）。 */
export type UpdatePhase =
  | 'idle'
  | 'checking'
  | 'up_to_date'
  | 'available'
  | 'downloading'
  | 'ready'
  | 'installing'
  | 'failed';

/** 更新状态快照 —— 与 Rust `updater::UpdateStatus` 严格一致（snake_case）。 */
export interface UpdateStatus {
  phase: UpdatePhase;
  /** 当前应用版本（构建时写入的 tauri.conf.json version）。 */
  current_version: string;
  /** 可用新版本号；无更新时为 null。 */
  version: string | null;
  /** 更新说明（来自 Release notes）。 */
  notes: string | null;
  /** 新版本发布时间。 */
  date: string | null;
  /** 已下载字节数。 */
  downloaded: number;
  /** 更新包总字节数（服务端未提供时为 null）。 */
  total: number | null;
  /** 状态说明文案（如「已是最新版本」「已忽略该版本」）。 */
  message: string | null;
  /** 最近一次检查完成时间。 */
  last_checked: string | null;
}

/**
 * 配置健康状态 —— 与 Rust `config::ConfigHealth` 严格一致（snake_case）。
 * 本次启动加载配置文件时发生的关键事件，用于在设置页明确提示用户。
 */
export interface ConfigHealth {
  /** 配置文件存在，但本机无法解密/读取（换机、系统重装、篡改或损坏）。 */
  unreadable: boolean;
  /** 覆盖前备份守卫生成的 .bak 文件路径；仅在 unreadable 状态下执行保存后才有值。 */
  backup_path: string | null;
  /** 明文→密文迁移被暂缓（当前仅能取得兜底机器标识），下次启动会重试。 */
  migration_deferred: boolean;
  /** 本次启动已完成明文→密文迁移（原明文已被加密信封安全替换）。 */
  migrated_from_plaintext: boolean;
}
