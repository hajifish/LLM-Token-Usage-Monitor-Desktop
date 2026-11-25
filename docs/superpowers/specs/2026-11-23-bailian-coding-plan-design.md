# 百炼 Coding Plan 供应商接入设计

- 日期：2026-11-23（v2 转向：2026-11-25）
- 分支：`feature/bailian-coding-plan`
- 状态：v1（纯 sk-sp- Key）已实施但**被现网实测否定**；v2 改走 bl CLI 免密（用户已确认）。见文末「v2 转向」。

## 背景与目标

阿里云百炼（Model Studio）有三套互不相通的系统，Key 与 Base URL 都不通用：

| 系统 | Key 形态 | 计费方式 | 用量查询通道 |
| --- | --- | --- | --- |
| 按量计费（通用） | `sk-xxx` | 按量扣账户余额 | 无公开余额 API |
| **Coding Plan** | `sk-sp-xxx` | 按**模型调用次数**，5h 滚动 / 每周 / 账单月三窗口 | 有第三方验证过的查询端点 |
| Token Plan（团队/个人） | 专属 Key | 按 Credits（token 折算）抵扣，订阅月重置 | 仅控制台 Cookie（含 CSRF + sec_token） |

**本期范围：只做 Coding Plan。** 它以 API-key 模式直接落入现有架构，端点已被多个第三方实现（dsh-musage、CodexBar）验证；Token Plan 需新增「粘贴控制台 Cookie」机制（工作量约 3-4 倍且 Cookie 会过期），留待下期。

目标：新增供应商类型「Bailian Coding」，在托盘菜单与主窗口展示三窗口（5小时 / 每周 / 账单月）的剩余比例，主值为最紧张窗口的剩余百分比，与 Kimi Code / Qoder 等 percent 型供应商同口径。

## 决策

| 项 | 决策 |
| --- | --- |
| 范围 | 仅 Coding Plan；Token Plan 下期 |
| 内部类型名 | `Bailian Coding`（配置键 / 工厂键） |
| 显示名 | `百炼 Coding Plan` |
| 托盘短码 | `BC`（与 KC/QD 同风格；将来 Token Plan 若做叫 BT，成对） |
| 凭据来源 | 用户粘贴 Coding Plan 专用 Key（`sk-sp-` 前缀，存 api_key）；**需 API Key**，不进免密名单 |
| 配额展示 | 三窗口平铺（5小时 / 每周 / 账单月），缺哪个窗口跳过哪个 |
| 主值语义 | 5h 剩余 % 优先，回退周 → 月（与 Kimi Code 主窗口选择一致）；托盘主聚合走 `primary_quota`（最紧张窗口） |
| currency | `"%"`（percent 型） |
| fetch_usage | `Ok(None)`（端点无 token / 成本数据） |
| 区域 | 国内宿主优先，`ConsoleNeedLogin` 时自动重试国际宿主 |
| 图标 | `public/providers/bailiancoding.png` 占位（圆底「百」字），后续可换官方 logo |

## 数据通道（第三方验证的非官方端点）

```
POST https://bailian.console.aliyun.com/data/api.json
     ?action=zeldaEasy.broadscope-bailian.codingPlan.queryCodingPlanInstanceInfoV2
     &product=broadscope-bailian&api=queryCodingPlanInstanceInfoV2
Content-Type: application/json        body: {}
Authorization: Bearer <sk-sp-key>
x-api-key: <sk-sp-key>
X-DashScope-API-Key: <sk-sp-key>      ← 三个 header 同发（各宿主接受方式不同）
```

国际宿主：`https://modelstudio.console.alibabacloud.com` + 同 path。

### 响应 schema

```jsonc
{
  "code": "Success" | "200" | "ConsoleNeedLogin" | "InvalidApiKey" | "Unauthorized" | ...,
  "message": "...",            // 或 "msg"
  "data": {
    "codingPlanInstanceInfos": [{
      "codingPlanQuotaInfo": {
        "per5HourUsedQuota": 123, "per5HourTotalQuota": 6000,
        "per5HourQuotaNextRefreshTime": 1780000000,   // unix 秒
        "perWeekUsedQuota": ..., "perWeekTotalQuota": ..., "perWeekQuotaNextRefreshTime": ...,
        "perBillMonthUsedQuota": ..., "perBillMonthTotalQuota": ..., "perBillMonthQuotaNextRefreshTime": ...
      }
    }]
  }
}
```

额度按「模型调用次数」计（Pro 档：5h 6000 / 周 45000 / 月 90000）。

### 非官方风险

端点为逆向所得，存在变更风险。错误信息带上原始 `code` 方便排查；额度源全部缺席视为 schema 漂移直接报错，不静默给错误数值（与 Qoder 同纪律）。

## Rust 端

新模块 `src-tauri/src/providers/bailian_coding.rs`：

- `BailianCodingProvider::new(api_key)`：api_key 直存（参照 zhipu.rs），`tokio::sync::OnceCell` 缓存单次解析结果，`fetch_balance` + `fetch_quota_infos` 共用。
- 请求层：先国内宿主；响应 `code == "ConsoleNeedLogin"` 时用国际宿主重试一次（同 dsh-musage 策略）。
- 解析层（纯函数，单测优先）：
  - `code` 非 `Success`/`200`：`InvalidApiKey`/`Unauthorized` → 认证失败文案（提醒检查是否误用百炼通用 `sk-` Key）；其余 → 服务端错误（附原始 code + message）。
  - `ConsoleNeedLogin` → 专用信号，触发国际重试。
  - `codingPlanInstanceInfos` 为空 → 「未订阅 Coding Plan」。
  - `codingPlanQuotaInfo` 缺失 → 解析错误。
  - 单窗口：`used`/`total` 有限且 `total > 0` 才有效，否则跳过；`resetRaw > 1e9` 视为 unix 秒转 RFC3339 填 `reset_at`。
  - 三窗口全缺 → schema 漂移错误。
- `fetch_balance`：5h 剩余 % 优先，回退周 → 月，`currency: "%"`。
- `fetch_quota_infos`：最多 3 张配额卡（「5小时」「每周」「账单月」），`plan_label: "coding plan"`，`reset_at` 为 RFC3339、`reset_time` 走 `format_iso_reset` 文本回退。
- `fetch_usage`：`Ok(None)`。

### 工厂与同步点（守卫测试强制）

- `providers/mod.rs`：`ALL_PROVIDER_TYPES` 加 `"Bailian Coding"`；`display_name` → `"百炼 Coding Plan"`；`create_provider` 加分支；`display_name_maps_all_types` 测试期望表加行。
- `tray.rs`：`tray_short_code` 加 `"Bailian Coding" => Some("BC")`（`every_builtin_provider_type_has_short_code` 守卫）。
- `scheduler.rs`：`needs_api_key` 保持需 Key（不在免密 matches! 名单，无需改函数体）；`needs_api_key_list_locked` 测试的「应要求 API Key」列表加 `"Bailian Coding"`。

## 前端集成

- `src/providerMeta.ts`：`PROVIDER_OPTIONS` 加 `{ value: 'Bailian Coding', label: '百炼 Coding Plan' }`；**不进** `KEYLESS_PROVIDERS`（标准 API Key 输入框）。
- `src/components/Providers.tsx`：Key 输入下加提示「sk-sp- 开头，从百炼 Coding Plan 页面获取；勿与按量计费 sk- Key 混用」。
- `src/components/UsagePanel.tsx`：`BRAND_LOGOS` 加 `'Bailian Coding': '/providers/bailiancoding.png'`。
- `public/providers/bailiancoding.png`：占位图（圆底「百」字）。
- 设置页托盘开关、通知等走 percent 型供应商通用路径，零改动。

## 测试（TDD）

parser 单测优先：
- 完整三窗口 fixture（used/total/reset 齐全）→ 3 张卡，剩余 % 与 reset_at 正确。
- 部分窗口（缺周/月）→ 有几张渲染几张。
- 单窗口 total ≤ 0 / 字段缺失 → 跳过该窗口。
- 三窗口全缺 → 报错。
- `ConsoleNeedLogin` → 触发国际重试信号。
- `InvalidApiKey`/`Unauthorized` → 认证失败；其它 code → 服务端错误附 code。
- 空 `codingPlanInstanceInfos` → 未订阅。
- unix 秒时间戳 → RFC3339 转换。
- 既有同步守卫测试（display_name / tray_short_code / needs_api_key）自动逼出所有映射点。

## 待定项

- 托盘/卡片图标 `bailiancoding.png`：先用生成的占位图（圆底「百」字），之后用户换官方 logo。
- Token Plan（Cookie 方案）：下期独立设计。

## v2 转向（2026-11-25）：纯 Key 被实测否定，改走 bl CLI 免密

### 实测否定 v1

v1 实施后真机实测：真实 `sk-sp-` Key、假 `sk-sp-` Key、完全不带鉴权 header，三种情况控制台网关
`bailian.console.aliyun.com/data/api.json?...queryCodingPlanInstanceInfoV2` 一律返回
`{"code":"ConsoleNeedLogin","message":"请登录"}`。即三 header 被网关忽略，纯 Key 不被接受。

交叉核对社区实现：
- dsh-musage 的 dashscope provider 确为三 header、无 Cookie 层，但其"已验证"注释无证据，与实测一致属一厢情愿。
- CodexBar `AlibabaCodingPlanUsageFetcher` 的 apiKey 模式同打该网关，收 ConsoleNeedLogin 抛
  `apiKeyUnavailableInRegion`；其文档明说 CN 账号配 API key 仍 ConsoleNeedLogin、**必须 web session（Cookie）**。
- 结论：**不存在纯 sk-sp- Key 查配额的通道**；sk-sp- 仅用于推理。所有可用通道都需先做一次控制台登录。

### 决策（用户确认）：bl CLI 免密

本机已装百炼官方 `bl` CLI（2.11.2），其自带：
- `bl auth login --console`：浏览器登录一次，控制台凭据存本机 `~/.bailian`。
- `bl usage coding-plan --output json`：[Console] 鉴权，官方命令直接返回 Coding Plan 配额，自动处理网关/续期/区域。

故 v2 把「Bailian Coding」改为**免密 / 读本机凭据型**供应商（同 Kimi Code / Qoder 架构）：
provider 不再收 api_key，改为 shell 调 `bl usage coding-plan --output json --quiet` 并解析 stdout。

### v2 集成点变更（相对 v1）

- `providers/bailian_coding.rs`：删除 HTTP 三 header / CN-INTL 重试 / ConsoleNeedLogin 逻辑；
  `new()` 无参；`fetch_windows` 改 tokio 子进程调 bl；解析 bl 的 JSON 输出（形状以登录后实测 fixture 为准）。
  bl 未安装 / console 未登录 → 明确报错引导 `bl auth login --console`。
- `scheduler.rs` `needs_api_key`：把 `"Bailian Coding"` 加入免密 matches! 名单；测试免密列表加名、需 Key 列表移除。
- `providerMeta.ts` `KEYLESS_PROVIDERS`：加 `'Bailian Coding'`。
- `Providers.tsx`：免密分支提示改「自动读取本机 bl CLI 控制台登录态（先运行 bl auth login --console）」；删除 sk-sp- 输入提示。
- `mod.rs` `create_provider`：`BailianCodingProvider::new()` 无参。
- 显示名 / 托盘短码 BC / 图标 / 三窗口 QuotaInfo 语义不变。

### v2 测试

- 以登录后 `bl usage coding-plan --output json` 真实输出为 fixture 写解析单测。
- bl 缺失 / 未登录 / 非零退出 / 空输出 的错误路径单测（mock 子进程输出）。
- 同步守卫测试（needs_api_key 免密名单 / display_name / tray_short_code）随名单更新。
## v3 增量（2026-11-25）：百炼 Token Plan（用户确认「加 Token Plan，保留 Coding」）

### 现网事实（bl 实测）

- 用户账号下 `bl usage coding-plan` 返回 `{}`（无 VALID Coding Plan 订阅）；
  `bl usage token-plan` 有数据 → 用户实际订阅的是 **Token Plan（Essential 档）**，非 Coding Plan。
  故 v2 的「未找到有效的 Coding Plan 订阅」提示是如实汇报，不是缺陷。
- Token Plan 归一化输出（bailian-cli-commands 的 FT/IT，正则 `^per(\d+)(Hour|Day|Week|Month)(Percentage|ResetTime)$`）：
  `{ "per1MonthPercentage": 0.00079…, "per1MonthResetTime": 1798214400000, "hasQuotaData": true }`。
  **percentage 为已用占比（0-1）**；**ResetTime 为 unix 毫秒**（coding-plan 为秒，两者不同）；
  无订阅时 `{ "hasQuotaData": false }`；窗口按跨度升序（Hour=1/Day=24/Week=168/Month=720 小时权重）。
- bl 不暴露 Token Plan 绝对额度，只给占比 → 配额走**百分比型**口径（同 Codex）：used=已用%、total=100、current_value=None。

### 设计

- 新增免密供应商 `Bailian Token`（显示名「百炼 Token Plan」，托盘短码 **BT**），
  shell 调 `bl usage token-plan --output json --quiet`；窗口名：per1Month→「订阅月」，
  其余 per1X→「每X」、perNX→「N X」（小时/天/周/月）；主窗口取跨度最短者。
- 抽共享通道 `providers/bailian_cli.rs::usage_json(subcommand)`（spawn_blocking + std::process::Command，
  bl 缺失/未登录/非零退出/JSON 解析错误映射），Coding 与 Token 共用；coding 的 fetch_windows 改调它。
- 同步点：mod.rs（ALL_PROVIDER_TYPES/display_name/create_provider/测试行）、tray.rs BT、
  scheduler.rs needs_api_key 免密名单+锁定测试、providerMeta.ts（PROVIDER_OPTIONS/KEYLESS_PROVIDERS）、
  Providers.tsx 免密提示复用 bl 文案、UsagePanel BRAND_LOGOS + public/providers/bailiantoken.png（橙圆白 T）。
- 无订阅/全窗口缺席 → 「未找到有效的 Token Plan 订阅（bl 返回空配额）」。

### v3 测试与验收

- 解析单测 10 个：现网月度形状（含毫秒重置换算）、hasQuotaData=false、{}、多窗口跨度排序、
  缺重置、仅 ResetTime 键跳过、非法键忽略、小重置时间戳忽略、超用 clamp、负占比跳过。
- 现网探针 `#[ignore] live_bl_token_plan`（手动 `cargo test --lib live_bl_token_plan -- --ignored --nocapture`）：
  2026-11-25 实测「订阅月: 剩余 99.75% reset=2026-12-25T16:00:00+00:00」（= 北京 12-26 00:00），与 bl 表格输出一致。
- 门禁：cargo fmt/clippy -D warnings/test 241 过；prettier/eslint/tsc/vite build 全绿。
- 界面验收：供应商页添加「百炼 Token Plan」（免密、无需 Key）后，面板应显示订阅月剩余%与重置倒计时。
