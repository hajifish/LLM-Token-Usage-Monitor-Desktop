//! Skills 清单:list_skills 命令与扫描/解析纯函数。
//!
//! 只读扫描本机 Claude Code / Codex 的 skill 目录(自建 + 插件),
//! 不写任何文件、不改 fs 权限。frontmatter 手写行级解析(不引入 yaml 依赖):
//! 只取 name / description 两个单行字段,失败不致命(回退目录名/空描述)。

use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

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
        let Some((key, raw)) = split_kv(line) else {
            continue;
        };
        if raw.trim().starts_with('>') || raw.trim().starts_with('|') {
            // 折叠标量标记(>、>-、|- 等):值在后续缩进行,收集后拼接
            multiline = Some((key, Vec::new()));
        } else {
            let value = strip_quotes(raw);
            if !value.is_empty() {
                match key {
                    "name" => fm.name = Some(value),
                    "description" => fm.description = Some(value),
                    _ => {}
                }
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

/// 扫描 skills 目录下每个非隐藏子目录的 SKILL.md,构造 SkillEntry。
/// - 无 frontmatter name → 回退目录名;description 缺失 → 空串
/// - 目录不存在 / 子目录无 SKILL.md → 跳过(静默)
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
        let file_name_os = entry.file_name();
        let Some(file_name) = file_name_os.to_str() else {
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

#[cfg(test)]
mod skills_tests {
    use super::*;

    #[test]
    fn parse_frontmatter_extracts_single_line_fields() {
        let fm = parse_frontmatter(
            "---\nname: commit-pr\ndescription: 提交代码并创建 PR。\n---\n\n正文",
        );
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

    // ---- 目录扫描 ----
    // 手写 tempdir(项目惯例,见 qoder.rs 测试;不引入 tempfile dev-dep)
    struct ScratchDir(std::path::PathBuf);
    impl ScratchDir {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir()
                .join(format!("llmtm-skills-test-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            ScratchDir(p)
        }
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 在 root/<dir>/ 下写一个 SKILL.md
    fn write_skill(root: &std::path::Path, dir: &str, md: &str) {
        let dir = root.join(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), md).unwrap();
    }

    #[test]
    fn scan_user_skills_collects_entries_with_fallbacks() {
        let s = ScratchDir::new("user");
        let skills_root = s.path().join("skills");
        write_skill(
            &skills_root,
            "commit",
            "---\nname: commit\ndescription: 提交。\n---\n正文",
        );
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

    // ---- 插件扫描 ----

    #[test]
    fn claude_manifest_takes_precedence_over_cache() {
        let s = ScratchDir::new("manifest");
        let plugins = s.path().join("plugins");
        // 已安装插件(清单指向 inst-a;同一 installPath 双 scope 记录应去重)
        let inst_a = plugins.join("cache/org-a/installed-plugin/1.0.0");
        write_skill(
            &inst_a.join("skills"),
            "alpha",
            "---\nname: alpha\ndescription: 已安装\n---\n",
        );
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
        write_skill(
            &cache_plugin.join("skills"),
            "beta",
            "---\nname: beta\n---\n",
        );
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
        write_skill(
            &version_dir.join("skills"),
            "pdf",
            "---\nname: pdf\ndescription: PDF 处理\n---\n",
        );
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
}
