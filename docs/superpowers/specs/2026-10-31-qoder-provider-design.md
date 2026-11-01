# Qoder 供应商接入设计

- 日期：2026-10-31（v2 方案切换：2026-11-01）
- 分支：`feature/main-window-compact`（用户指定在本分支上补充，不另拉分支）
- 状态：已确认设计（v2：本地凭据）

## 背景与目标

Qoder 是阿里基于 Qwen 系模型的 agentic IDE，额度单位为 Credits。本项目需要一个新供应商类型「Qoder」，在托盘菜单与主窗口中展示：

1. 计划内额度（`userQuota`）、加量包（`addOnQuota`）、专属积分包（`dedicatedResourcePackages[]`）的剩余比例；
2. 主值（卡片大数字 / 托盘主聚合）为全部额度桶中最紧张者的剩余百分比；
3. 国际版（qoder.com）与国内版（qoder.cn）账号均可用，用户无需在设置里手动选择区域。

## 决策

| 项 | 决策 |
| --- | --- |
| 区域支持 | 单类型「Qoder」；凭据来源决定区域（本地凭据按文件路径判定，PAT 保留双区域探测兜底） |
| 凭据来源 | **默认**：本机 Qoder / Qoder CN 桌面客户端登录凭据（`auth.v1.dat`，见 v2 节）；**兜底**：用户粘贴 PAT（`pt-` 前缀，存 api_key） |
| 配额展示 | 全部额度桶平铺（计划额度 / 加量包 / 每专属积分包一行） |
| 主值语义 | 全部额度行中最小 `remaining_percent`（与托盘 `primary_quota` 一致） |
| 签到 / 多账号 | 不做 |
| 托盘 emoji | 🟣 |

### 为什么走 PAT 而不是读本地凭据（v1 时判断，已被 v2 推翻）

> **v2 注**：此节是 v1 的决策依据。后续实测发现 PAT 入口实际不存在（见 v2 节背景变更），且 CreditDaddy 公开源码给出了完整的本地凭据解密配方，路线反转。

v1 时判断：本机装了 Qoder 国际版与国内版 IDE，但登录凭据存放在客户端加密存储中，Qoder CLI 也未安装；没有像 `~/.kimi-code/config.toml` 那样可直读的明文凭证文件。当时认为 PAT 是官方功能，稳定性与合规性均优于抓取客户端内部状态。

### 接口来源与非官方风险

接口形态来自社区逆向成果（CreditDaddy、qoder2api 等项目实测可用），非官方文档承诺：

```
POST {base}/api/v1/jobToken/exchange   body {"personal_token": "pt-..."} → {"token": "jt-..."}
GET  {base}/api/v2/quota/usage         Authorization: Bearer <jt>
GET  {base}/api/v2/user/plan           → plan_tier_name
```

国际 base：`https://openapi.qoder.sh`；国内 base：`https://openapi.qoder.com.cn`。
请求头：`Authorization: Bearer <jt>`、`Cosy-ClientType: 10`、`User-Agent: Qoder`、`Accept: application/json`；兑换请求用 `User-Agent: qodercli/1.0.0`、`Cosy-ClientType: 5`、`Content-Type: application/json`。

服务端调整接口时本供应商会报错，不会静默给出错误数值。

## 数据流

每轮调度都通过 `create_provider` 新建实例，`QoderProvider` 内用 `OnceCell` 缓存本轮结果：`fetch_balance` 与 `fetch_quota_infos` 共享同一组网络请求（与 `KimiCodeProvider` 同模式）。

```
api_key 非空（PAT 模式）:
  read PAT → resolve_token: pt- 开头则兑换成 jt；否则原样使用
  → 探测区域（先国际后国内，凭据被拒/404 切换）→ 解析 quota/usage → plan（尽力）

api_key 为空（本地凭据模式，默认）:
  扫描 ~/Library/Application Support/com.qoder.app*/auth.v1.dat（国际）
        ~/Library/Application Support/com.qodercn.app*/auth.v1.dat（国内）
    → 多候选取 mtime 最新 → 钥匙串密码解密 → JSON 取 token（dt-）
  → 按文件来源直接选 base（不盲探）→ GET quota/usage → plan（尽力）
```

PAT 模式不做跨轮区域缓存：每轮最多多一组请求（默认 120s 刷新间隔），换取无全局状态、账号跨区域切换零感知。本地凭据模式每轮重读文件——客户端每 60±5 分钟自动刷新 token，重读即自动跟进。

## v2 方案切换（2026-11-01）：本地凭据替代 PAT

### 背景变更

用户实测确认：Qoder 控制台不提供可复制的外部 PAT（「Settings → Personal Access Tokens」入口实际不存在），v1 的粘贴路径对真实用户不可达。社区工具（CreditDaddy）已验证可行路线：读取本机 Qoder / Qoder CN 桌面客户端的登录凭据。协议细节全部来自其公开源码（`src/qoderApp.js` / `src/qoderClient.js` / `src/constants.js`）。

### 凭据链（macOS）

1. **候选文件**：`~/Library/Application Support/com.qoder.app*/auth.v1.dat`（国际）、`~/Library/Application Support/com.qodercn.app*/auth.v1.dat`（国内，本机实存 `com.qodercn.app.stable`）；多候选取 mtime 最新（最近活跃登录）。
2. **钥匙串密码**：`security find-generic-password -w -s "<N> Safe Storage"`，N 依次试 `Qoder App`、`Qoder CN`。首次访问触发 macOS 标准授权弹窗，用户点「始终允许」后永久静默；应用内再叠加进程级密码缓存（内存，不落盘），即使用户只点「允许」也不会每轮弹窗。
3. **解密**：文件 = 前缀 `v10` + AES-128-CBC 密文（IV = 16 个空格 0x20），密钥 = PBKDF2-SHA1(密码, `"saltysalt"`, 1003 轮, 16 字节)，PKCS7 去填充。
4. **明文 JSON**：`{token: "dt-…", refreshToken, expiresAt, user: {id, name, email}}`。只取 `token`，其余字段（含 refreshToken）不落日志。

### 调用链变更

- `dt-` 客户端会话 token **直接作 Bearer**，无需 jobToken 兑换（仅 `pt-` PAT 走兑换，CreditDaddy `resolveToken` 同判据）。
- 区域不再盲探：`com.qodercn.app*` → 国内 base，`com.qoder.app*` → 国际 base。
- 查询头补齐对齐客户端：`Cosy-Version`、`Cosy-MachineOS: macos`（原有 `Cosy-ClientType: 10`、`User-Agent: Qoder` 不变）。
- 不实现 token 刷新（refreshToken 流程需要客户端风控签名，超出监控工具边界）；token 失效时报错引导用户打开客户端重新登录。
- 安全边界：应用只读用户自己的凭据文件与用户自己的钥匙串项（与 Claude Code / Kimi Code 供应商同性质）；token 不写日志、不落盘、不外传。

### 设置页与调度

- Qoder 加入免密供应商名单：`scheduler.rs` 的 `needs_api_key` 豁免 + `Settings.tsx` 的 `CLI_OAUTH_PROVIDERS`，UI 提示「需已登录 Qoder / Qoder CN 桌面客户端」。
- api_key 留空 = 本地凭据模式（默认）；非空仍按 PAT 处理（保留 v1 逻辑作为高级兜底，测试继续覆盖）。

### 错误文案（本地凭据模式新增/变更）

| 场景 | 文案 |
| --- | --- |
| 未找到凭据文件 | 「未找到 Qoder 客户端登录凭据（auth.v1.dat），请先安装并登录 Qoder / Qoder CN 桌面客户端」 |
| 钥匙串读取失败 | 「无法读取 Qoder 钥匙串密钥（<原因>），请在 macOS 授权弹窗中点击允许」 |
| 解密/解析失败 | 「Qoder 本地凭据解析失败（<原因>），客户端格式可能已变更」 |
| 本地 token 被拒（401/403） | 「Qoder 本地凭据已失效，请打开 Qoder / Qoder CN 客户端重新登录后重试」 |

PAT 模式文案保持 v1 不变。

## 数据映射

`quota/usage` 响应字段：`userQuota` / `addOnQuota` 形如 `{used, total, remaining, resetTime}`（`resetTime` 兼容 ISO 字符串与数值两种形态）；`dedicatedResourcePackages[]` 元素含 `id, name, used, total, remaining, unit, expiresAt, status, available`；顶层另有 `isQuotaExceeded`、`expiresAt`。

| 响应桶 | QuotaInfo 行 | 规则 |
| --- | --- | --- |
| `userQuota` | 名称「计划额度」 | `remaining_percent = remaining/total×100`（total=0 但 remaining>0 时按 100）；`current_value = remaining`（绝对 Credits）；`reset_at` 由 resetTime 转 RFC3339 |
| `addOnQuota` | 名称「加量包」 | 同上 |
| `dedicatedResourcePackages[]` 每包 | 名称取包 `name` | `expiresAt` 是**到期**不是窗口重置：沿用 Kimi 订阅余额行先例，`reset_time` 写到期文本、`reset_at = None`，避免倒计时语义错位；`available == false` 或 `total == 0` 的包跳过 |
| `plan_tier_name` | 各行 `plan_label` | 请求失败时缺省「Qoder」 |

- `used` / `total` / `remaining` 三者为 0 的桶视为缺省，不生成行（与 qoder2api 的判定一致）。
- 顶层 `isQuotaExceeded` 与账号级 `expiresAt` 本次不参与展示：额度是否耗尽以各桶 `remaining_percent` 为准，避免两套口径互相矛盾。
- `BalanceData`：`currency = "%"`；`available_balance = total_balance = 全部行最小 remaining_percent`；无任何行时为 100。
- 行全空时 `fetch_quota_infos` 返回 `None`（与 Kimi Code 一致）。
- 卡面只显示百分比；绝对 Credits 数值进 `current_value` 供后续迭代，本次不改 UI。

## 错误处理

| 场景 | 文案 |
| --- | --- |
| PAT 为空 | 调度层按现有规则跳过（加入 needs_api_key 判定，不加免密名单） |
| 兑换失败（401/403） | 「Qoder PAT 无效或已过期，请在 Settings → Personal Access Tokens 重新创建」 |
| 国际 + 国内均 401/403 | 同上 |
| 非鉴权类非 2xx | 「Qoder API error: <status>」 |
| 响应解析失败 | 「Qoder 响应解析失败: <err>（接口可能已调整）」 |
| 网络失败 | 「Qoder 请求失败: <err>」 |

区域探测中任一端点的网络层失败直接返回错误，不切换端点重试。

## 接线触点

沿用仓库既有惯例，以下位置同步新增 `"Qoder"`：

- `src-tauri/src/providers/qoder.rs`（新增）：`QoderProvider` 实现 `LlmProvider`
- `src-tauri/src/providers/mod.rs`：`mod qoder;`、`ALL_PROVIDER_TYPES`、`create_provider`、`display_name`
- `src-tauri/src/tray.rs`：`tray_short_code` → `"QD"`（遍历守卫测试自动覆盖）
- `src-tauri/src/scheduler.rs`：emoji → `"🟣"`；**不**加入免密名单
- `src/components/Settings.tsx`：`PROVIDER_OPTIONS` 增加 `{ value: 'Qoder', label: 'Qoder' }`
- `src/components/UsagePanel.tsx`：`BRAND_LOGOS` 增加 `Qoder: '/providers/qoder.png'`
- `public/providers/qoder.png`：官方 logo

`short_window` 无需新增映射：「计划额度」「加量包」本身已足够短；专属包名原样透传。

## 测试

Rust 单测（`qoder.rs` 内，网络函数依赖注入或拆纯函数解析）：

1. `quota/usage` 反序列化：三桶齐全、缺字段、total=0、resetTime 两种形态；
2. 行映射：userQuota / addOnQuota / dedicatedResourcePackages 的名称、剩余%、`reset_at` 有无（专属包必须为 None）；
3. 空桶过滤：三角全零不生成行；`available=false` 的包跳过；
4. 主值：多行时取最小剩余百分比；
5. 错误映射：401/403 → PAT 文案；500 → 通用文案；
6. 区域探测：先国际失败后国内成功（用纯函数 `select_region` 或可注入的请求层覆盖）。

守卫测试：`ALL_PROVIDER_TYPES` ↔ `display_name` ↔ `tray_short_code` 的现有遍历守卫应自动覆盖新类型。

## 验收标准

1. 已登录 Qoder / Qoder CN 桌面客户端的机器上，设置页添加「Qoder」（无需粘贴任何 Key）后，托盘与卡片显示多行配额（计划额度 / 加量包 / 各专属包）；首次出现钥匙串授权弹窗，允许后不再重复。
2. 粘贴国际版或国内版 PAT 的兜底模式仍能取到数据，无需手动选择区域。
3. 本地凭据缺失 / 失效、PAT 无效分别显示对应的明确提示，而非裸 HTTP 状态码。
4. 专属积分包行显示到期时间但不出现重置倒计时。
5. `cargo test`、`cargo clippy`、`npm run build`、ESLint 全部通过。
6. 主窗口与托盘布局在新增该供应商后无破版（行数增多时正常滚动/换行）。

## 不在本次范围

- 每日签到 / 自动领取 Credits；
- 多账号管理（多凭据文件只取 mtime 最新一份）；
- 网页会话 Cookie 路径与逐资源包明细接口（`/api/v2/me/usages/big_model_credits`）——只认浏览器 Cookie；
- 卡面展示绝对 Credits 数值；
- refreshToken 自动刷新流程（需客户端风控签名）。
