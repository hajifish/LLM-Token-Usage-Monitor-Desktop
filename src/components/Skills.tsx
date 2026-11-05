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

  // 同步重置在事件处理器里做：立即显示「扫描中…」，再触发 effect 加载
  const switchAgent = (a: Agent) => {
    if (a === agent) return;
    setSkills(null);
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

  /** 卡片只放关键信息：名称 + 描述（截断）+ 来源标记（自建 / 插件名） */
  const renderCard = (s: SkillEntry) => (
    <div key={`${s.source}:${s.path}`} className="skill-card">
      <span className="skill-card-name" title={s.name}>
        {s.name}
      </span>
      <p className="skill-card-desc" title={s.description}>
        {s.description}
      </p>
      <span className={`skill-card-badge ${s.source}`}>
        {s.source === 'user' ? '自建' : s.plugin_name}
      </span>
    </div>
  );

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
                <div className="skills-grid">{userSkills.map(renderCard)}</div>
              </div>
            )}
            {pluginSkills.length > 0 && (
              <div className="settings-section">
                <h3>插件 ({pluginSkills.length})</h3>
                <div className="skills-grid">{pluginSkills.map(renderCard)}</div>
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
