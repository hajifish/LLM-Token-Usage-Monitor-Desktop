# 状态栏方案 B 重设计：原生菜单增强 + V3 聚合标题 · 设计 spec

- 日期：2026-10-31
- 状态：待用户评审
- 分支：`feat/tray-menu-redesign`（基于 `origin/develop` @ 6c3fd84）
- 设计草图：`docs/design/status-bar-redesign-mockup.html`（三方案对照与假数据渲染）

## 1. 背景与目标

用户在状态栏（macOS 菜单栏托盘）就要看清楚全部供应商的用量，不想打开主页。经三方案草图评审（A 情报面板 popover / B 原生菜单增强 / C 菜单栏文字变体），确认：

- **展开层选方案 B**：原生 NSMenu 增强，零新增窗体；
- **菜单栏常显文字选 V3 聚合态**：只显示最紧张的一项，最紧凑；
- **多窗口供应商默认只显示主窗口一行**，其余折叠进子菜单；
- **历史维度不做**（详见 §2）；
- 供应商尽可能全量展示，放不下靠 macOS 原生整页菜单滚动，不做人为截断（现有 50 条上限机制保留）。

## 2. 历史维度的决策依据（查证结论）

全部 8 类供应商数据源均为远程 API、仅返回当前值，无请求级历史、无历史端点、不读本地会话文件：

| 供应商 | 端点 | 返回 |
|---|---|---|
| Claude Code | `api.anthropic.com/api/oauth/usage` | five_hour / seven_day(_opus/_sonnet) 各窗口 utilization + resets_at |
| Codex | `chatgpt.com/backend-api/wham/usage` | 当前窗口配额 |
| Zhipu / Kimi Code | 开放平台配额接口 | 当前配额 |
| DeepSeek / Kimi API / OpenAI / Anthropic | 余额接口 | 当前余额 |

做历史只能本地快照累积（轮询追加快照 + 窗口重置感知差值 + 分桶），成本中等且数据自部署起才有。**本设计不含历史**；将来主页历史可独立立项，若日后升级方案 A（popover 面板），主页积累的快照可直接复用。

## 3. 现状（origin/develop @ 6c3fd84）

- 调度器已在轮询中拉取 `quota_infos`（`scheduler.rs:212`），四家订阅计划供应商均已实现 `fetch_quota_infos`，**但托盘菜单从未消费该数据**（`format_provider_line` 只读 balance）。
- 菜单机制：`TrayMenuState` 一次性构建菜单 + 按签名（`name|alias` 序列）原地 `set_text`，避免 `set_menu` 与菜单交互并发的 AppKit 闪退。本设计沿用该机制。
- 菜单栏文字 `build_tray_title`：别名截断版（`智谱 70% · DeepSeek ¥88.88`）。
- **注意**：未合并的 `feat/tray-interval-update-simplify` 分支（远端 50a0947 领先本地）已含类型短码映射（CC/DS/OA…）、logo/文字互斥、刷新间隔子菜单等改动，与本设计改动面重叠。见 §8 实施顺序。

## 4. 数据模型（已有，不改）

`QuotaInfo { name, used, total, current_value, remaining, remaining_percent, reset_time: Option<String>(ISO), plan_label }`
`BalanceData { available_balance, currency: "%" | "USD" | "CNY", voucher_balance, cash_balance, … }`
`UsageData { total_tokens, prompt_tokens, completion_tokens, today_cost: Option, today_requests: Option }`
`ProviderStatus { name, alias, enabled, balance, usage, error, quota_infos: Option<Vec<QuotaInfo>> }`

## 5. 详细设计

### 5.1 托盘菜单块结构（`format_provider_line` 重构为按块生成多行）

每家供应商一个「块」，1~3 个菜单项（均 disabled 展示项，沿用现状）：

```
● Claude Code · Max工作 — 剩 62%              ← 标题行（每家必有）
    5h ▓▓▓░░░░░░░ 剩62% · 1小时24分后重置       ← 主窗口行（订阅计划型且有 quota_infos）
    其余 3 个窗口 ▸                             ← 折叠子菜单（窗口数 >1 时）
        7d   ▓░░░░░░░░░ 剩81% · 3天4小时后重置
        7天Opus ▓░░░░░░░░░ 剩95%
● Codex · Pro — 剩 9%
    5h ▓▓▓▓▓▓▓▓▓░ 剩9% · 12分后重置
● DeepSeek — ¥88.88
    今日 ¥1.24 · 42次 · 1.2M tok
────────────────
更新于 HH:MM · 刷新间隔 ▸ · 打开主页 · 检查更新 · ⏻ 退出（尾部不变）
```

**标题行**：`{状态点} {alias} — {主指标}`
- 状态点：沿用现有 `status_dot(warning_level)` 字符（原生菜单文本无法着色，方案 B 不引入供应商识别色）。
- 主指标：percent 型 = `剩 {n}%`（数值来源规则见 5.2：有 quota_infos 时以主窗口 remaining_percent 为准，否则回退 balance.available_balance）；USD = `${x}`；CNY = `¥{x}`；error = `获取失败`；无数据 = `无数据`。

**主窗口行**（percent 型且 `quota_infos` 非空）：`{窗口名} {进度条} 剩{x}% · {倒计时}`
- 窗口名：`QuotaInfo.name` 原值（5小时 / 7天 / 7天Opus…，菜单内用原值，不缩写）。
- 进度条：10 格 `▓`/`░`，`▓` 数 = round(used / total × 10)；已用越多 `▓` 越多；沿用现有 `progress_bar` 函数。
- 倒计时：见 5.4；缺 reset_time 则省略 ` · {倒计时}` 段。

**折叠子菜单**（quotas.len() > 1）：一个 Submenu 项 `其余 {n-1} 个窗口`，内含其余窗口各一行（格式同主窗口行）。子菜单标题行本身为 disabled 展示项。

**API 型明细行**（currency 为 USD/CNY 且 usage 有值）：`今日 {today_cost} · {today_requests}次 · {tokens 格式化}`，Option 字段缺省即省略该段，全缺省则不加此行。tokens 格式化：`1.2M` / `980k` 风格。

**降级**：percent 型但 `quota_infos` 为 None（拉取失败）→ 仅标题行（主指标仍来自 balance）；error/无数据 → 仅标题行。

### 5.2 主窗口选择规则

`quota_infos` 按 `remaining_percent` 升序取第一；并列（差 < 0.5）取 `reset_time` 距当前更近者；`reset_time` 缺失排最后。标题行主指标（balance.available_balance）与主窗口行可能因四舍五入出现 61% vs 62% 的不一致——统一以**选中主窗口的 remaining_percent** 作为标题行主指标来源；balance 仅在无 quota_infos 时使用。

### 5.3 菜单栏常显文字 V3 聚合（`build_tray_title` 重写）

- 候选：`enabled && provider_show_in_tray`（现有过滤逻辑不变）。
- 排序取第一：`warning_level` 降序 →（同 level）percent 型按剩余% 升序在前，非 percent 型随后按余额升序 →（仍并列）主窗口（§5.2 选中者）reset_time 更近在前。
- 格式（短码沿用 `tray_short_code` 映射，未收录类型回退别名截断）：
  - percent 型，平静态：`CC 62%`
  - percent 型，告警（level ≥ 1）：`⚡ CC 9% · 12分`（倒计时紧凑格式）
  - API 型：`DS ¥88.88`，告警加 `⚡` 前缀
- 无候选 → `None`（纯图标，现状不变）。
- 菜单栏文本无法着色，告警语义仅由 `⚡` 字符承载（原生限制，已与用户确认）。

### 5.4 倒计时格式化（新增纯函数，菜单与菜单栏共用）

输入 `Option<String>` ISO 时间：解析失败/缺失 → None（省略倒计时段）；已过期（≤0）→ 省略（等待下轮刷新）。
- 菜单格式：<1h → `X分后重置`；<24h → `X小时Y分后重置`；≥24h → `X天Y小时后重置`。
- 菜单栏紧凑格式：<1h → `X分`；<24h → `X:YY`；≥24h → `Xd`。

### 5.5 菜单句柄与签名扩展（`TrayMenuHandles` / `provider_signature`）

- 块内条目数与子菜单存在性随窗口数变化 → 签名扩展为 `name|alias|q1,q2,…`（quota 名有序序列）。签名变化才整体重建，否则原地 `set_text`（含子菜单内条目）。
- `TrayMenuHandles` 结构调整为块状：`Vec<ProviderBlock { title_item, detail_item, extra_submenu: Option<(Submenu, Vec<MenuItem>)> }>` + `time_item`。
- 原地刷新并发安全性要求不变（绝不 `set_menu` 替换活动菜单）；50 条上限与溢出提示机制按「菜单项数」沿用。

### 5.6 阈值

告警阈值沿用现有 `provider_warning_level`（percent：<5 黄 / ≤0 红；CNY 余额 <10 黄；USD 不参与），本次不改（草图中的 35%/10% 仅为示意）。

## 6. 错误处理

- `quota_infos` 拉取失败/None：降级仅标题行（§5.1），不阻塞其他供应商渲染。
- reset_time 解析失败：省略倒计时，不影响该行其余部分。
- 菜单重建失败/部分 append 失败：沿用现有「关键项必须全部成功才装配」策略；供应商展示项失败可忽略。
- tooltip 与菜单栏标题更新失败：现有 `log::warn` 行为不变。

## 7. 测试计划

纯函数单测（沿用 `tray_title_tests` / `format_provider_line` 测试风格）：
1. 主窗口选择：最低剩余、并列取近重置、缺失排尾、单窗口。
2. 块行生成：percent 型（多窗口/单窗口/无 quota 降级）、USD/CNY 型（usage 全有/部分缺省/全缺省）、error/无数据。
3. 倒计时格式化：分钟/小时/天边界、缺失、解析失败、已过期。
4. V3 聚合：排序规则（level → percent 升序 → API 靠后）、告警/平静格式、空候选 → None、show_in_tray 过滤。
5. 签名扩展：窗口数变化 → 签名变化；仅数值变化 → 签名不变。
6. 进度条格数与 tokens 格式化的边界值。

## 8. 实施顺序与依赖（需用户确认）

`feat/tray-interval-update-simplify`（远端 50a0947）与本设计改动面重叠（build_tray_title、tray 菜单）。两个选项：

- **推荐**：先收尾并合入该分支（其短码映射、间隔子菜单是本设计的组成部分），本设计在其之上实施，冲突最小；
- 或本设计独立实施，短码映射等在本分支重新引入，合并时解决冲突。

另：本地 `develop` 与 `origin/develop` 已分叉（本地多 2 个更名提交未推送，远端多 4 个提交），需用户自行处理后再走 PR。

## 9. 验收清单（人工）

- [ ] 多窗口供应商（Claude Code）：标题行 + 主窗口行 + 折叠子菜单，主窗口 = 最紧窗口；
- [ ] 单窗口供应商（Zhipu）：无子菜单；API 供应商：今日消耗行正确、字段缺省时省略；
- [ ] 菜单栏 V3：平静/告警/纯 API/无候选 四态正确；告警 `⚡` 前缀 + 紧凑倒计时；
- [ ] 轮询原地刷新不闪退（数值变化不重建；增删供应商/窗口数变化才重建）；
- [ ] 供应商很多时菜单整体滚动，无截断异常；50 上限提示正常；
- [ ] 亮暗两外观下状态点/进度条字符可读。

## 10. 不做的事

- 历史/sparkline（独立立项，见 §2）；
- popover 窗体（方案 A 备选）；
- 告警阈值调整；
- 主页 UI、设置页改动。
