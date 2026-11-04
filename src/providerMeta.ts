/**
 * 供应商元数据共享模块 —— 供应商配置页与设置页共用。
 * 此文件保持零依赖，避免循环导入（同 types.ts 风格）。
 */

// 新增类型时同步 providers/mod.rs::ALL_PROVIDER_TYPES 与 tray.rs::tray_short_code
/** 服务商类型下拉与显示名（与 providers/mod.rs 的 display_name 保持同口径）。 */
export const PROVIDER_OPTIONS = [
  { value: 'OpenAI', label: 'OpenAI(API)' },
  { value: 'Codex', label: 'Codex' },
  { value: 'Anthropic', label: 'Anthropic(API)' },
  { value: 'Claude Code', label: 'Claude Code' },
  { value: 'DeepSeek', label: 'DeepSeek' },
  { value: 'Kimi API', label: 'Kimi API' },
  { value: 'Kimi Code', label: 'Kimi Code' },
  { value: 'Zhipu', label: '智谱' },
  { value: 'Qoder', label: 'Qoder' },
];

/** 无需用户配置 API Key，凭证从本机 CLI / 桌面客户端自动读取的供应商类型。
 *  与 Rust needs_api_key 名单同口径；Qoder 虽属免密默认，但其 PAT 兜底入口
 *  在供应商页渲染分支特例放行（所有平台可填、可清空）。 */
export const KEYLESS_PROVIDERS = new Set(['Codex', 'Claude Code', 'Kimi Code', 'Qoder']);

/**
 * 是否 macOS。Qoder 的本地凭据读取（客户端 auth.v1.dat + 钥匙串解密）仅 macOS 支持，
 * 其他平台该路径必然失败，需回退为 PAT 输入（#88）。用 navigator.platform / userAgent
 * 做最小侵入探测，不引入 Tauri 插件。
 */
export const IS_MAC =
  /Mac/i.test(navigator.platform || '') || /Mac OS X/i.test(navigator.userAgent);

/** 根据 provider 标识取默认显示名（下拉框 label）；未知类型兜底返回标识本身。 */
export function providerLabel(name: string): string {
  return PROVIDER_OPTIONS.find((o) => o.value === name)?.label || name;
}
