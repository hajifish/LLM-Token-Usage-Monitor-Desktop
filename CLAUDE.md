# CLAUDE.md

## 版本号与发版机制（重要，勿凭直觉操作）

- 静态版本号（package.json / package-lock.json / Cargo.toml / Cargo.lock / tauri.conf.json）是 **dev-only 元数据**：仅供本地开发构建展示。**不要**把它手工对齐到最新发布版本——真实版本号由 release.yml 构建时按 `v*` tag 总数派生：`0.(2+COUNT/256).(COUNT%256)`，全局只升不降（应用内更新按 semver 比较）。
- 改静态版本号时 5 处必须一起改（CI 的 quality job 有一致性守卫，不一致即失败）。
- auto-tag.yml 对每次 master push 自动创建日期 tag `vYYYY.MM.DD.N`（纯记账）。注意：它用 GITHUB_TOKEN 推 tag，**不会级联触发 release.yml**——发版需手动 `gh workflow run release.yml --ref <tag>`。
- 发布草稿（draft Release）是更新分发闸门：手动 publish 后 `releases/latest/download/latest.json` 才对外可见。
- 应用内更新语义：更新只升不降；debug 构建跳过自动检查且拒绝安装（设计如此）。
- 更新签名密钥：公钥在 tauri.conf.json，私钥在仓库 Secrets（`TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`）。私钥丢失 = 全部存量用户无法再收到更新。
