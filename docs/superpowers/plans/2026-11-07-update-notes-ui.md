> 过程档案：update-notes-ui（2026-11-07）。原型对比定稿 modal；独立验证证据见 .claude/verify-evidence/20261107-0034-update-notes-final/（Lane A）与监督者 Lane B 托盘通知记录。原稿 .audit/update-notes-ui.todo.md（工作区草稿，不入库）。

# update-notes-ui — 更新日志展示重设计

## Feature playbook 步骤

1. how 子系统走查 — done inline：子系统共 4 文件（UpdateNotice.tsx / useUpdate.ts / types.ts / app.css 更新段），已全部读过；唯一消费者 App.tsx:110。skip: 子系统过小，独立 how 子代理是仪式
2. architect 并行设计探索 — done as 原型对比（exhaust-the-design-space）：modal vs 内嵌展开两原型，截图人工定夺
3. throughput checkpoint：
   - Blocking first steps：设计定稿（用户人工确认变体）阻塞一切实现 — gate by CLAUDE.md 人工把关流程
   - Independent workstreams：n/a: 单一 UI 表面、3 个文件强耦合（解析器/组件/样式），拆分收益为负
   - Shared mutable state：UpdateNotice.tsx 与 app.css 各只有本任务一个写者，无共享
   - Smallest safe decomposition：单实现者最优；理由：整个改动是一个连贯的展示面 + 一个纯函数解析器，一次评审一个 diff
4. 委托实现（用户确认变体后，feature 模型 subagent，含数据形状与成功标准）
5. verify UI lane 验证（available/ready/下载中 三相 + 弹窗开关 + Esc）
6. 小步提交
7. n/a: 设计无内部争议（用户定夺即终审）
8. Opening a PR

## 任务

- [x] 子系统走查（UpdateNotice/useUpdate/types/CSS/4 份历史发版说明格式）
- [x] shim 加 fireUpdateStatus(patch) 辅助
- [x] 解析器 src/lib/releaseNotes.ts（含无名称条目、脚注、fallback）
- [x] 两原型：modal 弹窗 / 内嵌展开（URL 参数切换，临时代码）
- [x] Lane A 截图对比（available + ready 相位）；证据 .claude/verify-evidence/20261107-0013-update-notes-proto/
- [x] 原型期修掉的真 bug：name/detail 必须同一文本流（flex 并列会让长加粗名碎行）
- [x] 监督者会话回报（SendMessage 已送达）
- [x] 变体裁定 = modal（监督者转达用户免确认授权；理由：默认变体、验证最完整、三相位截图覆盖）
- [x] 扫尾 1：移除 URL 切换临时代码与 inline 整条分支（expanded state / 切换按钮 / update-notes-panel / update-notes-slide keyframe / .update-btn.ghost.active / update-banner-wrap 包裹层）
- [x] 扫尾 2：ready 相位 INSTALL_NOTE 移出滚动区，固定槽 .update-dialog-note-slot 于 footer 上方（Lane A 实测：槽在滚动区外、note 全可见、正文独立滚动、主按钮文案正确）
- [x] 扫尾 3（监督者评审发现，已核实为真 bug）：releaseNotes.ts fallbackText 判据改用过滤后 sections；「有标题零条目」形态回退原文（Lane A 实测 rn-fallback 渲染原文、空分节 0 个）
- [x] 追加：蓝底横幅 ghost 按钮提亮一档（自报待办）
- [x] 静态闸门：eslint 0 / tsc --noEmit 0 / vite build 成功（46 modules）
- [ ] 提交、PR 回 develop（监督者统一分批提交，本会话不 commit）

## 原型环境噪声记录（非组件 bug）

- 截图 PNG 写入工作区会触发 vite watch 整页 reload，mock 状态复位成 idle；对策是点击后立即截图、验完再断言
- 一次未归因的幽灵点击（dialog 打开后被莫名触发 api.download，shim 拒绝留下错误文案）；换精确选择器后未复现
