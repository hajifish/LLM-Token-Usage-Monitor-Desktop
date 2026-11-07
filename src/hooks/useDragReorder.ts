import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { PointerEvent as ReactPointerEvent } from 'react';

/** 位移超过该阈值才激活拖拽：点击与微动不得误触发 */
const ACTIVATE_PX = 6;
const FLIP_MS = 160;

interface LiveState<T> {
  items: T[];
  keys: string[];
}

interface Gesture<T> {
  pointerId: number;
  active: boolean;
  key: string;
  startX: number;
  startY: number;
  /** 手势期间冻结的快照与 key：轮询更新 items 不打断手势；冻结 key 让
   *  同身份双账号在换位时保持同一 DOM 元素，FLIP 才能动画而非重挂载 */
  snapshot: T[];
  keys: string[];
  /** 当前展示位置 -> 快照下标 */
  perm: number[];
  /** 激活瞬间的槽位矩形（快照顺序）：hit-test 的坐标系。用布局快照而非
   *  elementsFromPoint，后者命中的是 FLIP 动画中卡片的绘制位置，快速
   *  拖动时会目标震荡 */
  slots: { key: string; rect: DOMRect }[];
  /** 起点槽位矩形：判定中心与渲染位移的锚。换位后当前槽位改变，但锚
   *  不变 —— 中心若按「当前槽位 + 起点量的全程位移」计算，换位瞬间会
   *  凭空多出一格越界量，下一 move 立即连环换位，卡片飞到最远端
   *  （真机反馈「拖到中间弹到最边上」的根因） */
  originRect: DOMRect;
  cleanup: () => void;
}

export interface DragReorder<T> {
  /** 当前应渲染的顺序：手势期间为活顺序，空闲时回退 items */
  items: T[];
  /** 与 items 一一对应的稳定 DOM key（身份 + 同身份出现序号） */
  keys: string[];
  draggedKey: string | null;
  offset: { x: number; y: number };
  handlePointerDown: (e: ReactPointerEvent<HTMLElement>, index: number) => void;
  setCardRef: (key: string, el: HTMLElement | null) => void;
}

/**
 * 指针拖拽重排 hook。pointer 事件自实现而非 HTML5 DnD：WKWebView 内
 * dragstart 行为不稳，且 Playwright 真鼠标拖拽不触发 HTML5 DnD，无法
 * 自动化验收；pointer 方案浏览器与真机行为一致。
 *
 * 手势生命周期：pointerdown 记起点（未激活）→ 位移过阈值激活（快照 +
 * 槽位矩形 + 指针捕获）→ move 期间按指针所在的原始槽位活重排（其余卡
 * FLIP 让位，被拖卡跟随指针）→ up/cancel 提交最终顺序的身份列表。
 * cancel 与正常松手同路提交：活重排已被用户看见，回退反而撒谎。
 */
export function useDragReorder<T>(opts: {
  items: T[];
  identityOf: (t: T) => string;
  onCommit: (orderedIdentities: string[]) => void;
}): DragReorder<T> {
  const { items, identityOf, onCommit } = opts;
  const [live, setLive] = useState<LiveState<T> | null>(null);
  const [draggedKey, setDraggedKey] = useState<string | null>(null);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const cards = useRef(new Map<string, HTMLElement>());
  const prevRects = useRef(new Map<string, DOMRect>());
  const flipGen = useRef(0);
  const gesture = useRef<Gesture<T> | null>(null);
  /** onCommit/identityOf 走 latest ref：window 监听器闭包不随重渲染刷新 */
  const latest = useRef({ identityOf, onCommit });
  useEffect(() => {
    latest.current = { identityOf, onCommit };
  });

  const idleKeys = useMemo(() => {
    const seen = new Map<string, number>();
    return items.map((t) => {
      const id = identityOf(t);
      const n = seen.get(id) ?? 0;
      seen.set(id, n + 1);
      return `${id}\u0002${n}`;
    });
  }, [items, identityOf]);

  const setCardRef = useCallback((key: string, el: HTMLElement | null) => {
    if (el) cards.current.set(key, el);
    else cards.current.delete(key);
  }, []);

  /** 槽位命中（容纳 + 中线回滞）：所有槽位可达（3+1 两行布局里第二行
   *  卡片要能直达第一行中间，相邻一格模型到不了），但双重条件缺一不可 ——
   *  被拖卡视觉中心落入目标槽矩形（矩形互不重叠，候选唯一，无先后顺序
   *  抽签），且在主轴方向越过该槽中线。判定中心纯指针驱动（起点槽位锚
   *  + 全程位移），换位后不重锚 —— 若按「当前槽位 + 全程位移」计算，
   *  换位瞬间凭空多出一格越界量，下一 move 连环换位，卡片飞到最远端
   *  （真机反馈「拖到中间弹到最边上」的根因）。换回需完整退过原槽中线，
   *  死区整槽宽；斜穿目标槽角落时中线未越过、不触发，不会路过就被抢走。
   *  禁用任何容差放大。槽位矩形在激活瞬间冻结（布局坐标，不随 FLIP/
   *  跟随 transform 变化）。返回换位后的位置 */
  const evaluateSwap = useCallback((g: Gesture<T>, clientX: number, clientY: number): number => {
    const curPos = g.perm.findIndex((i) => g.keys[i] === g.key);
    const cur = g.slots[curPos];
    const cx = g.originRect.left + g.originRect.width / 2 + (clientX - g.startX);
    const cy = g.originRect.top + g.originRect.height / 2 + (clientY - g.startY);
    const curCx = cur.rect.left + cur.rect.width / 2;
    const curCy = cur.rect.top + cur.rect.height / 2;
    const to = g.slots.findIndex((s, idx) => {
      if (idx === curPos) return false;
      const r = s.rect;
      if (cx < r.left || cx > r.right || cy < r.top || cy > r.bottom) return false;
      const midX = r.left + r.width / 2;
      const midY = r.top + r.height / 2;
      // 1px 容差：终点恰落在目标中心时浮点舍入会让等值比较时灵时不灵
      // （真机快甩以格心为终点，Lane A 复现过同点成败不一）
      const EPS = 1;
      if (Math.abs(midX - curCx) >= Math.abs(midY - curCy)) {
        return midX > curCx ? cx >= midX - EPS : cx <= midX + EPS;
      }
      return midY > curCy ? cy >= midY - EPS : cy <= midY + EPS;
    });
    if (to < 0 || to === curPos) return curPos;
    // FLIP 基线：重排前的视口坐标
    prevRects.current.clear();
    cards.current.forEach((el, key) => prevRects.current.set(key, el.getBoundingClientRect()));
    const perm = g.perm.slice();
    perm.splice(to, 0, perm.splice(curPos, 1)[0]);
    g.perm = perm;
    setLive({ items: perm.map((i) => g.snapshot[i]), keys: perm.map((i) => g.keys[i]) });
    return to;
  }, []);

  const endGesture = useCallback((ev: PointerEvent) => {
    const g = gesture.current;
    if (!g || ev.pointerId !== g.pointerId) return;
    gesture.current = null;
    g.cleanup();
    document.body.classList.remove('card-dragging');
    // WebKit 双保险：激活时清过一次，结束时再清（见 handlePointerDown 注释）
    window.getSelection()?.removeAllRanges();
    if (!g.active) return;
    // 松手前用最终坐标再判定一次换位：无停顿快甩时浏览器会合并
    // pointermove，最后一跃可能不产生 move 事件（Lane A 复现过）
    evaluateSwap(g, ev.clientX, ev.clientY);
    setDraggedKey(null);
    setOffset({ x: 0, y: 0 });
    setLive(null);
    // 未产生实际换位（误激活、边界蹭动）不提交：落盘会附带一次全量重轮询
    if (g.perm.every((v, i) => v === i)) return;
    latest.current.onCommit(g.perm.map((i) => latest.current.identityOf(g.snapshot[i])));
  }, [evaluateSwap]);

  const beginMove = useCallback((ev: PointerEvent) => {
    const g = gesture.current;
    if (!g || ev.pointerId !== g.pointerId) return;
    if (!g.active) {
      if (Math.hypot(ev.clientX - g.startX, ev.clientY - g.startY) < ACTIVATE_PX) return;
      g.active = true;
      document.body.classList.add('card-dragging');
      window.getSelection()?.removeAllRanges();
      try {
        cards.current.get(g.key)?.setPointerCapture(g.pointerId);
      } catch {
        // 捕获失败只损失窗外跟踪，窗口内手势不受影响
      }
      g.slots = g.keys
        .map((key) => ({ key, rect: cards.current.get(key)?.getBoundingClientRect() }))
        .filter((s): s is { key: string; rect: DOMRect } => !!s.rect);
      const originIdx = g.keys.indexOf(g.key);
      if (originIdx >= 0 && g.slots[originIdx]) g.originRect = g.slots[originIdx].rect;
      // 上一次手势的落位动画可能仍在飞行中，getBoundingClientRect 会拍到
      // 动画里的偏移位置，槽位坐标系就歪了 —— 激活时先清掉残留 inline 样式
      cards.current.forEach((el) => {
        if (el.dataset.flipGen) {
          el.style.transition = '';
          el.style.transform = '';
          delete el.dataset.flipGen;
        }
      });
      setDraggedKey(g.key);
    }

    const pos = evaluateSwap(g, ev.clientX, ev.clientY);

    // 渲染位移：扣掉最终槽位相对起点槽位的差值，换位后卡片钉在指针底下。
    // 必须在换位判定之后按新槽位计算 —— 否则换位当帧用的是旧槽位偏移，
    // 卡片闪跳一格（Lane A 逐帧断言实测 316px 跳变）
    const fin = g.slots[pos];
    setOffset({
      x: ev.clientX - g.startX - (fin.rect.left - g.originRect.left),
      y: ev.clientY - g.startY - (fin.rect.top - g.originRect.top),
    });
  }, [evaluateSwap]);

  // FLIP：重排后从旧位置动画到新位置。inline transition 会覆盖样式表
  // .p-card 的 transition:all，动画结束必须清掉，否则卡片 hover 过渡失效
  useLayoutEffect(() => {
    if (live === null || draggedKey === null) return;
    const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    cards.current.forEach((el, key) => {
      if (key === draggedKey) return;
      const prev = prevRects.current.get(key);
      if (!prev) return;
      const now = el.getBoundingClientRect();
      const dx = prev.left - now.left;
      const dy = prev.top - now.top;
      if (dx === 0 && dy === 0) return;
      if (reduce) return;
      const gen = String(++flipGen.current);
      el.dataset.flipGen = gen;
      el.style.transition = 'none';
      el.style.transform = `translate(${dx}px, ${dy}px)`;
      el.getBoundingClientRect();
      el.style.transition = `transform ${FLIP_MS}ms ease`;
      el.style.transform = '';
      const settle = () => {
        if (el.dataset.flipGen !== gen) return; // 已被更新的让位动画接管
        el.style.transition = '';
        el.style.transform = '';
        delete el.dataset.flipGen;
      };
      el.addEventListener('transitionend', settle, { once: true });
      setTimeout(settle, FLIP_MS + 100);
    });
    prevRects.current.clear();
  }, [live, draggedKey]);

  const handlePointerDown = useCallback(
    (e: ReactPointerEvent<HTMLElement>, index: number) => {
      if (e.button !== 0 || gesture.current) return;
      // 原生文本选择手势在 pointerdown 即被浏览器武装，激活后再清选区
      // （removeAllRanges + user-select:none）在 WebKit 上拦不住已启动的
      // 选择继续扩展 —— 必须在起点就 preventDefault 断掉它。代价是卡片
      // 文字不可划选复制，换稳定的拖拽体验
      e.preventDefault();
      const move = (ev: PointerEvent) => beginMove(ev);
      const detach = () => {
        window.removeEventListener('pointermove', move);
        window.removeEventListener('pointerup', up);
        window.removeEventListener('pointercancel', up);
      };
      const up = (ev: PointerEvent) => {
        detach();
        endGesture(ev);
      };
      window.addEventListener('pointermove', move);
      window.addEventListener('pointerup', up);
      window.addEventListener('pointercancel', up);
      gesture.current = {
        pointerId: e.pointerId,
        active: false,
        key: idleKeys[index],
        startX: e.clientX,
        startY: e.clientY,
        snapshot: items,
        keys: idleKeys,
        perm: items.map((_, i) => i),
        slots: [],
        // 占位：激活时以冻结槽位表为准覆写；beginMove 仅在激活后才使用
        originRect: new DOMRect(),
        cleanup: detach,
      };
    },
    [items, idleKeys, beginMove, endGesture],
  );

  return {
    items: live?.items ?? items,
    keys: live?.keys ?? idleKeys,
    draggedKey,
    offset,
    handlePointerDown,
    setCardRef,
  };
}
