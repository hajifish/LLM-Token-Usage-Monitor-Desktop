---
name: running-pr-review-loop
description: Use when a PR has just been created (e.g. after pr-new/commit-pr skills) or the user asks to review a PR with a fix-and-re-review loop until mergeable. Triggers: 发起 PR 评审、评审这个 PR、PR review、评审循环、直至可合并.
---

# Running the PR Review Loop

迭代式 PR 评审协议：独立评审任务把问题**直接发到 PR 评论** → 反馈控制会话 → 控制会话修复并 push → 唤醒**同一**评审任务复审 → 循环到 MERGEABLE → 通知用户，由用户决定合并。

## When to Use

- PR 已创建，用户要求评审（「发起 PR review」「评审这个 PR」）
- 用户期望闭环：发现问题 → 修复 → 二次评审，直到可合并

**When NOT:** 用户只要一次性口头评审结论；PR 尚未创建。

## Protocol（控制会话执行）

1. **派评审任务**：dispatch 一个 general-purpose subagent，提示词用 [review-task-template.md](review-task-template.md)（同目录），模板全部 9 个占位符逐项填写（PR_URL / PR_NUMBER / BRANCH / BASE_BRANCH / REPO_ROOT / PR_SUMMARY / SPEC_DOCS_HINT / RISKY_BITS / QUALITY_GATES；无 spec 文档时 SPEC_DOCS_HINT 填「无 spec 文档，以 PR 描述为准」）。**派发前记录分支 HEAD SHA**——它是第 2 轮复审「增量在 <SHA> 之后」的基准。
2. **等报告**：评审者最终消息含 verdict（NEEDS_FIXES | MERGEABLE）与已发评论摘要。
3. **NEEDS_FIXES → 修复**：控制会话自己修，或派修复 subagent（契约：过质量门、不改 PR 评论、报告含测试证据——评审者永不改码）。push 前跑全部质量门。
4. **复审**：`SendMessage` 唤醒**同一个**评审 agent（上下文保留），注明「第 N 轮复审，增量在 <SHA> 之后，重点看增量但需复核集成」。
5. **MERGEABLE → 停**：向用户汇报并等待合并指示。**不自动合并。**

## Known Pitfalls（实测）

| 坑 | 处理 |
| --- | --- |
| gh 账号 = PR 作者，GitHub 拒绝 `--approve`（self-approve） | 评审者回退 `gh pr review <N> --comment`，正式判定写在评论开头注明 |
| 目标为 develop 的 PR 无 CI checks（build.yml 仅对 master 触发） | 无 checks ≠ CI 失败；以 `gh pr checks` + `gh pr view --json mergeable,mergeStateStatus` 判定 |
| 每轮多条评论刷屏 | 每轮至多一条评审评论（结构化：严重/建议/已核实/结论） |
| 评审者把自己当修复者 | 模板里已声明只读；修复责任在控制会话 |
| 评审者评论发送失败（重试一次仍败） | 评审者把完整发现放进最终消息并注明「评论未发成，请控制会话代发」；控制会话代发时注明系评审者结论 |

## Common Mistakes

- 修复后**重新 dispatch 新评审 agent**（丢上下文）→ 必须 SendMessage 原 agent
- MERGEABLE 后直接合并 → 必须先问用户
- 把发现只留在会话里不发 PR → 协议核心就是评论落 PR，供人工复核与留痕
