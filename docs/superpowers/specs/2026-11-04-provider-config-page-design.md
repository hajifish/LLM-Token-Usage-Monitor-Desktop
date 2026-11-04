# 供应商配置独立页设计

- 日期：2026-11-04
- 分支：`feature/provider-config-page`
- 状态：已确认设计（待实施）

## 背景与目标

当前供应商增删改与「状态栏显示」「低余额通知」等全局设置混在同一设置页，页面职责过重。本次调整：

1. **新增供应商配置页**：主页右上角「设置」图标左侧增加「+」按钮，点击进入独立的供应商管理页，可新增、编辑、删除供应商；页面风格与设置页一致，有「保存」与「返回」。
2. **精简设置页**：供应商相关功能从设置页移除，仅保留菜单栏指示（托盘显示）与余额/配额告警通知等全局设置。
3. **范围外**：「托盘图标点击行为」（打开主页 vs 展示托盘详情菜单）为后续可能的设置项，本次不做，Rust 侧零改动。

## 决策

| 项 | 决策 |
| --- | --- |
| 页面定位 | 「+」打开的是**完整管理页**：展示已有供应商列表，页内完成新增、编辑、删除；主页不设其他编辑入口 |
| 路由 | App.tsx `view` 扩展为 `'usage' \| 'settings' \| 'providers'`，纯前端状态路由，不引路由库 |
| 「+」按钮 | 复用 `settings-icon-btn` 样式（加号 SVG，`title="供应商配置"`），置于 header-actions 中刷新按钮与设置图标之间 |
| 设置页「保存」 | **移除**。剩余设置项全部即时生效，保存按钮成为死功能；header 仅留「← 返回」 |
| 健康横幅归属 | unreadable / deferred / migrated 三条配置健康横幅**迁入供应商配置页**（均与密钥、保存风险相关；设置页不再保存，横幅无处着落） |
| 保存语义 | 供应商页保存改为**反向合并**：取 fresh 配置后只替换 `providers` 数组，其余字段以 fresh 为准（见下） |
| 新组件 | `src/components/Providers.tsx`，结构复刻 Settings（`settings` 容器 + `settings-header` + `settings-body`） |
| 共享常量 | 新建 `src/providerMeta.ts`（零依赖）：`PROVIDER_OPTIONS`、`providerLabel()`、`KEYLESS_PROVIDERS`、`IS_MAC` 及「与 Rust `ALL_PROVIDER_TYPES` 同口径」同步提醒注释 |
| 样式 | 全部复用现有类，预期 app.css 无改动或仅极小调整 |
| 数据结构 | `ProviderConfig` / `AppConfig` 不变，旧配置天然兼容，无迁移 |

## 数据流与字段所有权

供应商配置页的保存逻辑与现 Settings 的差异：

- **现状（Settings.handleSave）**：页面挂载时的配置快照整份写回，落盘前仅从 fresh 回填 `refresh_interval` / `auto_check_update`。
- **新（Providers 保存）**：`invoke('get_config')` 取 fresh → 仅替换 `fresh.providers` 为页面状态 → `save_config` 落盘。`refresh_interval` / `auto_check_update` / `tray_text_enabled` / `notifications_enabled` 一律以 fresh 为准。

由此字段所有权收敛为：**`providers` 数组归供应商页**（保存时写回），**其余全局字段归设置页/托盘菜单**（即时生效写回）。两页不可能同时打开（单窗口单视图），页面挂载快照即最新，互不回滚对方改动。

`providers[i].show_in_tray` 随数组归供应商页所有：该页不渲染此字段，取挂载快照值原样带回；设置页对它的即时生效编辑与供应商页在时间上天然隔离。

## 组件设计

### `src/providerMeta.ts`（新建）

```ts
export const PROVIDER_OPTIONS = [...];   // 自 Settings.tsx 原样迁出
export function providerLabel(name: string): string;
export const KEYLESS_PROVIDERS: Set<string>;
export const IS_MAC: boolean;
```

Settings 页托盘列表仍需 `providerLabel` 显示供应商名 → 改从本模块导入。

### `src/components/Providers.tsx`（新建）

- `onBack` prop 返回主页。
- header：「← 返回」（`back-btn`）+「保存」（`save-btn-header`，含保存中态）。
- body：三条健康横幅（BANNER 样式常量与 `dismiss` 逻辑自 Settings 原样迁入）→ 供应商卡片列表（类型下拉、别名、启用开关、两段式删除确认、各家 Key/Token 字段与 Qoder 特例渲染分支）→「+ 添加服务商」按钮。
- 保存成功/失败沿用 `save-toast` 提示；保存后重拉健康状态（`refreshHealth`）。
- `handleProviderNameChange`（切换类型时别名保留/置空规则）随迁。

### `src/components/Settings.tsx`（精简）

- 移除：服务商区块渲染、`addProvider` / `removeProvider` / `handleProviderNameChange` / `updateProvider` / `handleSave` / `saving` 状态 /「保存」按钮 / `PROVIDER_OPTIONS` 等常量定义。
- 保留：状态栏显示区块（总开关 + 每供应商 `show_in_tray` 即时生效）、低余额通知区块（开关 + 测试通知）、`applyTrayDisplay` / `applyNotificationEnabled` / `sendTestNotification`、`save-toast`、header「← 返回」。
- `deletingIndex` 删除确认逻辑随服务商区块迁出。

### `src/App.tsx`

- `view` 状态扩展 `'providers'`；「+」按钮 `onClick={() => setView('providers')}`。
- `navigate` 事件监听保持现状（后端目前从不发送该事件，不动）。

## 错误处理

- 供应商页保存失败：沿用现模式——后端已映射中文错误消息，页面 `save-toast` 展示，不静默。
- 健康横幅 dismiss 为会话内状态（`useState`），迁页行为不变；「已升级为加密存储」的一次性 localStorage 提醒键（`llmtm.migratedNoticeShown`）保持原键不变。

## 测试与验收

前端无测试框架，质量门为：

1. `npm run build`（tsc + vite）通过；`npm run lint`、`npm run format:check` 通过。
2. `npm run tauri dev` 人工走查：
   - 主页「+」进入供应商页、返回、保存链路（新增/编辑/删除/切换类型/别名规则）；
   - 设置页：托盘总开关与单供应商 `show_in_tray` 即时生效、通知开关与测试通知；
   - 供应商页保存后，设置页字段（如托盘开关）不被回滚（反向合并语义）；
   - 旧配置启动无迁移提示、功能正常。
