# 精简模式四候选交叉评审（arena Phase C 裁判报告，2026-11-06）

裁判为独立只读代理，按候选文件路径盲评（不见模型名）。评分 0-3 分/条，rubric 六条见 process-todo.md。

## 评分表

| | R1 | R2 | R3 | R4 | R5 | R6 | 合计 |
|---|---|---|---|---|---|---|---|
| A | 3 | 3 | 3 | 3 | 3 | 2 | 17 |
| B | 3 | 3 | 2 | 2 | 3 | 3 | 16 |
| C | 3 | 2 | 3 | 2 | 3 | 2 | 15 |
| D | 3 | 2 | 3 | 3 | 3 | 3 | 17 |

**A**
- R1=3：`apply_shape` 是全仓唯一几何写点、命令与 Ready 共用、shape 守卫幂等 + 绝对值重设、Physical→Physical 恢复明确；唯一瑕疵是回退路径手抄 720×600 常量。
- R2=3：`boolean|null` 三态含启动 null 门（A/B 独有，防止错形首帧闪现），持久化理由充分（background_mode 先例），单出口哲学；背景门 + version-tag 门 + 主门共 3 处，超预算 1 处（轻微）。
- R3=3：CompactCell 全部在 Rust 由既有 `tray::primary_quota`/`quota_warning_level`/`tray_value` 组装，零新增选取逻辑，reset_at+reset_text；明确选托盘侧并记录主页漂移不扩大。注意：投影丢掉币种信息（见设计缺陷节）。
- R4=3：hook 严格走读 fresh→save→apply→回弹；capabilities 零扩；托盘零改动。
- R5=3：冻结背景用"图与材质永不同时缺席"时序主动消解，五个 grounding 风险全部有处置或 Lane B 实测项；work_area 用回退分支对冲（实际已可用，对冲多余但无害）。
- R6=2：CompactCell wire 类型 + TS 镜像 + scheduler 填充 + 新 countdown.ts（Rust 格式平行复制）是四者中最重的非必要 diff；丢 unit 信息为后续 USD 蓝色着色埋下返工点。

**B**
- R1=3：compact.rs 单一写点 + config 幂等闸 + saved 空槽才捕获 + setup 观测 default_size（无手抄常量），四者中唯一给出完整逐失败点收敛表。
- R2=3：同样的三态 null 门，持久化理由充分，出口清晰；渲染分支点在预算内（背景图保留走 cover 覆盖）。
- R3=2：值经复用 primary_quota 同源，但 `warningLevelOf` 在 TS 第三次实现 quota_warning_level 阈值——正是 grounding 点名（也是 B 自己约束 6 承诺不做）的第三份漂移口径；tray_value 格式也被复制。
- R4=2：命令内部走 persist_config（引 set_refresh_interval 先例，先例属实）而非 rubric 点名的前端读改写回弹纪律；capabilities/托盘干净。
- R5=3：唯一对锁定版本 tauri 2.11.6 实证 `Monitor::work_area()` 存在（含 PhysicalRect 字段公开性）的候选——本仓最大几何未知点被它消掉；装饰规避、cover 裁剪、其余风险全列举。
- R6=3：compact.rs + hook + CompactBar + compactModel + BrandLogo 抽取（migrate-callers 同波完成），无依赖、单 CSS 节，文案修改由入口替换直接证成。

**C**
- R1=3：单一模块 + band_rect 纯函数 + 空槽才捕获；启动路径直接调 `enter()`，窗口尚在 conf 几何时自然快照——全仓无 720×600 常量（四者中最优雅）。
- R2=2：`'compact'` 进 view 联合类型是四者中最好的类型表达，但无 bootstrap 门——启动时普通 UI 会在已变条形的窗口里闪现数帧（B 的文档点名了这个失败形态，A/B 用 null 门防住，C/D 均漏）；exit 失败仍强制切设置页，设置页困在条形几何里无出口。
- R3=3：primary_quota 上线复用托盘定义；唯一字面达成"托盘、精简条、主页主数字三方同源"的设计（靠把主页 % 主数字切到 primary 口径，已声明并给 PR 拆分选项）。
- R4=2：命令内 persist（同 B 的偏差，同样有先例与论证）；capabilities/托盘干净。
- R5=3：自绘近不透明胶囊把冻结背景失真（R5）与 hud 材质观感（R1）两个风险结构性消灭；剩余几何/红绿灯风险逐条列举并定义降级形态。
- R2/R6 扣分主因：把主页 % 主数字切换 + 告警变敏感这类用户可见行为变更捆进精简模式 PR（C 自己也承认可拆），并重构 UsagePanel 到 format.ts——超出最小 diff。

**D**
- R1=3：边沿独占快照（Some 不覆盖）+ take() 消费 + "置顶⇔精简几何"由单一所有者结构性编码；Ready 与命令共用内核；720×600 回退常量是唯一瑕疵。
- R2=2：裸 boolean 无 bootstrap 门——精简启动时普通视图在条形窗口内闪现（同 C 的漏）；version-tag 未门控会漏进条内；按钮退出不 await、navigate 退出 await，时序不一致。
- R3=3：primary_quota + warning_level 双字段 Rust 侧派生（零新增选取逻辑、TS 无阈值表），格式化抽取 formatBalanceValue 与主页共用（单一实现），reset_at 活倒计时 + reset_time 兜底 + 过期隐藏对齐 tray 语义。
- R4=3：hook 严格走读 fresh→save→apply→回弹且失败 best-effort 回写配置（A 漏了这步）；capabilities 零扩；托盘零改动。
- R5=3：胶囊消除背景/材质两风险；1 帧瞬态接受并给出换序备选；隐藏窗口 current_monitor、红绿灯垂直落点、resizable 等新风险全部列实测。
- R6=3：仅 2 个 wire 字段、无新模型文件、抽取无行为变更、零依赖；新表面（托盘开关、主页迁移、通知复用）可直接由原语组合，无需动 wire。

## 事实核查

未发现候选对代码库的实质性错误陈述。抽查全部命中：行号引用（App.tsx 设置按钮、tray.rs primary_quota/quota_warning_level/quota_countdown_compact/TrayUpdateItem/sync_tray_click_flag、models.rs reset_at、types.ts 缺口、app.css frozen 层、background.rs、capabilities、scheduler.rs set_refresh_interval/poll_providers、commands.rs persist_config 为 pub(crate)、useFrozenBackground、useUsage 乐观行、UsagePanel 各段、verify SKILL.md 14 命令）均对；B 的 API 实证为真（Cargo.lock 锁 tauri 2.11.6；`pub fn work_area(&self) -> &PhysicalRect<i32, u32>` 在 tauri-2.11.6/src/window/mod.rs:96；PhysicalRect 字段 pub 在 tauri-runtime-2.11.3/src/dpi.rs:28-32）。唯一引用瑕疵：B 称回归测试在 tray.rs:882-895，实际 `v2_alert_source_matches_display_value` 在 L894-908——实质正确，行号偏移。

**共享前提漂移（synthesis 已处理）**：四候选基线是 main（aa4d1eb），而 develop 已合入 PR #121（71efe9f"主页移除 Skills/设置按钮"），主页设置/Skills 按钮已删、托盘新增 Skill & Plugin 项、navigate 已认 'skills'。"替换设置按钮"的入口前提在集成分支上不成立（改为新增按钮落位右上角），navigate 守卫需覆盖 settings 与 skills 两目标。已核实并落进最终 spec。

**设计性事实缺陷（已计入评分）**：
- A 的 CompactCell 只有 value_text/level/reset_at/reset_text，丢掉币种/unit——CompactBar 无法做 USD 蓝色点缀，加回即 wire 变更。
- A 的 hook 分两次 IPC（save_config 后调 set_compact_mode），命令失败时 UI 回弹但磁盘 config 仍为 true——A 未分析这一致窗口；B 有收敛表、D 有显式回写，A 独缺。
- C/D 无启动 null 门；C 的 exit 失败仍导航进条形窗口里的设置页。
- B 的 `.app-compact` 覆盖类与其 JSX 草图不一致（草图级裂缝，非结构缺陷）。

## 基准推荐：D

A 与 D 同分（17），按接口深度与边界干净度判 D 胜出：
- **边界是可组合的原语而非封闭投影**。D 上线 `primary_quota` + `warning_level` 两个域事实；未来表面（托盘开关、主页口径迁移、通知复用、tooltip）无需改 wire 即可组合。A 的 CompactCell 是为这一根条定制的封闭投影——已实际丢了视图需要的 unit 事实，第一次扩展就要动 Rust 结构 + TS 镜像 + fixtures 四处同步。
- **不变量是结构编码的**：D 的"置顶⇔精简几何"由单一所有者函数对编码、快照只在正常→精简边沿写入、take() 消费——无人能只改一半；A 依赖 ShapeState.shape 与 AppConfig.compact_mode 两份真值 + 前端两段 IPC 的时序自律。
- **纪律样板最贴合**：D 的 hook 是 useFrozenBackground 同构（读 fresh→改一字段→save→invoke→回弹→失败回写），配置与几何的失败路径闭合。
- D 的可修补面都是小 graft；A 的修补（unit 字段、配置回写）要动它的核心边界。

## 落选候选的嫁接清单

**从 A（并入 base）**：
1. `boolean|null` 三态 + 启动 null 门（`compact === null` 时渲染空）——直接修掉 D 唯一的实质 R2 缺陷，改动局部：hook 初值 + App 首帧分支。
2. enter/exit 时序逐行注释纪律（每步顺序与"图/材质永不同时缺席"不变量写进 hook 注释）。

**从 B（并入 base）**：
1. 失败收敛状态表作为实施清单与测试用例（尤其"saved 为 Some 则跳过捕获"防横条几何覆盖存档）。
2. 两条实证：`Monitor::work_area()`/PhysicalRect 字段在锁定版本的存在性证明；命令入口的 config 幂等早退闸。

**从 C（并入 base）**：
1. 启动基线自然快照：Ready 路径也走同一条 capture-then-apply 边沿——窗口此刻还在 conf 几何，快照即创建几何，删掉回退路径里手抄的 720×600 常量。
2. 降级形态显式定义（启动套用失败 → 磁盘 compact 而窗口常态，退出按钮自救）。

## Rubric 歧义（影响排序的三处）

1. R3"与托盘、主页主数字同源"按字面不可满足（托盘与主页本就漂移）；实际按"零新增选取逻辑 + 不引入第三份漂移 + 立场显式"评分。
2. R2"渲染分支点 ≤2 处"未定义计数口径（属性级小门算不算），A 在 2-3 之间摆动。
3. R4 只点名前端配置纪律，但仓库存在 Rust 侧直写先例（set_refresh_interval→persist_config）；从严从宽摆动 B/C 各 1 分，不改变基准结论。

（picker 侧独立评分为 B 17 / A 16 / D 15.5 / C 15；与裁判在基准上不一致，读双方理由后的最终裁定：D 为基 + B 的原子落盘单命令与收敛表 + A 的 null 门 + C 的启动快照/降级/滚动/空态动作，详见主 spec §2。）
