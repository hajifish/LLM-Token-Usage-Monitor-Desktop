# Skills 清单页设计(spec)

日期:2026-11-05
分支:feature/skills-page(自 develop)

## 背景与目标

本机同时使用 Claude Code 与 Codex,两者的 skill 分散在各自的家目录下(自建与插件下载混在一起),用户没有一个快速俯瞰的入口。目标:在应用内新增一个**只读**子页面,按 agent(Claude Code / Codex)分 tab 展示 skill 清单,帮助用户快速判断"我有哪些 skill、是自建还是下载的、怎么调用"。

非目标(明确不做):

- 不做删除 / 编辑 / 新建 / 禁用 skill(破坏性操作,后续需求再议)
- 不做实时文件监听(进入页面 / 切 tab / 手动刷新时扫描)
- 不纳入项目级 skill(`.claude/skills` 随项目走,本应用无项目上下文)
- 不纳入 Claude Code / Codex 之外的其他 agent

## 数据源(本机实测确认)

### Claude Code(agent = "claude")

| 来源 | 路径 | 说明 |
| --- | --- | --- |
| 自建 | `~/.claude/skills/<名>/SKILL.md` | frontmatter 含 name / description |
| 插件下载 | `~/.claude/plugins/installed_plugins.json` 各 `installPath` 下 `skills/<名>/SKILL.md` | 该清单能区分"已安装"与"卸载残留缓存" |

回退:installed_plugins.json 缺失或解析失败 → 裸扫 `~/.claude/plugins/cache/<org>/<plugin>/<version>/skills/`,并 `log::warn`。

### Codex(agent = "codex")

| 来源 | 路径 | 说明 |
| --- | --- | --- |
| 自建 | `~/.codex/skills/<名>/SKILL.md` | `.system/` 为 Codex 内置系统 skill,排除 |
| 插件下载 | `~/.codex/plugins/cache/<org>/<plugin>/<version>/skills/<名>/SKILL.md` | Codex 无 installed 清单,直接裸扫 cache |

通用规则:

- 扫描时跳过所有 `.` 开头的目录(顺带排除 `.system` 等内置目录)
- 只认 `SKILL.md`(规范文件名)
- 目录不存在 = 该来源为空,属正常路径而非错误
- 同一插件多版本目录并存时不去重,如实列出(实际极少发生)

## 架构

```
Skills 页(tab: claude | codex)
  └─ invoke('list_skills', { agent })
       └─ Rust skills.rs:
            scan_user_skills(home, agent)   // 纯函数
            scan_plugin_skills(home, agent) // 纯函数
            parse_frontmatter(&str)         // 纯函数
          → Vec<SkillEntry>
```

- 新文件 `src-tauri/src/skills.rs`:扫描与解析均为纯函数(入参为根路径),`#[tauri::command] pub fn list_skills(agent: String)` 在 `lib.rs` 注册
- 前端新组件 `src/components/Skills.tsx`,复用 `.settings` 布局模式(返回头 + 滚动主体);`App.tsx` view 增加 `'skills'`,header 增加第三个图标入口(闪电/sparkles 风格 SVG)
- 零新运行时依赖(Rust 已有 `dirs` / `serde`);`tempfile` 仅作为 dev-dependency 供单测
- 不改动任何 fs 权限 / capabilities,不写任何文件

## 数据模型

Rust(`src-tauri/src/skills.rs`)与 TS(`src/types.ts`)字段 snake_case 严格一致(沿用 ConfigHealth 惯例):

```rust
#[derive(Serialize)]
pub struct SkillEntry {
    pub name: String,           // frontmatter name,缺失回退目录名
    pub description: String,    // frontmatter description,解析失败为空串
    pub source: String,         // "user" | "plugin"
    pub plugin_name: Option<String>, // "superpowers" / "github" 等
    pub plugin_org: Option<String>,  // "claude-plugins-official" / "openai-curated" 等
    pub plugin_description: Option<String>, // 插件 plugin.json 自述,归一单行
    pub path: String,           // SKILL.md 绝对路径(详情展示)
}
```

frontmatter 解析取舍:**手写行级解析,不引入 yaml 依赖**。只取 `name:` / `description:` 两个单行字段;值带引号则去引号;多行 YAML 值容忍降级(取已解析部分);无 frontmatter 时 name 回退目录名、description 为空。解析失败不致命(本机抽样均为单行值)。

## 命令

```rust
#[tauri::command]
pub fn list_skills(agent: String) -> Result<Vec<SkillEntry>, String>
```

- `agent` 仅接受 `"claude"` / `"codex"`,其他值返回 Err(参数错误应对用户可观测)
- `dirs::home_dir()` 失败 → Err(极罕见,前端显示错误提示)
- 其余场景(目录缺失、清单文件损坏、单条 SKILL.md 读失败)一律返回可能为空的列表;单条失败 `log::warn` 后跳过,不阻塞整页
- 同步命令即可(本地 IO 毫秒级,无阻塞风险)

## 页面 UI(720px 窗口;以下为 2026-11-05 验收迭代后的定稿形态)

- 顶栏:`← 返回` + 标题「Skills」+ 刷新按钮(强制重新 invoke)
- **Agent tab**:两枚品牌图标 tab(Claude 星芒 / OpenAI 花结,active 态品牌色,悬停有文字提示);每次切换重新 invoke(不缓存),切换期间显示加载态
- 搜索框:按名称 / 描述(中文与原文皆可命中)/ 插件名实时过滤
- **网格卡片**:一行 5 个小方块;卡片只放名称(≤2 行截断)+ 描述(≤3 行截断)
- **悬停详情弹窗**:鼠标悬停卡片弹出(不点击)——名称、完整描述、插件名(org)、路径;fixed 定位避开滚动容器裁剪,`pointer-events:none` 防滑入闪烁
- 分组:
  - 「用户目录 (N)」:`~/.<agent>/skills` 下的散装 skill,网格直铺
  - 「插件下载(K 个插件)」:**按插件折叠分组**——每插件一行(插件名 + 一句话简介 + 数量胶囊,技能多者在前),默认收起,点击展开该组网格;搜索时强制展开
  - 不做「自建/官方下载」标记:`~/.claude/skills` 下对话创建与按请求下载的 skill 落盘无任何元数据可区分(实测 grill-me 在 installed_plugins/marketplaces/catalog 均无记录)
- **中文化**:插件简介与 skill 描述经静态词典(`src/i18n/{plugins,skills}-zh.json`,键 `org/plugin`、`plugin:skill`)显示中文;无词条回退英文原文,零运行时成本;插件更新后需重新生成词典
- 空态:区分「无 skill」与「加载失败」;每个 tab 独立呈现

## 错误处理汇总

| 场景 | 行为 |
| --- | --- |
| agent 参数非法 | 命令 Err,前端提示 |
| home 目录解析失败 | 命令 Err,前端提示 |
| skills / plugins 目录不存在 | 该来源为空,正常返回 |
| installed_plugins.json 损坏 | log::warn + 回退裸扫 cache |
| 单个 SKILL.md 读取/解析失败 | log::warn 跳过,不影响整页 |

## 测试

- Rust 单测(tempfile 构造假目录树):
  - `parse_frontmatter`:单行值 / 带引号 / 缺 name / 无 frontmatter / 多行容忍
  - 扫描纯函数:自建 + 插件混合目录、跳过 `.` 开头目录、跳过无 SKILL.md 的目录、坏文件跳过、目录不存在返回空
- 前端:遵循项目现状(eslint + tsc,无 UI 测试框架),页面人工验收

## 验收清单(人工)

1. header 图标进入页面,默认 Claude Code tab,列出本机用户目录与插件 skill
2. 切到 Codex tab,列出 `~/.codex` 下 skill;`.system` 不出现
3. 插件按归属折叠分组,组头有名称+简介+数量;展开/收起、搜索强制展开可用
4. 悬停卡片弹出详情(描述中文、插件、路径)
5. 删除/新建一个 skill 目录(或安装新插件)后点刷新,清单随之变化
6. 两个 agent 目录均不存在的机器上,页面显示空态而非报错
