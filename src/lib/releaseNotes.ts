/**
 * 发版说明解析。
 *
 * notes 是 GitHub Release 正文（release.yml 从 docs/release-notes/vX.Y.Z.md 原样填入），
 * 约定格式为「## 分节 + - **名称**：描述 + > 脚注」。发版文件缺失时 release.yml 回退
 * GitHub 自动生成说明（## What's Changed + PR 链接），因此解析需容忍该形态：链接退化为
 * 纯文本，无法识别任何分节时由调用方用 fallbackText 原样兜底。
 */

export interface ReleaseNoteItem {
  /** 条目名（行首加粗部分）；无加粗前缀的条目为 null */
  name: string | null;
  /** 条目正文 */
  detail: string;
}

export interface ReleaseNoteSection {
  /** 分节标题（## 行的文本） */
  title: string;
  items: ReleaseNoteItem[];
}

export interface ReleaseNotes {
  sections: ReleaseNoteSection[];
  /** 「> 引用」脚注（多条合并为一段）；无则 null */
  footnote: string | null;
  /** 未解析出任何分节时的原文（调用方原样展示） */
  fallbackText: string | null;
}

/** 行内 markdown 链接退化为链接文字 */
function stripLinks(text: string): string {
  return text.replace(/\[([^\]]+)\]\([^)]*\)/g, '$1');
}

/** `- **名称**：描述` → { name, detail }；无加粗前缀时整行为 detail */
function parseItem(line: string): ReleaseNoteItem {
  const text = stripLinks(line).trim();
  const m = text.match(/^\*\*(.+?)\*\*\s*[：:]\s*(.*)$/);
  if (m) return { name: m[1].trim(), detail: m[2].trim() };
  return { name: null, detail: text };
}

export function parseReleaseNotes(notes: string): ReleaseNotes {
  const sections: ReleaseNoteSection[] = [];
  const footnotes: string[] = [];

  for (const rawLine of notes.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line === '---') continue;
    const heading = line.match(/^#{1,3}\s+(.+)$/);
    if (heading) {
      sections.push({ title: stripLinks(heading[1]).trim(), items: [] });
      continue;
    }
    const quote = line.match(/^>\s?(.*)$/);
    if (quote) {
      footnotes.push(quote[1].trim());
      continue;
    }
    const bullet = line.match(/^[-*+]\s+(.+)$/);
    if (bullet && sections.length > 0) {
      sections[sections.length - 1].items.push(parseItem(bullet[1]));
    }
  }

  // 过滤后的分节才是有效解析结果；「有标题但零条目」等形态按未解析处理，走原文兜底
  const parsed = sections.filter((s) => s.items.length > 0);
  return {
    sections: parsed,
    footnote: footnotes.length > 0 ? stripLinks(footnotes.join(' ')).trim() : null,
    fallbackText: parsed.length === 0 ? notes.trim() || null : null,
  };
}
