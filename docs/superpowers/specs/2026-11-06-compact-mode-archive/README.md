# 精简模式设计档案（2026-11-06，暂停待确认）

本目录是"精简模式"功能设计阶段的完整过程档案。功能尚未实施：spec 写完、等人工确认（确认前不动产品代码）。恢复开发时从主 spec 读起。

## 文件清单

- `../2026-11-06-compact-mode-design.md` 主 spec（合成稿，含实施计划与 §6 六个待拍板项）。恢复开发的入口。
- `grounding.md` Phase A 架构摸底（3 个只读 explorer + 1 个合成者的产物；开头有 2026-11-06 增补，记录 PR #121 造成的前提变更：主页设置/Skills 按钮已移入托盘）。
- `candidates/` Phase B 四个并行候选完整设计稿，按 runner 槽位命名（a=opus 槽、b=fable 槽、c=sonnet 槽、d=haiku 槽）。四家在核心架构上收敛，分歧与嫁接价值见 cross-judge.md。
- `cross-judge.md` Phase C 裁判报告（独立代理盲评：评分、事实核查、基准推荐 D、嫁接清单），末尾附 picker 侧独立评分与最终裁定。
- `process-todo.md` 过程清单（Feature playbook 步骤、rubric、双方评分表、暂停点记录）。

## 恢复方式

主 spec §6 六个拍板项定夺后，按 §5 六步实施计划开工（每步一个可验证 commit）。设计过程不需要重跑：四候选、评分、前提变更都已在档案里。
