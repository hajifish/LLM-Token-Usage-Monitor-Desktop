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
}
