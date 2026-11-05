import { useCallback, useEffect, useMemo, useState } from 'react';
import type { CSSProperties } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { SkillEntry, SkillI18n } from '../types';

type Agent = 'claude' | 'codex';

/** Claude 星芒标记(品牌橙由 tab active 态着色) */
function ClaudeMark() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.2"
      strokeLinecap="round"
    >
      <line x1="12" y1="4" x2="12" y2="20" />
      <line x1="4" y1="12" x2="20" y2="12" />
      <line x1="6.2" y1="6.2" x2="8.4" y2="8.4" />
      <line x1="17.8" y1="6.2" x2="15.6" y2="8.4" />
      <line x1="6.2" y1="17.8" x2="8.4" y2="15.6" />
      <line x1="17.8" y1="17.8" x2="15.6" y2="15.6" />
    </svg>
  );
}

/** OpenAI 六瓣花结标记(Codex) */
function CodexMark() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinejoin="round"
    >
      {[0, 60, 120, 180, 240, 300].map((deg) => (
        <path
          key={deg}
          d="M12 2.6 L13.9 8.4 L12 10 L10.1 8.4 Z"
          transform={`rotate(${deg} 12 12)`}
        />
      ))}
    </svg>
  );
}

const AGENTS: { key: Agent; label: string; Icon: () => JSX.Element }[] = [
  { key: 'claude', label: 'Claude Code', Icon: ClaudeMark },
  { key: 'codex', label: 'Codex', Icon: CodexMark },
];

interface SkillsProps {
  onBack: () => void;
}

/** 悬浮详情弹窗尺寸(fixed 定位,用卡片矩形计算落点) */
const POPOVER_W = 300;
const POPOVER_MAX_H = 280;

/** 弹窗落在卡片正下方;下方空间不足时翻到上方;左右收进窗口内 */
function popoverStyle(rect: DOMRect): CSSProperties {
  const x = Math.min(
    Math.max(rect.left + rect.width / 2 - POPOVER_W / 2, 8),
    window.innerWidth - POPOVER_W - 8,
  );
  const below = rect.bottom + 8;
  const y =
    below + POPOVER_MAX_H <= window.innerHeight ? below : Math.max(rect.top - POPOVER_MAX_H - 8, 8);
  return { left: x, top: y, width: POPOVER_W, maxHeight: POPOVER_MAX_H };
}

/**
 * Skill 清单页:只读浏览本机 Claude Code / Codex 的 skill(自建 + 插件下载)。
 * 入口为主页 header 图标(App.tsx)。切 tab / 手动刷新时重新扫描,
 * 本地 IO 毫秒级,刻意不缓存——数据永远新鲜。
 * 卡片只放关键信息,完整情况悬停弹出详情(不点击)。
 */
function Skills({ onBack }: SkillsProps) {
  const [agent, setAgent] = useState<Agent>('claude');
  const [skills, setSkills] = useState<SkillEntry[] | null>(null);
  const [error, setError] = useState('');
  const [query, setQuery] = useState('');
  /** 悬停中的卡片(skill + 卡片矩形),驱动 fixed 定位详情弹窗 */
  const [hovered, setHovered] = useState<{ skill: SkillEntry; rect: DOMRect } | null>(null);
  /** 已展开的插件组(插件名);不在表内 = 收起。默认全收起,搜索时强制展开 */
  const [expandedPlugins, setExpandedPlugins] = useState<Record<string, boolean>>({});
  /** 本地中文翻译覆盖({应用数据目录}/skills-i18n/,用户可选;空表=英文原文) */
  const [i18n, setI18n] = useState<SkillI18n>({ plugins: {}, skills: {} });

  /** skill 描述的中文展示(键 plugin:skill;无词条回退原文) */
  const zhSkillDesc = (s: SkillEntry) =>
    (s.plugin_name && i18n.skills[`${s.plugin_name}:${s.name}`]) || s.description;

  // 全部 setState 都在异步回调里(effect 内同步 setState 会触发
  // react-hooks/set-state-in-effect);切换时的「扫描中」由 switchAgent 置空实现
  const load = useCallback((a: Agent) => {
    Promise.all([
      invoke<SkillEntry[]>('list_skills', { agent: a }),
      // 覆盖文件缺失属正常路径,不作为错误打扰页面
      invoke<SkillI18n>('get_skill_i18n').catch((): SkillI18n => ({ plugins: {}, skills: {} })),
    ])
      .then(([list, overrides]) => {
        setSkills(list);
        setI18n(overrides);
        setError('');
      })
      .catch((e) => {
        // 后端错误(参数非法/取不到主目录)对用户可观测
        setSkills([]);
        setError(String(e));
      });
  }, []);

  // 进入页面 / 切换 tab:重新扫描(本地毫秒级,刻意不缓存)
  useEffect(() => {
    load(agent);
  }, [agent, load]);

  // 同步重置在事件处理器里做:立即显示「扫描中…」,再触发 effect 加载
  const switchAgent = (a: Agent) => {
    if (a === agent) return;
    setHovered(null);
    setSkills(null);
    setExpandedPlugins({});
    setAgent(a);
  };

  const filtered = useMemo(() => {
    if (!skills) return [];
    const q = query.trim().toLowerCase();
    if (!q) return skills;
    // 中文覆盖与英文原文都可命中(用户看到的与记到的都可能任一语言)
    return skills.filter((s) =>
      [s.name, s.description, zhSkillDesc(s), s.plugin_name ?? ''].some((v) =>
        v.toLowerCase().includes(q),
      ),
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps -- zhSkillDesc 仅闭包 i18n,行为已由 deps 中的 i18n 覆盖
  }, [skills, query, i18n]);

  const userSkills = filtered.filter((s) => s.source === 'user');
  const pluginSkills = filtered.filter((s) => s.source === 'plugin');

  /** 插件按归属分组(org/plugin 全键防跨市场撞名;技能多的在前),每组可折叠——
   平铺 90+ 张卡片看不出哪些属于 pstack、哪些属于 superpowers */
  const pluginGroups = useMemo(() => {
    const map = new Map<string, SkillEntry[]>();
    for (const s of pluginSkills) {
      const org = s.plugin_org ?? '';
      const name = s.plugin_name ?? '其他';
      const key = `${org}/${name}`;
      const arr = map.get(key);
      if (arr) arr.push(s);
      else map.set(key, [s]);
    }
    return [...map.entries()]
      .map(([key, group]) => ({
        key,
        name: group[0].plugin_name ?? '其他',
        org: group[0].plugin_org ?? '',
        desc: i18n.plugins[key] || group[0].plugin_description || '',
        group,
      }))
      .sort((a, b) => b.group.length - a.group.length || a.name.localeCompare(b.name));
  }, [pluginSkills, i18n]);

  /** 搜索中强制展开所有组:命中项藏在收起组里会让搜索看起来失灵 */
  const searching = query.trim() !== '';

  const togglePlugin = (name: string) => setExpandedPlugins((m) => ({ ...m, [name]: !m[name] }));

  /** 卡片只放关键信息:名称 + 描述(截断);悬停看完整详情 */
  const renderCard = (s: SkillEntry) => (
    <div
      key={`${s.source}:${s.path}`}
      className="skill-card"
      onMouseEnter={(e) => setHovered({ skill: s, rect: e.currentTarget.getBoundingClientRect() })}
      onMouseLeave={() => setHovered(null)}
    >
      <span className="skill-card-name">{s.name}</span>
      <p className="skill-card-desc">{zhSkillDesc(s)}</p>
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
          {AGENTS.map(({ key, label, Icon }) => (
            <button
              key={key}
              className={`skills-tab ${key}${agent === key ? ' active' : ''}`}
              onClick={() => switchAgent(key)}
              title={label}
              aria-label={label}
            >
              <Icon />
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
                <h3>用户目录 ({userSkills.length})</h3>
                <div className="skills-grid">{userSkills.map(renderCard)}</div>
              </div>
            )}
            {pluginSkills.length > 0 && (
              <div className="settings-section">
                <h3>插件下载({pluginGroups.length} 个插件)</h3>
                {pluginGroups.map(({ key, name, desc, group }) => {
                  const open = searching || !!expandedPlugins[key];
                  return (
                    <div key={key} className="plugin-group">
                      <button
                        className="plugin-group-header"
                        onClick={() => togglePlugin(key)}
                        title={open ? '收起' : '展开'}
                      >
                        <span className="plugin-group-caret">{open ? '▾' : '▸'}</span>
                        <span className="plugin-group-name">{name}</span>
                        {desc && (
                          <span className="plugin-group-desc" title={desc}>
                            {desc}
                          </span>
                        )}
                        <span className="plugin-group-count">{group.length}</span>
                      </button>
                      {open && <div className="skills-grid">{group.map(renderCard)}</div>}
                    </div>
                  );
                })}
              </div>
            )}
            {filtered.length === 0 && (
              <div className="skills-empty">没有匹配「{query}」的 skill</div>
            )}
          </>
        )}
      </div>

      {/* 悬停详情弹窗:pointer-events:none 纯提示不拦截鼠标,避免滑入时闪烁 */}
      {hovered && (
        <div className="skill-popover" style={popoverStyle(hovered.rect)} role="tooltip">
          <div className="skill-popover-name">{hovered.skill.name}</div>
          {zhSkillDesc(hovered.skill) && (
            <p className="skill-popover-desc">{zhSkillDesc(hovered.skill)}</p>
          )}
          {hovered.skill.plugin_name && (
            <div className="skill-popover-meta">
              <span className="skill-popover-label">插件</span>
              <span>
                {hovered.skill.plugin_name}
                {hovered.skill.plugin_org ? `（${hovered.skill.plugin_org}）` : ''}
              </span>
            </div>
          )}
          <div className="skill-popover-meta">
            <span className="skill-popover-label">路径</span>
            <span className="skill-popover-path">{hovered.skill.path}</span>
          </div>
        </div>
      )}
    </div>
  );
}

export default Skills;
