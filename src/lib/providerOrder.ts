/**
 * 供应商排序领域模块 —— 首页拖拽排序的身份与置换逻辑。
 *
 * 顺序唯一真相源是 config.providers 数组：后端 scheduler.rs poll_providers
 * 按数组顺序产出状态行，托盘菜单顺序随 summary 自动跟随，前端无需另存顺序。
 * 首页只展示启用行，故排序建模为「启用条目在启用槽位置换」，禁用条目
 * 原地不动 —— 不可见行的位置不被意外改写，供应商配置页顺序也与直觉一致。
 */
import type { ProviderConfig, ProviderStatus } from '../types';
import { providerLabel } from '../providerMeta';

/**
 * 卡片身份 = 类型 + 解析后别名。空别名回退类型默认名，与后端
 * poll_providers 的 display_alias 同口径（providerLabel ↔ display_name，
 * 两表已逐条核对一致）。同类型同别名双账号身份相同，靠出现顺序区分。
 */
export function cardIdentity(name: string, alias: string | null | undefined): string {
  const resolved = (alias ?? '').trim() || providerLabel(name);
  return `${name}\u0001${resolved}`;
}

export const statusIdentity = (p: ProviderStatus) => cardIdentity(p.name, p.alias);
export const configIdentity = (p: ProviderConfig) => cardIdentity(p.name, p.alias);

/**
 * 把 items 中 picked 的条目按 orderedIdentities 的顺序回填到原启用槽位，
 * 其余条目原地不动。orderedIdentities 必须与被选中条目的身份多重集完全
 * 一致（每个身份出现次数相同），否则返回 null 整体放弃 —— 乐观更新与
 * 落盘两个调用方都依赖该语义：宁可不改，也不部分应用造成双写错位。
 */
export function reorderPicked<T>(
  items: T[],
  picked: (t: T) => boolean,
  identityOf: (t: T) => string,
  orderedIdentities: string[],
): T[] | null {
  const pending = new Map<string, number[]>();
  items.forEach((t, i) => {
    if (!picked(t)) return;
    const id = identityOf(t);
    const q = pending.get(id);
    if (q) q.push(i);
    else pending.set(id, [i]);
  });

  // orderedIdentities 第 k 项将放入第 k 个启用槽位，先解出它取自哪个原下标
  const sources: number[] = [];
  for (const id of orderedIdentities) {
    const idx = pending.get(id)?.shift();
    if (idx === undefined) return null;
    sources.push(idx);
  }
  if ([...pending.values()].some((q) => q.length > 0)) return null;

  const result: T[] = [];
  let k = 0;
  items.forEach((t) => {
    if (picked(t)) {
      result.push(items[sources[k]]);
      k += 1;
    } else {
      result.push(t);
    }
  });
  return result;
}
