# Skills 清单页实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增只读 Skills 清单子页,按 agent(Claude Code / Codex)分 tab 展示本机自建与插件下载的 skill。

**Architecture:** Rust 新模块 `skills.rs`(frontmatter 行级解析 + 目录扫描,全为纯函数,同步命令 `list_skills`)→ 前端新组件 `Skills.tsx`(双 tab、搜索、分组、行内详情)→ `App.tsx` header 图标接入。零新运行时依赖,不加 fs 权限。

**Tech Stack:** Tauri 2(Rust)+ React + TypeScript,单测内联 `#[cfg(test)] mod`(项目惯例)。

**Spec:** `docs/superpowers/specs/2026-11-05-skills-page-design.md`

## Global Constraints

- `SkillEntry` 字段 snake_case,Rust(`skills.rs`)与 TS(`types.ts`)严格一致:`name` / `description` / `source` / `plugin_name` / `plugin_org` / `path`
- 只读:不写任何文件、不加/改 fs capabilities、零新运行时依赖。**偏差说明**:spec 原写 tempfile 作 dev-dependency,实现遵循项目既有惯例(qoder.rs 测试的手写 ScratchDir 模式),同样零新依赖
- UI 文案、代码注释一律中文(项目惯例)
- 每个 Rust 任务提交前跑 `cargo fmt`(CI 有 rustfmt 守卫);命令为 `cd src-tauri && cargo fmt && cargo test`
- 扫描跳过一切 `.` 开头目录;只认 `SKILL.md` 文件名;目录不存在 = 空列表(非错误)
- 单个 SKILL.md 读取失败:文件存在但读失败 → `log::warn` 跳过;不存在 → 静默跳过
- 提交信息以 `feat:`/`docs:` 开头,末尾附 `Co-Authored-By: Claude Code <noreply@anthropic.com>`
- 工作分支:`feature/skills-page`(已存在,spec 已提交)

---

### Task 1: skills.rs — Frontmatter 行级解析纯函数

**Files:**
- Create: `src-tauri/src/skills.rs`(本任务只写文件头注释、`SkillEntry`、`Frontmatter`、`strip_quotes`、`split_kv`、`parse_frontmatter` 与测试;扫描函数后续任务再加)

**Interfaces:**
- Consumes: 无
- Produces(Task 2/3/4 依赖):
  - `pub struct SkillEntry { pub name: String, pub description: String, pub source: String, pub plugin_name: Option<String>, pub plugin_org: Option<String>, pub path: String }`(derive `Serialize, Debug, Clone, PartialEq`)
  - `struct Frontmatter { name: Option<String>, description: Option<String> }`(derive `Debug, Default, PartialEq`)
  - `fn parse_frontmatter(content: &str) -> Frontmatter`
  - `fn strip_quotes(raw: &str) -> String`、`fn split_kv(line: &str) -> Option<(&str, &str)>`

- [ ] **Step 1: 写失败测试**

创建 `src-tauri/src/skills.rs`,内容如下(此时 `parse_frontmatter` 等尚未实现,只放测试与文件头;实现体留空会编译失败,即预期的红灯):

```rust
//! Skills 清单:list_skills 命令与扫描/解析纯函数。
//!
//! 只读扫描本机 Claude Code / Codex 的 skill 目录(自建 + 插件),
//! 不写任何文件、不改 fs 权限。frontmatter 手写行级解析(不引入 yaml 依赖):
//! 只取 name / description 两个单行字段,失败不致命(回退目录名/空描述)。

use serde::Serialize;

/// 与 `src/types.ts` 的 `SkillEntry` 严格一致(snake_case)。
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct SkillEntry {
    /// frontmatter name,缺失回退目录名
    pub name: String,
    /// frontmatter description,解析失败为空串
    pub description: String,
    /// "user" | "plugin"
    pub source: String,
    /// 插件名("superpowers" 等);自建为 None
    pub plugin_name: Option<String>,
    /// 插件来源组织("claude-plugins-official" / "openai-curated" 等);自建为 None
    pub plugin_org: Option<String>,
    /// SKILL.md 绝对路径(详情展示)
    pub path: String,
}

#[cfg(test)]
mod skills_tests {
    use super::*;

    #[test]
    fn parse_frontmatter_extracts_single_line_fields() {
        let fm = parse_frontmatter("---\nname: commit-pr\ndescription: 提交代码并创建 PR。\n---\n\n正文");
        assert_eq!(fm.name.as_deref(), Some("commit-pr"));
        assert_eq!(fm.description.as_deref(), Some("提交代码并创建 PR。"));
    }

    #[test]
    fn parse_frontmatter_strips_quotes() {
        let fm = parse_frontmatter("---\nname: x\ndescription: \"You MUST use this.\"\n---\n");
        assert_eq!(fm.description.as_deref(), Some("You MUST use this."));
    }

    #[test]
    fn parse_frontmatter_without_name_returns_none() {
        let fm = parse_frontmatter("---\ndescription: 只有描述\n---\n");
        assert_eq!(fm.name, None);
    }

    #[test]
    fn parse_frontmatter_without_frontmatter_returns_default() {
        let fm = parse_frontmatter("# 直接正文,没有 frontmatter\n");
        assert_eq!(fm, Frontmatter::default());
    }

    #[test]
    fn parse_frontmatter_joins_folded_multiline_description() {
        let fm = parse_frontmatter("---\nname: x\ndescription: >-\n  第一行\n  第二行\n---\n");
        assert_eq!(fm.description.as_deref(), Some("第一行 第二行"));
    }

    #[test]
    fn parse_frontmatter_ignores_other_keys() {
        let fm = parse_frontmatter(
            "---\nname: x\nallowed-tools: Bash(git*), Bash(gh*)\ndisable-model-invocation: true\n---\n",
        );
        assert_eq!(fm.name.as_deref(), Some("x"));
        assert_eq!(fm.description, None);
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test skills`
Expected: 编译失败 `cannot find function \`parse_frontmatter\``(及 `Frontmatter` 等)。

- [ ] **Step 3: 实现**

在 `use serde::Serialize;` 之后、`#[cfg(test)]` 之前插入:

```rust
/// frontmatter 中本页关心的两个字段(其余字段一律忽略)
#[derive(Debug, Default, PartialEq)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
}

/// 去掉首尾空白与成对的单/双引号
fn strip_quotes(raw: &str) -> String {
    let v = raw.trim();
    if v.len() >= 2 {
        let bytes = v.as_bytes();
        let (first, last) = (bytes[0], bytes[v.len() - 1]);
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return v[1..v.len() - 1].trim().to_string();
        }
    }
    v.to_string()
}

/// 拆顶层 `key: value`(key 必须顶格);缩进行(子键/折行)返回 None
fn split_kv(line: &str) -> Option<(&str, &str)> {
    let (k, v) = line.split_once(':')?;
    if k.starts_with(' ') || k.starts_with('\t') {
        return None;
    }
    Some((k.trim(), v))
}

/// 解析 SKILL.md 开头的 YAML frontmatter(手写行级,仅支持单行标量值)。
///
/// 多行折叠标量(`>` / `|-` 等标记后跟缩进行)容忍降级:收集后续缩进的非空行
/// 以空格拼接。无 frontmatter 或字段缺失 → None,由调用方回退。
fn parse_frontmatter(content: &str) -> Frontmatter {
    let mut lines = content.lines();
    // 首行必须是 --- 才算 frontmatter
    if lines.next().map(str::trim) != Some("---") {
        return Frontmatter::default();
    }
    let mut fm = Frontmatter::default();
    // 多行折叠标量收集状态:目标键名与已收集内容
    let mut multiline: Option<(&str, Vec<String>)> = None;
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        // 上一字段是折叠标量:非空缩进行是内容行
        if let Some((_, buf)) = multiline.as_mut() {
            if line.starts_with(' ') || line.starts_with('\t') {
                let t = line.trim();
                if !t.is_empty() {
                    buf.push(t.to_string());
                    continue;
                }
            }
            multiline = None; // 非缩进行:折叠标量结束,回到键值解析
        }
        let Some((key, raw)) = split_kv(line) else { continue };
        let value = strip_quotes(raw);
        if value.is_empty() && (raw.trim().starts_with('>') || raw.trim().starts_with('|')) {
            // 折叠标量标记(>-、|- 等,标记后无同行内容):收集后续缩进行
            multiline = Some((key, Vec::new()));
        } else if !value.is_empty() {
            match key {
                "name" => fm.name = Some(value),
                "description" => fm.description = Some(value),
                _ => {}
            }
        }
    }
    // 文件在折叠标量中间结束:用已收集部分
    if let Some((key, buf)) = multiline {
        let joined = buf.join(" ");
        match key {
            "name" => fm.name = Some(joined),
            "description" => fm.description = Some(joined),
            _ => {}
        }
    }
    fm
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test skills`
Expected: 6 个测试全部 PASS。

- [ ] **Step 5: 格式化并提交**

```bash
cd src-tauri && cargo fmt && cd ..
git add src-tauri/src/skills.rs
git commit -m "feat: skills.rs frontmatter 行级解析纯函数(含单测)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: skills.rs — 用户级目录扫描

**Files:**
- Modify: `src-tauri/src/skills.rs`(追加扫描函数与测试;**不要**在本任务把 `mod skills;` 加进 lib.rs——命令还不存在,Task 4 一起接,避免 dead_code 告警进主干)

**Interfaces:**
- Consumes: Task 1 的 `SkillEntry` / `parse_frontmatter`
- Produces(Task 3/4 依赖):
  - `fn scan_skill_dirs(root: &Path, plugin_name: Option<&str>, plugin_org: Option<&str>, source: &str) -> Vec<SkillEntry>`(root 即 skills 目录;扫每个非隐藏子目录下的 `SKILL.md`)
  - `fn scan_user_skills(config_dir: &Path) -> Vec<SkillEntry>`(config_dir 如 `~/.claude`,扫其下 `skills/`)

- [ ] **Step 1: 写失败测试**

在 `mod skills_tests` 内追加(放在 frontmatter 测试之后):

```rust
    // ---- 目录扫描 ----
    // 手写 tempdir(项目惯例,见 qoder.rs 测试;不引入 tempfile dev-dep)
    struct ScratchDir(PathBuf);
    impl ScratchDir {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir()
                .join(format!("llmtm-skills-test-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            ScratchDir(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 在 root/<dir>/ 下写一个 SKILL.md
    fn write_skill(root: &Path, dir: &str, md: &str) {
        let dir = root.join(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), md).unwrap();
    }

    #[test]
    fn scan_user_skills_collects_entries_with_fallbacks() {
        let s = ScratchDir::new("user");
        let skills_root = s.path().join("skills");
        write_skill(&skills_root, "commit", "---\nname: commit\ndescription: 提交。\n---\n正文");
        write_skill(&skills_root, "no-frontmatter", "正文");
        // 隐藏目录(Codex .system 同理):跳过
        write_skill(&skills_root, ".system", "---\nname: hidden\n---\n");
        // 无 SKILL.md 的目录:跳过
        std::fs::create_dir_all(skills_root.join("empty-dir")).unwrap();

        let entries = scan_user_skills(s.path());
        assert_eq!(entries.len(), 2);
        let commit = entries.iter().find(|e| e.name == "commit").unwrap();
        assert_eq!(commit.source, "user");
        assert_eq!(commit.description, "提交。");
        assert!(commit.path.ends_with("commit/SKILL.md"));
        assert_eq!(commit.plugin_name, None);
        let nofm = entries.iter().find(|e| e.name == "no-frontmatter").unwrap();
        assert_eq!(nofm.description, "");
    }

    #[test]
    fn scan_user_skills_missing_root_returns_empty() {
        let s = ScratchDir::new("user-missing");
        assert!(scan_user_skills(&s.path().join("nonexistent")).is_empty());
    }
```

同时在文件顶部 `use serde::Serialize;` 后加 `use std::path::{Path, PathBuf};`。

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test skills`
Expected: 编译失败 `cannot find function \`scan_user_skills\``。

- [ ] **Step 3: 实现**

在 `parse_frontmatter` 之后插入:

```rust
/// 扫描 skills 目录下每个非隐藏子目录的 SKILL.md,构造 SkillEntry。
/// - 无 frontmatter name → 回退目录名;description 缺失 → 空串
/// - 目录不存在 / 子目录无 SKILL.md / 文件不存在 → 跳过(静默)
/// - SKILL.md 存在但读取失败 → log::warn 后跳过,不阻塞整页
fn scan_skill_dirs(
    root: &Path,
    plugin_name: Option<&str>,
    plugin_org: Option<&str>,
    source: &str,
) -> Vec<SkillEntry> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return out; // 目录不存在:正常空列表
    };
    for entry in entries.flatten() {
        let Some(file_name) = entry.file_name().to_str() else {
            continue;
        };
        if file_name.starts_with('.') {
            continue; // 隐藏目录(含 Codex .system 内置 skill)
        }
        let skill_md = entry.path().join("SKILL.md");
        let content = match std::fs::read_to_string(&skill_md) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                log::warn!("Skills: 读取失败,跳过 {}: {e}", skill_md.display());
                continue;
            }
        };
        let fm = parse_frontmatter(&content);
        out.push(SkillEntry {
            name: fm.name.unwrap_or_else(|| file_name.to_string()),
            description: fm.description.unwrap_or_default(),
            source: source.to_string(),
            plugin_name: plugin_name.map(str::to_string),
            plugin_org: plugin_org.map(str::to_string),
            path: skill_md.to_string_lossy().into_owned(),
        });
    }
    out
}

/// 用户级 skill:<config_dir>/skills/<名>/SKILL.md(config_dir 如 ~/.claude、~/.codex)
fn scan_user_skills(config_dir: &Path) -> Vec<SkillEntry> {
    scan_skill_dirs(&config_dir.join("skills"), None, None, "user")
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test skills`
Expected: 8 个测试全部 PASS。

- [ ] **Step 5: 格式化并提交**

```bash
cd src-tauri && cargo fmt && cd ..
git add src-tauri/src/skills.rs
git commit -m "feat: 用户级 skill 目录扫描(隐藏目录跳过、名字回退,含单测)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: skills.rs — 插件扫描(claude 清单优先 / 裸扫回退 / codex 裸扫)

**Files:**
- Modify: `src-tauri/src/skills.rs`

**Interfaces:**
- Consumes: Task 2 的 `scan_skill_dirs`
- Produces(Task 4 依赖):
  - `fn scan_plugin_skills(config_dir: &Path, agent: &str) -> Vec<SkillEntry>`(agent: "claude" | "codex";其余值不进入本函数,由命令层先行校验)
  - `fn claude_installed_plugin_dirs(plugins_root: &Path) -> Option<Vec<(String, String, PathBuf)>>`(org, plugin_name, install_path;文件缺失/解析失败 → None)
  - `fn scan_plugin_cache(config_dir: &Path) -> Vec<SkillEntry>`

- [ ] **Step 1: 写失败测试**

在 `mod skills_tests` 内追加:

```rust
    // ---- 插件扫描 ----

    #[test]
    fn claude_manifest_takes_precedence_over_cache() {
        let s = ScratchDir::new("manifest");
        let plugins = s.path().join("plugins");
        // 已安装插件(清单指向 inst-a;同一 installPath 双 scope 记录应去重)
        let inst_a = plugins.join("cache/org-a/installed-plugin/1.0.0");
        write_skill(&inst_a.join("skills"), "alpha", "---\nname: alpha\ndescription: 已安装\n---\n");
        // cache 残留(清单未指向):不应出现
        let stale = plugins.join("cache/org-a/stale-plugin/0.1.0");
        write_skill(&stale.join("skills"), "ghost", "---\nname: ghost\n---\n");
        let manifest = r#"{"version":2,"plugins":{"installed-plugin@org-a":[
            {"scope":"user","installPath":"INST_A"},
            {"scope":"project","installPath":"INST_A"}]}}"#
            .replace("INST_A", inst_a.to_str().unwrap());
        std::fs::create_dir_all(&plugins).unwrap();
        std::fs::write(plugins.join("installed_plugins.json"), manifest).unwrap();

        let entries = scan_plugin_skills(s.path(), "claude");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "alpha");
        assert_eq!(entries[0].source, "plugin");
        assert_eq!(entries[0].plugin_name.as_deref(), Some("installed-plugin"));
        assert_eq!(entries[0].plugin_org.as_deref(), Some("org-a"));
    }

    #[test]
    fn claude_manifest_corrupt_falls_back_to_cache() {
        let s = ScratchDir::new("corrupt");
        let plugins = s.path().join("plugins");
        let cache_plugin = plugins.join("cache/org-b/p/2.0.0");
        write_skill(&cache_plugin.join("skills"), "beta", "---\nname: beta\n---\n");
        std::fs::create_dir_all(&plugins).unwrap();
        std::fs::write(plugins.join("installed_plugins.json"), "{不是 json").unwrap();

        let entries = scan_plugin_skills(s.path(), "claude");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "beta");
        assert_eq!(entries[0].plugin_org.as_deref(), Some("org-b"));
    }

    #[test]
    fn codex_scans_cache_directly() {
        let s = ScratchDir::new("codex");
        let version_dir = s.path().join("plugins/cache/openai-curated/pdf/26.1.0");
        write_skill(&version_dir.join("skills"), "pdf", "---\nname: pdf\ndescription: PDF 处理\n---\n");
        // 隐藏组织目录(如 .plugin-appserver):跳过
        write_skill(
            &s.path().join("plugins/cache/.hidden/p/1.0/skills"),
            "secret",
            "---\nname: secret\n---\n",
        );

        let entries = scan_plugin_skills(s.path(), "codex");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "pdf");
        assert_eq!(entries[0].plugin_name.as_deref(), Some("pdf"));
        assert_eq!(entries[0].plugin_org.as_deref(), Some("openai-curated"));
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test skills`
Expected: 编译失败 `cannot find function \`scan_plugin_skills\``。

- [ ] **Step 3: 实现**

顶部 use 区改为(新增 BTreeSet):

```rust
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
```

在 `scan_user_skills` 之后插入:

```rust
/// 解析 ~/.claude/plugins/installed_plugins.json,返回去重后的
/// (org, plugin_name, install_path)。键形如 "frontend-design@claude-plugins-official";
/// 同一 installPath 可能因 user/project 双 scope 重复,按 installPath 去重。
/// 文件缺失或解析失败返回 None(调用方回退裸扫 cache)。
fn claude_installed_plugin_dirs(plugins_root: &Path) -> Option<Vec<(String, String, PathBuf)>> {
    let manifest = std::fs::read_to_string(plugins_root.join("installed_plugins.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&manifest).ok()?;
    let plugins = value.get("plugins")?.as_object()?;
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (key, records) in plugins {
        let (name, org) = match key.split_once('@') {
            Some((n, o)) => (n.to_string(), o.to_string()),
            None => (key.clone(), String::new()),
        };
        for record in records.as_array().into_iter().flatten() {
            let Some(install_path) = record.get("installPath").and_then(|v| v.as_str()) else {
                continue;
            };
            if seen.insert(install_path.to_string()) {
                out.push((org.clone(), name.clone(), PathBuf::from(install_path)));
            }
        }
    }
    Some(out)
}

/// 裸扫 <config_dir>/plugins/cache/<org>/<plugin>/<version>/skills/*/SKILL.md。
/// Codex 的主路径;Claude 在清单缺失/损坏时的回退。隐藏目录各层均跳过。
fn scan_plugin_cache(config_dir: &Path) -> Vec<SkillEntry> {
    let cache = config_dir.join("plugins").join("cache");
    let mut out = Vec::new();
    let Ok(orgs) = std::fs::read_dir(&cache) else {
        return out;
    };
    for org in orgs.flatten() {
        let org_name = org.file_name().to_string_lossy().into_owned();
        if org_name.starts_with('.') {
            continue;
        }
        let Ok(plugins) = std::fs::read_dir(org.path()) else {
            continue;
        };
        for plugin in plugins.flatten() {
            let plugin_name = plugin.file_name().to_string_lossy().into_owned();
            if plugin_name.starts_with('.') {
                continue;
            }
            let Ok(versions) = std::fs::read_dir(plugin.path()) else {
                continue;
            };
            // 同插件多版本目录不去重,如实列出(实际极少发生)
            for version in versions.flatten() {
                if version.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                out.extend(scan_skill_dirs(
                    &version.path().join("skills"),
                    Some(&plugin_name),
                    Some(&org_name),
                    "plugin",
                ));
            }
        }
    }
    out
}

/// 插件 skill 扫描:
/// - claude:以 installed_plugins.json 的 installPath 为准(区分已安装与卸载残留),
///   清单缺失/解析失败回退裸扫 cache(log::warn);
/// - codex:无安装清单,直接裸扫 cache。
fn scan_plugin_skills(config_dir: &Path, agent: &str) -> Vec<SkillEntry> {
    let plugins_root = config_dir.join("plugins");
    if agent == "claude" {
        if let Some(dirs) = claude_installed_plugin_dirs(&plugins_root) {
            return dirs
                .into_iter()
                .flat_map(|(org, name, path)| {
                    scan_skill_dirs(&path.join("skills"), Some(&name), Some(&org), "plugin")
                })
                .collect();
        }
        log::warn!("Skills: installed_plugins.json 缺失或解析失败,回退裸扫 plugins/cache");
    }
    scan_plugin_cache(config_dir)
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test skills`
Expected: 11 个测试全部 PASS。

- [ ] **Step 5: 格式化并提交**

```bash
cd src-tauri && cargo fmt && cd ..
git add src-tauri/src/skills.rs
git commit -m "feat: 插件 skill 扫描——claude 清单优先/裸扫回退、codex 裸扫(含单测)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: list_skills 命令 + lib.rs 注册

**Files:**
- Modify: `src-tauri/src/skills.rs`(追加命令与 `agent_dir`)
- Modify: `src-tauri/src/lib.rs:9-12`(mod 声明)、`lib.rs:19-22`(use)、`lib.rs:89-102`(generate_handler)

**Interfaces:**
- Consumes: Task 2/3 的 `scan_user_skills` / `scan_plugin_skills`
- Produces(前端 Task 5 依赖):Tauri 命令 `list_skills(agent: String) -> Result<Vec<SkillEntry>, String>`,JS 侧 `invoke('list_skills', { agent: 'claude' | 'codex' })`

- [ ] **Step 1: 写失败测试**

在 `mod skills_tests` 内追加:

```rust
    // ---- 命令层 ----

    #[test]
    fn list_skills_rejects_unknown_agent() {
        assert!(list_skills("cursor".to_string()).is_err());
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test skills`
Expected: 编译失败 `cannot find function \`list_skills\``。

- [ ] **Step 3: 实现**

在 `skills.rs` 的 `scan_plugin_skills` 之后(`mod skills_tests` 之前)插入:

```rust
/// agent 合法值与各自的家目录配置目录名
const AGENT_DIRS: &[(&str, &str)] = &[("claude", ".claude"), ("codex", ".codex")];

fn agent_dir(agent: &str) -> Option<&'static str> {
    AGENT_DIRS
        .iter()
        .find(|(a, _)| *a == agent)
        .map(|(_, d)| *d)
}

/// 只读扫描本机 Claude Code / Codex 的 skill 清单(自建 + 插件),供 Skills 页展示。
/// 同步命令:本地 IO 毫秒级。返回列表按「自建在前、插件在后」,组内按名字
/// 不分大小写排序(同名按路径),保证 UI 顺序稳定。
#[tauri::command]
pub fn list_skills(agent: String) -> Result<Vec<SkillEntry>, String> {
    let config_dir_name = agent_dir(&agent)
        .ok_or_else(|| format!("未知的 agent 参数:{agent}(仅支持 claude / codex)"))?;
    let home = dirs::home_dir().ok_or_else(|| "无法确定用户主目录".to_string())?;
    let config_dir = home.join(config_dir_name);
    let sort_by_name = |v: &mut Vec<SkillEntry>| {
        v.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.path.cmp(&b.path))
        });
    };
    let mut user = scan_user_skills(&config_dir);
    let mut plugin = scan_plugin_skills(&config_dir, &agent);
    sort_by_name(&mut user);
    sort_by_name(&mut plugin);
    user.extend(plugin);
    Ok(user)
}
```

修改 `src-tauri/src/lib.rs`(三处):

1. mod 声明区(`mod secrets;` 之后、`mod tray;` 之前)加:

```rust
mod skills;
```

2. use 区(`use scheduler::Scheduler;` 之后)加:

```rust
use skills::list_skills;
```

3. `tauri::generate_handler![...]` 列表中(`apply_window_effects,` 之后)加一行:

```rust
skills::list_skills,
```

注意:第 2 步的 `use skills::list_skills;` 与第 3 步的路径写法二选一即可——采用上面写法时 handler 里直接写 `list_skills,`。最终该列表形如:

```rust
        .invoke_handler(tauri::generate_handler![
            get_usage,
            get_config,
            get_config_health,
            save_config,
            send_test_notification,
            trigger_refresh,
            apply_tray_click_behavior,
            apply_window_effects,
            list_skills,
            updater::get_update_status,
            updater::check_update,
            updater::download_update,
            updater::install_update
        ])
```

- [ ] **Step 4: 跑全部测试确认通过(不只 skills)**

Run: `cd src-tauri && cargo test`
Expected: 全部 PASS(含既有测试与新增 12 个)。

- [ ] **Step 5: 格式化并提交**

```bash
cd src-tauri && cargo fmt && cd ..
git add src-tauri/src/skills.rs src-tauri/src/lib.rs
git commit -m "feat: list_skills 命令与 lib.rs 注册(agent 校验、稳定排序)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 5: 前端 — SkillEntry 类型 + Skills 组件 + 样式

**Files:**
- Modify: `src/types.ts`(文件末尾追加接口)
- Create: `src/components/Skills.tsx`
- Modify: `src/styles/app.css`(文件末尾追加样式段)

**Interfaces:**
- Consumes: Task 4 的 `invoke('list_skills', { agent })`,返回 `SkillEntry[]`
- Produces(Task 6 依赖):`export default function Skills({ onBack }: { onBack: () => void })`

- [ ] **Step 1: types.ts 追加类型**

在 `src/types.ts` 文件末尾追加:

```ts
/**
 * 单个 skill 条目 —— 与 Rust `skills::SkillEntry` 严格一致(snake_case)。
 */
export interface SkillEntry {
  /** frontmatter name,缺失时回退目录名。 */
  name: string;
  /** frontmatter description,解析失败为空串。 */
  description: string;
  /** "user"(自建)| "plugin"(插件下载)。 */
  source: 'user' | 'plugin';
  /** 插件名(superpowers 等);自建为 null。 */
  plugin_name: string | null;
  /** 插件来源组织(claude-plugins-official / openai-curated 等);自建为 null。 */
  plugin_org: string | null;
  /** SKILL.md 绝对路径(详情展示)。 */
  path: string;
}
```

- [ ] **Step 2: 创建 Skills.tsx**

创建 `src/components/Skills.tsx`:

```tsx
import { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { SkillEntry } from '../types';

type Agent = 'claude' | 'codex';

const AGENTS: { key: Agent; label: string }[] = [
  { key: 'claude', label: 'Claude Code' },
  { key: 'codex', label: 'Codex' },
];

interface SkillsProps {
  onBack: () => void;
}

/** 行的唯一键:来源+路径(同名列多条也能稳定区分) */
const rowKey = (s: SkillEntry) => `${s.source}:${s.path}`;

/** 完整命名:自建 = name;插件 = plugin:name(与 Claude Code 技能清单显示一致) */
const fullName = (s: SkillEntry) =>
  s.source === 'plugin' && s.plugin_name ? `${s.plugin_name}:${s.name}` : s.name;

/**
 * Skill 清单页:只读浏览本机 Claude Code / Codex 的 skill(自建 + 插件下载)。
 * 入口为主页 header 图标(App.tsx)。切 tab / 手动刷新时重新扫描,
 * 本地 IO 毫秒级,刻意不缓存——数据永远新鲜。
 */
function Skills({ onBack }: SkillsProps) {
  const [agent, setAgent] = useState<Agent>('claude');
  const [skills, setSkills] = useState<SkillEntry[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [query, setQuery] = useState('');
  /** 当前展开详情的行 key;null 表示全部收起 */
  const [expandedKey, setExpandedKey] = useState<string | null>(null);
  /** 刚复制完成的行 key(短暂显示「已复制」) */
  const [copiedKey, setCopiedKey] = useState<string | null>(null);

  const load = useCallback((a: Agent) => {
    setLoading(true);
    setError('');
    invoke<SkillEntry[]>('list_skills', { agent: a })
      .then(setSkills)
      .catch((e) => {
        // 后端错误(参数非法/取不到主目录)对用户可观测
        setSkills([]);
        setError(String(e));
      })
      .finally(() => setLoading(false));
  }, []);

  // 进入页面 / 切换 tab:重新扫描,并收起已展开的详情
  useEffect(() => {
    setSkills(null);
    setExpandedKey(null);
    load(agent);
  }, [agent, load]);

  const filtered = useMemo(() => {
    if (!skills) return [];
    const q = query.trim().toLowerCase();
    if (!q) return skills;
    return skills.filter((s) =>
      [s.name, s.description, s.plugin_name ?? ''].some((v) =>
        v.toLowerCase().includes(q),
      ),
    );
  }, [skills, query]);

  const userSkills = filtered.filter((s) => s.source === 'user');
  const pluginSkills = filtered.filter((s) => s.source === 'plugin');

  const copy = (s: SkillEntry) => {
    navigator.clipboard
      .writeText(fullName(s))
      .then(() => {
        setCopiedKey(rowKey(s));
        setTimeout(() => setCopiedKey(null), 1500);
      })
      .catch(console.error);
  };

  const renderRow = (s: SkillEntry) => {
    const key = rowKey(s);
    const expanded = expandedKey === key;
    return (
      <div key={key} className="skill-item">
        <button
          className="skill-row"
          onClick={() => setExpandedKey(expanded ? null : key)}
        >
          <span className="skill-name">{s.name}</span>
          {s.plugin_name && <span className="skill-badge">{s.plugin_name}</span>}
          <span className="skill-desc">{s.description}</span>
          <span className="skill-caret">{expanded ? '▴' : '▾'}</span>
        </button>
        {expanded && (
          <div className="skill-detail">
            {s.description && <p className="skill-detail-desc">{s.description}</p>}
            {s.plugin_org && (
              <div className="skill-detail-meta">
                <span className="skill-detail-label">来源</span>
                <span>
                  {s.plugin_org} / {s.plugin_name}
                </span>
              </div>
            )}
            <div className="skill-detail-meta">
              <span className="skill-detail-label">路径</span>
              <span className="skill-detail-path">{s.path}</span>
            </div>
            <div className="skill-detail-meta">
              <span className="skill-detail-label">调用名</span>
              <code className="skill-detail-name">{fullName(s)}</code>
              <button className="skill-copy-btn" onClick={() => copy(s)}>
                {copiedKey === key ? '已复制' : '复制'}
              </button>
            </div>
          </div>
        )}
      </div>
    );
  };

  return (
    <div className="settings">
      <div className="settings-header" data-tauri-drag-region>
        <button className="back-btn" onClick={onBack}>
          ← 返回
        </button>
        <span className="skills-title">Skills</span>
        <div className="skills-tabs">
          {AGENTS.map((a) => (
            <button
              key={a.key}
              className={`skills-tab${agent === a.key ? ' active' : ''}`}
              onClick={() => setAgent(a.key)}
            >
              {a.label}
            </button>
          ))}
        </div>
        <button
          className={`header-refresh-btn${loading ? ' is-refreshing' : ''}`}
          onClick={() => load(agent)}
          title="重新扫描"
          disabled={loading}
        >
          <svg
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <polyline points="23 4 23 10 17 10"></polyline>
            <polyline points="1 20 1 14 7 14"></polyline>
            <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"></path>
          </svg>
        </button>
      </div>

      <div className="settings-body">
        <input
          className="skills-search"
          type="text"
          placeholder="搜索名称 / 描述 / 插件…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />

        {error && <div className="skills-error">加载失败:{error}</div>}
        {!error && !skills && <div className="skills-empty">扫描中…</div>}
        {!error && skills && skills.length === 0 && (
          <div className="skills-empty">未发现任何 skill(目录不存在或为空)</div>
        )}
        {skills && skills.length > 0 && (
          <>
            {userSkills.length > 0 && (
              <div className="settings-section">
                <h3>自建 ({userSkills.length})</h3>
                {userSkills.map(renderRow)}
              </div>
            )}
            {pluginSkills.length > 0 && (
              <div className="settings-section">
                <h3>插件 ({pluginSkills.length})</h3>
                {pluginSkills.map(renderRow)}
              </div>
            )}
            {filtered.length === 0 && (
              <div className="skills-empty">没有匹配「{query}」的 skill</div>
            )}
          </>
        )}
      </div>
    </div>
  );
}

export default Skills;
```

- [ ] **Step 3: app.css 末尾追加样式**

在 `src/styles/app.css` 文件末尾追加:

```css
/* ===== Skills 清单页 ===== */
.skills-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--system-label);
}

/* agent 分段切换(skills 页 header 右侧) */
.skills-tabs {
  display: flex;
  gap: 2px;
  margin-left: auto;
  background: rgba(255, 255, 255, 0.06);
  border-radius: 8px;
  padding: 2px;
}
.skills-tab {
  border: none;
  background: none;
  color: var(--system-secondary-label);
  font-size: 12px;
  padding: 4px 12px;
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.15s;
}
.skills-tab:hover {
  color: var(--system-label);
}
.skills-tab.active {
  background: var(--system-blue);
  color: #fff;
}

.skills-search {
  margin-bottom: 12px;
}

/* skill 行(卡片) */
.skill-item {
  margin-bottom: 6px;
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.06);
  overflow: hidden;
}
.skill-row {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  border: none;
  background: none;
  color: var(--system-label);
  padding: 9px 12px;
  cursor: pointer;
  text-align: left;
  font-size: 13px;
}
.skill-row:hover {
  background: rgba(255, 255, 255, 0.05);
}
.skill-name {
  font-weight: 600;
  flex-shrink: 0;
  max-width: 40%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.skill-badge {
  flex-shrink: 0;
  font-size: 11px;
  color: var(--system-blue);
  background: rgba(10, 132, 255, 0.12);
  padding: 1px 8px;
  border-radius: 99px;
}
.skill-desc {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--system-tertiary-label);
  font-size: 12px;
}
.skill-caret {
  flex-shrink: 0;
  color: var(--system-tertiary-label);
  font-size: 10px;
}

/* 行内展开的详情 */
.skill-detail {
  padding: 4px 12px 10px;
  border-top: 1px solid var(--system-separator);
}
.skill-detail-desc {
  margin: 8px 0;
  font-size: 12.5px;
  line-height: 1.6;
  color: var(--system-label);
}
.skill-detail-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 6px;
  font-size: 12px;
}
.skill-detail-label {
  color: var(--system-tertiary-label);
  flex-shrink: 0;
}
.skill-detail-path {
  color: var(--system-secondary-label);
  word-break: break-all;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}
.skill-detail-name {
  background: rgba(255, 255, 255, 0.08);
  padding: 2px 8px;
  border-radius: 6px;
}
.skill-copy-btn {
  border: 1px solid var(--system-separator);
  background: none;
  color: var(--system-blue);
  border-radius: 6px;
  padding: 3px 10px;
  font-size: 12px;
  cursor: pointer;
}
.skill-copy-btn:hover {
  background: rgba(10, 132, 255, 0.12);
}

/* 空态 / 错误态 */
.skills-empty,
.skills-error {
  text-align: center;
  padding: 24px 0;
  color: var(--system-tertiary-label);
  font-size: 13px;
}
.skills-error {
  color: #ffb3b3;
}
```

- [ ] **Step 4: 验证类型与规范**

Run: `npm run build && npm run lint`
Expected: `tsc` + `vite build` 成功、eslint 0 error。
(本任务尚无入口,Skills.tsx 会被 vite tree-shake——属预期;Task 6 接线后生效。)

- [ ] **Step 5: 提交**

```bash
git add src/types.ts src/components/Skills.tsx src/styles/app.css
git commit -m "feat: Skills 清单页组件与样式(双 agent tab、搜索、行内详情、复制调用名)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 6: App.tsx 接线(header 入口)+ 全量验证

**Files:**
- Modify: `src/App.tsx:5-8`(import)、`App.tsx:14`(view 联合类型)、`App.tsx:82-98`(header 按钮)、`App.tsx:129-140`(渲染分支)

**Interfaces:**
- Consumes: Task 5 的 `Skills` 默认导出组件
- Produces: 无(终端任务)

- [ ] **Step 1: 修改 App.tsx**

四处修改:

1. import 区(`import Providers from './components/Providers';` 之后)加:

```tsx
import Skills from './components/Skills';
```

2. view 状态联合类型:

```tsx
const [view, setView] = useState<'usage' | 'settings' | 'providers' | 'skills'>('usage');
```

3. header-actions 内、供应商「+」按钮**之前**,插入 skill 入口图标(闪电):

```tsx
                <button
                  className="settings-icon-btn"
                  onClick={() => setView('skills')}
                  title="Skill 清单"
                >
                  <svg
                    width="18"
                    height="18"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"></polygon>
                  </svg>
                </button>
```

4. 渲染分支:现有 `) : (` Providers 分支改为 `) : view === 'providers' ? (`,并新增末尾 Skills 分支:

```tsx
        ) : view === 'settings' ? (
          <Settings onBack={() => setView('usage')} background={background} />
        ) : view === 'providers' ? (
          <Providers
            onBack={() => {
              setView('usage');
              // 乐观显示:新供应商行立即出现在面板上(行内「加载中」),
              // 真实数据由随后的刷新轮补齐——慢供应商也能马上确认添加成功
              refreshAfterProviderChange();
            }}
          />
        ) : (
          <Skills onBack={() => setView('usage')} />
        )}
```

- [ ] **Step 2: 全量验证**

Run: `npm run build && npm run lint && cd src-tauri && cargo fmt --check && cargo test && cd ..`
Expected: 前端构建+lint 通过;rustfmt 无差异;Rust 测试全绿(12 个新增 + 既有)。

- [ ] **Step 3: 提交**

```bash
git add src/App.tsx
git commit -m "feat: 主页 header 接入 Skills 页入口(闪电图标)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

- [ ] **Step 4: 开发冒烟(可选,人工验收前置)**

Run: `npm run tauri dev`
按 spec 验收清单核对:
1. header 闪电图标进入页面,默认 Claude Code tab,列出本机自建与插件 skill
2. 切到 Codex tab,列出 `~/.codex` 下 skill;`.system` 不出现
3. 搜索过滤、行内展开、复制调用名可用
4. 手动刷新按钮重新扫描
5. 空目录/异常场景显示空态或错误提示而非崩溃

---

## Self-Review 记录

- **Spec 覆盖**:双 agent tab(Task 5)、切换重扫不缓存(Task 5 useEffect)、claude 清单优先+回退(Task 3)、codex 裸扫(Task 3)、隐藏目录跳过(Task 2/3)、frontmatter 手写解析含引号/折叠容忍(Task 1)、agent 校验 Err(Task 4)、home 失败 Err(Task 4)、SKILL.md 存在读失败 warn 跳过(Task 2)、稳定排序(Task 4)、header 图标(Task 6)、空态/加载态/错误态(Task 5)、复制完整命名(Task 5)——全覆盖
- **偏差**:tempfile dev-dep → 手写 ScratchDir(项目 qoder.rs 惯例,同样零依赖),已在 Global Constraints 标注
- **类型一致性**:`SkillEntry` 六字段在 Task 1(Rust)/Task 5(TS)完全一致;`scan_skill_dirs`/`scan_user_skills`/`scan_plugin_skills` 签名在 Task 2/3/4 引用处核对一致;组件导出 `export default Skills` 与 Task 6 import 一致
