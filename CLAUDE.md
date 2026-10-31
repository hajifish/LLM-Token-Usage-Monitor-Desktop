# CLAUDE.md

## 分支与开发流程（原则）

- 本地开发集成分支默认为 **develop**；main/master 为发布线。开发新功能时：先从远端更新 develop 与 main（fetch + fast-forward 到各自最新），再从**最新 develop** 创建 `feature/<名>` 分支开发，不直接在 develop 上堆功能提交。
- 远端尚无 develop 时，从 master 最新提交创建本地 develop，需要时再推送远端。
- 新功能沿用现有人工把关流程：设计 spec → 实施计划 → **人工检查与确认后才执行** → 完成后人工验收，再合回 develop。

## 版本号与发版机制（重要，勿凭直觉操作）

- 版本号 = 发布 tag，格式必须是 `vX.Y.Z`（如 `v0.3.0`）。release.yml 构建时直接取 `${tag#v}` 写入 5 处清单；号段由人工决定，全局只升不降（应用内更新按 semver 比较）。
- 发版流程：develop → master 合并 → 手动 `git tag vX.Y.Z && git push origin vX.Y.Z` → tag push 自动触发 release.yml → 等草稿 Release 构建完成 → 手动 publish。
  - 不要再用「自动推 tag」的自动化（旧的 auto-tag.yml 已删除）：用 GITHUB_TOKEN 推的 tag **不会级联触发** release.yml。
  - 兜底：tag 未能触发时 `gh workflow run release.yml --ref <tag>`。
- 发布产物命名由 `scripts/normalize-release-assets.sh` 统一为 `TokenUsage-X.Y.Z-<arch>.<ext>`（Tauri 默认产出的是 `Token.Usage_X.Y.Z_aarch64.dmg` 这种带点的名字），latest.json 里的下载地址同步改写。产物集合变化时同步更新该脚本的映射表，否则发布会在 normalize 步骤直接失败。
- 静态版本号（package.json / package-lock.json / Cargo.toml / Cargo.lock / tauri.conf.json）是 **dev-only 元数据**：仅供本地开发构建展示。**不要**把它手工对齐到最新发布版本。改静态版本号时 5 处必须一起改（CI 的 quality job 有一致性守卫，不一致即失败）。
- 发布草稿（draft Release）是更新分发闸门：手动 publish 后 `releases/latest/download/latest.json` 才对外可见。
- 应用内更新语义：更新只升不降；debug 构建跳过自动检查且拒绝安装（设计如此）。
- 更新签名密钥：公钥在 tauri.conf.json，私钥在仓库 Secrets（`TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`）。私钥丢失 = 全部存量用户无法再收到更新。
