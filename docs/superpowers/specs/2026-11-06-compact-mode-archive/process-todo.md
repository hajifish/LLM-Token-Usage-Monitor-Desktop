# compact-mode todo

## Feature playbook steps (poteto-mode)

1. `how` over the affected subsystem.
2. `architect` for parallel design exploration.
3. Write the throughput checkpoint as four todo items. A dimension that genuinely does not apply (single file, no fan-out) keeps its item with `n/a: <reason>` rather than being dropped:
   - **Blocking first steps.** Gates run before fan-out.
   - **Independent workstreams.** Disjoint files, services, or layers parallelize. Shared writes serialize.
   - **Shared mutable state.** Default to splitting the target (the **separate-before-serializing-shared-state** principle skill). Serialize only for real invariants.
   - **Smallest safe decomposition.** If one worker is best, name why.
4. Delegate code-writing to a subagent using your configured feature model (default in poteto-mode's Models section) with a specific scope (file paths, named data shape and its organizing structure per **principle-model-the-domain**, a state machine over scattered booleans, a table/registry over branching, a typed model over repeated shape assumptions, chosen before the delegate writes logic, and success criteria). Pick each subagent's reasoning level for the job, per the Subagents section. When the implementation admits multiple valid shapes (error handling, abstraction layer, test structure), delegate via the **arena** skill instead so the runners surface the alternatives and the cross-judge guards the pick. Mandatory: no skip-with-reason escape, and Laziness Protocol does not override it (the gain is review separation, not lines saved). A subagent forbidden to spawn satisfies this by owning the diff directly with the same review separation. No "standing by" reply that waits on a nested agent. **Give every file-writing delegate its own worktree** (spawn it with `isolation: "worktree"`, or hand it an exclusive branch), and do not write files or run a suite in a worktree a delegate still holds. On Claude Code, `isolation: "worktree"` branches from the remote default branch unless the `worktree.baseRef` setting is `"head"`. When the delegate builds on commits the default branch lacks, commit them, create its worktree with `git worktree add <path> -b <delegate-branch> HEAD`, and name that base commit in the brief. Fencing a file in the brief's prose is not a lock (**principle-separate-before-serializing-shared-state**). Comments per **Comments**. Surgical edits, re-ground against the source for upstream-derived files. Port shared-primitive improvements to all consumers and verify each. Commit liberally.
5. Verify on the matching surface. "Inconclusive" or wrong-surface is not a pass. Flag it.
6. Rebase into small, ordered commits. Stack follow-ups.
   Use the **sequence-verifiable-units** principle skill, building, verifying, and committing each small unit before the next.
7. If the design is contested, `interrogate` before shipping.
8. Run **Opening a PR**.

## Arena rubric（Phase D 评分标准，runner 不可见）

- R1 窗口几何单一所有权：进出精简的 size/position/decorations/置顶收敛在一个模块/命令，幂等可重入，原几何恢复有明确方案；全仓库不出现第二处窗口几何写点。
- R2 模式状态显式建模：一个类型表达"正常/精简"（非散落布尔），持久化决策明确（选/不选都有理由），退出路径清晰，前端渲染分支点 ≤2 处。
- R3 口径复用：精简行的剩余/重置取值与托盘、主页主数字同源（balance.available_balance / 现有 reset 文本或 reset_at 补类型），零或近零新增后端选取逻辑。
- R4 遵守仓库纪律：托盘改动（若有）走句柄托管；配置走读 fresh→save→apply 回弹模式；capabilities 只在选 JS 直调时扩。
- R5 过渡风险有解：冻结背景层、材质观感、红绿灯/装饰、贴顶几何四个未验证点，设计要么给出处理方案要么明确列入实施期实测清单。
- R6 最小合理 diff：无新依赖、无新 CSS 体系、落点符合现有文件组织；后续扩展（如托盘开关）不需返工。

## Arena Phase D picker 评分（候选 A=opus B=fable C=sonnet D=haiku，0-3 分/条）

| | R1 | R2 | R3 | R4 | R5 | R6 | 计 |
|---|---|---|---|---|---|---|---|
| A | 3 | 3(null门) | 3(CompactCell) | 2(命令内不落盘,前端两步) | 2.5(材质接管,观感未决) | 2.5 | 16 |
| B | 3(收敛表+saved守卫+实测work_area) | 3(null门+不变式) | 2.5(primary_quota,判级在TS) | 3(命令内原子落盘) | 2.5(cover裁剪有条带风险) | 3(不动主页口径) | 17 |
| C | 3 | 2(无null门,首帧可能挤错形) | 2(TS新阈值表) | 3 | 3(胶囊,双风险绕开) | 2(顺带迁移UsagePanel,diff最大) | 15 |
| D | 2.5(双命令) | 2(无null门) | 3(+warning_level,阈值单源Rust) | 2(前端两步落盘) | 3(胶囊) | 3(最小diff) | 15.5 |

四家收敛点：Rust 单命令独占几何、compact_mode 持久化+启动预套、保留红绿灯不碰 set_decorations、alwaysOnTop、minimize-2/maximize-2 图标对、不加设置页开关、不加托盘菜单项、内存级几何备份。

拟定合成（待裁判对照）：B 为基；嫁接 D 的 warning_level wire 字段（阈值单源 Rust）；A/C 的空态"打开设置"动作；单行 chips + 超宽滚动（C）；活倒计时 reset_at+30s（多数派）；条背景默认走 A 的材质接管、C/D 自绘胶囊作实测备选（CSS 一层之差，走查定稿）；B 的 0.8 宽比 + 高 64 为初值走查校准。

## Project gate (CLAUDE.md, overrides playbook ordering)

- [ ] GATE: after step 3, present 设计 spec + 实施计划, wait for 人工检查与确认, before any implementation (step 4).

## Progress

**⏸ 2026-11-06 用户暂停（先暂停开发）。恢复点：spec 已写完待人工确认，确认前不动产品代码。**

- [x] 1. how over the affected subsystem — grounding 沉淀于 .audit/compact-mode-grounding.md（含 PR #121 前提变更增补）
- [x] 2. architect design exploration
  - [x] Phase A Ground（复用 how 产物）
  - [x] Phase B arena Fan out（4 runners 完成，候选在各自 worktree 的 compact-mode-design.md）
  - [x] Phase C Cross-judge（裁判：A/D 并列 17，荐 D 为基；并发现 PR #121 前提漂移，已核实并重定分支）
  - [x] Phase D/E 最终合成（读双方理由后定：D 为基 + B 的原子落盘单命令/收敛表/work_area 实证 + A 的 null 门 + C 的启动快照/降级/滚动/空态动作；上表"拟定合成"行是裁判前的旧稿，以本行为准）
- [ ] 3. throughput checkpoint（实施前写）
- [~] GATE: spec 在 docs/superpowers/specs/2026-11-06-compact-mode-design.md（未提交），§6 六个拍板项待用户定夺
- [ ] 4. delegate implementation（暂停于此，未开始）
- [ ] 5. verify on real surface (project `verify` skill)
- [ ] 6. small ordered commits
- [ ] 7. interrogate (only if design contested)
- [ ] 8. PR opened
