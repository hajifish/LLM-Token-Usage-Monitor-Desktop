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

/** 行的唯一键：来源+路径（同名列多条也能稳定区分） */
const rowKey = (s: SkillEntry) => `${s.source}:${s.path}`;

/** 完整命名：自建 = name；插件 = plugin:name（与 Claude Code 技能清单显示一致） */
const fullName = (s: SkillEntry) =>
  s.source === 'plugin' && s.plugin_name ? `${s.plugin_name}:${s.name}` : s.name;

/**
 * Skill 清单页：只读浏览本机 Claude Code / Codex 的 skill（自建 + 插件下载）。
 * 入口为主页 header 图标（App.tsx）。切 tab / 手动刷新时重新扫描，
 * 本地 IO 毫秒级，刻意不缓存——数据永远新鲜。
 */
function Skills({ onBack }: SkillsProps) {
  const [agent, setAgent] = useState<Agent>('claude');
  const [skills, setSkills] = useState<SkillEntry[] | null>(null);
  const [error, setError] = useState('');
  const [query, setQuery] = useState('');
  /** 当前展开详情的行 key；null 表示全部收起 */
  const [expandedKey, setExpandedKey] = useState<string | null>(null);
  /** 刚复制完成的行 key（短暂显示「已复制」） */
  const [copiedKey, setCopiedKey] = useState<string | null>(null);

  // 全部 setState 都在异步回调里（effect 内同步 setState 会触发
  // react-hooks/set-state-in-effect）；切换时的「扫描中」由 switchAgent 置空实现
  const load = useCallback((a: Agent) => {
    invoke<SkillEntry[]>('list_skills', { agent: a })
      .then((list) => {
        setSkills(list);
        setError('');
      })
      .catch((e) => {
        // 后端错误（参数非法/取不到主目录）对用户可观测
        setSkills([]);
        setError(String(e));
      });
  }, []);

  // 进入页面 / 切换 tab：重新扫描（本地毫秒级，刻意不缓存）
  useEffect(() => {
    load(agent);
  }, [agent, load]);

  // 同步重置在事件处理器里做：立即显示「扫描中…」并收起详情，再触发 effect 加载
  const switchAgent = (a: Agent) => {
    if (a === agent) return;
    setSkills(null);
    setExpandedKey(null);
    setAgent(a);
  };

  const filtered = useMemo(() => {
    if (!skills) return [];
    const q = query.trim().toLowerCase();
    if (!q) return skills;
    return skills.filter((s) =>
      [s.name, s.description, s.plugin_name ?? ''].some((v) => v.toLowerCase().includes(q)),
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
        <button className="skill-row" onClick={() => setExpandedKey(expanded ? null : key)}>
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
              onClick={() => switchAgent(a.key)}
            >
              {a.label}
            </button>
          ))}
        </div>
        <button className="header-refresh-btn" onClick={() => load(agent)} title="重新扫描">
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

        {error && <div className="skills-error">加载失败：{error}</div>}
        {!error && !skills && <div className="skills-empty">扫描中…</div>}
        {!error && skills && skills.length === 0 && (
          <div className="skills-empty">未发现任何 skill（目录不存在或为空）</div>
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
