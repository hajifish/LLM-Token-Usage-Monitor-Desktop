# 背景固化（固定背景 / 跟随系统）设计

- 日期：2026-11-04
- 分支：`feature/freeze-background`（自 develop `f207ac7` 创建）
- 状态：设计定稿，待写实施计划

## 背景与目标

应用窗口为 `transparent: true` + `hudWindow` 材质（tauri.conf.json），页面 CSS 背景近乎全透明，观感 = 材质对桌面壁纸的实时模糊/暗化。壁纸被其他软件更换后，应用配色随之漂移，有时非常怪异。

本次目标：

1. **固化**：把「此刻」窗口的半透明背景（材质 × 当前壁纸的合成观感）截成一张图存入应用数据目录，此后窗口铺这张图，不再透出真实桌面；换壁纸不影响应用观感（核心验收点）。
2. **交互**：设置页新增「背景」区块，仅一个模式开关——「跟随系统」（默认，现状）/「固定背景」。不设单独的「重新截取」按钮：**每次从跟随系统切到固定背景的瞬间即重新截取**，开关本身就是重截入口。
3. **范围外**：托盘菜单入口、多套背景方案管理、手动导入图片、Windows/Linux 适配（本功能仅 macOS：`screencapture`、Screen Recording 权限、`CG*ScreenCaptureAccess` 均为 macOS 机制）。

## 决策

| 项 | 决策 |
| --- | --- |
| 截取机制 | **方案一：区域截屏**。前端先进入捕获态（全部内容 `visibility: hidden`，仅剩窗口材质），Rust 用窗口坐标执行 `/usr/sbin/screencapture -x -R<x,y,w,h>`，截出的就是「纯净的半透明背景效果」。（方案二 `screencapture -l<windowID>` 免坐标换算但需 objc 取 windowID、处理阴影边距，仅作为方案一在多屏失准时的后备，不在本期实现） |
| 屏幕权限 | 截取前 `CGPreflightScreenCaptureAccess()` 预检；未授权则 `CGRequestScreenCaptureAccess()` 触发系统授权框，随后**直接报错返回**（授权须重启应用才生效，当场截图必然失败）。错误走仓库惯例 `Result<T, String>`：后端组好完整中文引导文案（含「授权后需重启应用」），前端原样 toast 并让开关回弹 |
| 交互模式 | 沿用设置页即时生效纪律（`applyTrayDisplay` 同模式）：读最新配置 → 仅改 `background_mode` → 落盘；**后端写入成功才同步本地开关状态**，失败回弹不静默 |
| 模式语义 | 切到固定背景 = 立即截取并覆盖旧图；切回跟随系统 = 恢复实时材质、**不删图**；重启时若已是固定模式，直接加载存量图，不闪屏重截 |
| 渲染 | 固定模式：窗口底层铺图（`background-size: 100% 100%`；窗口 720×600 固定不可调，截图像素 1:1 无裁剪），Rust 侧 `set_effects(None)` 移除实时材质（Tauri 2.11 `set_effects` 接受 `Option`，已核实）；窗口 `transparent` 保持 `true`，切回时 `set_effects(hudWindow)` 恢复，无需重建窗口 |
| 底图圆角 | macOS 窗口圆角外的截图像素是截取瞬间的桌背内容，需按窗口圆角裁掉（底图层 `border-radius` + `overflow: hidden`，具体值实施时对照实机），保持今天的透明圆角观感 |
| 配置 | `AppConfig` 新增 `background_mode: "follow" \| "frozen"`（serde default `"follow"`，旧配置天然兼容），Rust `models.rs` 与前端 `types.ts` 严格同步；背景图不进配置，固定为 `app_data_dir/background.png` |
| 图片加载 | tauri.conf.json 开 `assetProtocol`（`enable: true`，scope `$APPDATA/**`），前端 `convertFileSrc()` 取 URL + 时间戳 query 防缓存；CSP `img-src` 现有配置已含 `asset: https://asset.localhost`，无需改 |
| 时序编排 | 前端编排（见下），Rust 命令保持纯函数性质，不监听前端状态 |

## 截取流程（时序）

```
用户切开关 → 前端进入捕获态 → 等 WebView 重绘完成
  → invoke('capture_background')（Rust：预检权限→算坐标→screencapture→校验→落盘）
  → 成功：读 fresh 配置改 background_mode=frozen → save_config 落盘
          → 退出捕获态、铺底图 → invoke('apply_window_effects', frozen) → 开关同步
  → 失败：退出捕获态、toast 错误、开关回弹、不落盘配置
```

窗口材质的启停**只经 `apply_window_effects` 一条路**（含启动初始化）：`capture_background` 保持纯职责（截取+校验+落盘），避免两处都能改材质导致的状态分叉。

- **捕获态**：根元素加 class（如 `capture-blank`），`.app` 全部内容与面板背景 `visibility: hidden`——材质是窗口级的，内容隐藏后整窗露出纯净材质。用户会看到约 200ms 的闪空，属预期（用户已确认此方案）。
- **重绘等待**：`requestAnimationFrame` ×2 + 约 150ms 定时（实施时定值），确保截图前 WebView 已实际重绘。
- **切回跟随系统**：无截取，直接 save_config（`background_mode=follow`）→ 移除底图 → `apply_window_effects(follow)`。

## Rust 命令设计

### `capture_background() -> Result<String, String>`

1. `CGPreflightScreenCaptureAccess()`（CoreGraphics FFI，直接 `extern "C"` 声明，不引新 crate）；未授权 → `CGRequestScreenCaptureAccess()` 弹系统框 → 返回中文引导错误（含「授权后需重启应用再重试」）。
2. 取主窗口 `outer_position()` / `outer_size()`（物理像素）÷ `scale_factor()` → 逻辑点，格式化 `-R x,y,w,h`（Tauri 全局坐标与 CG 点坐标同空间：原点主屏左上、y 向下）。
3. `Command::new("/usr/sbin/screencapture").args(["-x", "-R…", tmp路径])`；校验通过后覆盖 `app_data_dir/background.png`（先写临时文件，见错误处理）。
4. 校验：文件存在、大小在合理区间（如 20KB–20MB）。**不做像素级尺寸校验**：最终渲染是铺满窗口，尺寸略差只影响清晰度不影响正确性，`background-size: 100% 100%` 天然兜底。

### `apply_window_effects(mode) -> Result<(), String>`

`frozen` → `set_effects(None)`（材质在 webview 之下，不移除时圆角外透明区会继续透出随壁纸变化的材质，违背目标）；`follow` → `set_effects(WindowEffectsConfig { effects: [hudWindow], state: active })`（与 tauri.conf 现配置一致）。启动时 `lib.rs` setup 以内部函数按同一逻辑初始化（frozen 且图存在 → 无材质；否则保留材质），命令与内部函数共用实现。

## 前端设计

### 设置页「背景」区块（置于「低余额通知」之后、「服务商」之前）

- 单开关：`固定背景`（开）/`跟随系统`（关），描述文案说明语义：「开启即截取当前半透明背景并固定；关闭后恢复实时跟随桌面」。
- 开启流程按「截取流程」时序；捕获态与截取期间开关 `disabled`（沿用 trayBusy 模式，可见可预期）。
- 错误分支 toast 文案：权限 → 「需要屏幕录制权限：请在系统设置中授权并重启应用后重试」；截取失败 → 透出后端映射的中文错误。

### 底图渲染（App.tsx / 全局）

- 固定层（`position: fixed; inset: 0`，置于所有内容之下）以 `convertFileSrc(background.png) + ?t=<时间戳>` 为背景图，`background-size: 100% 100%`，圆角裁剪如决策表。
- 捕获态 class 挂在根元素，全局 CSS 一处定义；固定层在捕获态同样隐藏。
- 底图加载失败（`Image` 预加载 onerror）：toast 提示「背景图加载失败」，界面保持可用（最坏情况观感回退为透明）。

### 启动加载

- App 挂载读配置：`background_mode=frozen` 时前端以 `appDataDir()` 拼 `background.png` 路径 → `convertFileSrc()` → `Image` 预加载探测：成功 → 铺图；失败（图缺失/损坏/协议异常）→ 回退跟随外观 + 设置页背景区块显示一次性横幅「背景图加载失败，请重新开启固定背景以重截」。**不为此新增后端字段或命令**：图存在性以预加载结果为准，前端自足。
- Rust 侧启动初始化（材质开关）不依赖前端：`lib.rs` setup 直接查文件存在性 + 配置，决定 `set_effects(None)` 还是保留材质。两侧判定独立、无需握手；极端竞态（启动瞬间文件被删）最坏表现为短暂观感不一致，可接受。

## 错误处理

| 场景 | 行为 |
| --- | --- |
| 未授权 / 刚授权未重启 | `capture_background` 返回后端组好的中文引导文案（含系统设置路径与「需重启」说明）；开关回弹；不落盘 |
| `screencapture` 非零退出 / 无产物 / 产物过小 | 返回映射中文错误；开关回弹；保留旧图不覆盖（先写临时文件，校验通过才覆盖 `background.png`） |
| 启动时 frozen 但图缺失 | 回退跟随系统外观 + 设置页横幅提示；`background_mode` 配置值保持不动（下次重启仍自动重试加载） |
| 底图 URL 加载失败 | toast 提示；界面可用性不受影响 |
| 前端铺图成功但 `apply_window_effects` 失败 | toast 提示（圆角处可能仍透出材质），不影响主流程 |

## 测试与验收

Rust 单测（沿用现有 `cargo test` 基建）：

- `background_mode` serde 兼容：旧配置文件（无该字段）反序列化为 `follow`。
- 坐标换算纯函数：物理像素 + scale → `-R` 参数字符串（含 Retina 2x、副屏负坐标用例）。
- 临时文件 → 校验 → 覆盖的文件管理路径逻辑（截取本身依赖真实屏幕，不在单测覆盖）。

质量门：`npm run build` / `npm run lint` / `npm run format:check`；`cargo fmt` / `clippy` / `test`。

人工走查（`npm run tauri dev`）：

1. 首次开启：闪现纯净半透明 → 固定成功，观感与切换前一致（含圆角、卡片叠加层）；
2. 重启应用：仍为固定背景，无闪屏、无重截；
3. 切回跟随系统：恢复实时半透明；再次开启：按新壁纸重新截取；
4. **系统更换壁纸后：固定模式下应用观感不变（核心目标）**；
5. 未授权路径：明确提示、开关回弹、旧图未被破坏；
6. 多显示器（如有条件）：窗口位于副屏时截取与铺图正确；
7. 注意 TCC 授权绑定二进制：`target/debug` 的 dev 构建与正式安装版需各授权一次。

## 与在途分支的关系

`feature/provider-config-page` 同样改动 `Settings.tsx` / `models.rs` / `types.ts` 且尚未合回 develop。两功能无逻辑依赖；先合并者为准，后合并的分支需 rebase 解决设置页区块与配置字段的合并冲突（本分支新增区块夹在「低余额通知」与「服务商」之间，provider-config-page 会移除「服务商」区块，冲突范围小、语义清晰）。

## 2026-11-05 修订：改为内置资产方案（用户决策）

运行时截屏方案实施完成后，用户澄清真实需求：**开发期**截好一张干净的半透明背景图，随包内置为默认背景（`public/default-background.png`）；应用运行时不截屏、无屏幕录制权限。修订内容：

- 默认模式改为 **frozen**（serde 默认值变更；显式保存过 follow 的旧配置不受影响）。
- 删除 `capture_background` 命令、CG 权限 FFI、assetProtocol 配置、临时文件晋级链路及其单测；Cargo.toml 的 `protocol-asset` feature 同步移除。
- 启动时序：启动时**不动材质**（follow/frozen 均保持 hudWindow 兜底观感），前端预加载并铺好内置图之后再经 `apply_window_effects` 移除材质——图覆盖在材质之上，移除瞬间无观感变化，加载期零闪透明。
- 内置图截取方式（开发期一次性）：临时空窗补丁启动 dev 实例 → `screencapture -R<窗口区域>`（终端已有的权限，应用永不申请）→ 红绿灯按钮区域用邻近材质像素修补。
- 「重新截取」语义删除：更换背景图属开发期资产替换，不再是运行时功能。
- 原「错误处理」表中权限/截取失败/图缺失各分支随之消灭；仅保留 `apply_window_effects` 软失败提示。
