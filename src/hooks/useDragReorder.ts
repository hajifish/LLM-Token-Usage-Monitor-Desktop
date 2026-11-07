import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { PointerEvent as ReactPointerEvent } from 'react';

/** 位移超过该阈值才激活拖拽：点击与微动不得误触发 */
const ACTIVATE_PX = 6;
const FLIP_MS = 160;
/** 换位触发点：被拖卡中心进入目标槽位该比例深度才换位。0.25 时换位
 *  需拖约 3/4 槽宽，换回触发点留在原侧 25%，死区半槽宽 —— 手感与
 *  抗边界抖动（±3px 级）的平衡点。改成中线（0.5）用户反馈拖不动 */
const ENTER_FRAC = 0.25;

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

  const endGesture = useCallback((ev: PointerEvent) => {
    const g = gesture.current;
    if (!g || ev.pointerId !== g.pointerId) return;
    gesture.current = null;
    g.cleanup();
    document.body.classList.remove('card-dragging');
    // WebKit 双保险：激活时清过一次，结束时再清（见 handlePointerDown 注释）
    window.getSelection()?.removeAllRanges();
    if (!g.active) return;
    setDraggedKey(null);
    setOffset({ x: 0, y: 0 });
    setLive(null);
    // 未产生实际换位（误激活、边界蹭动）不提交：落盘会附带一次全量重轮询
    if (g.perm.every((v, i) => v === i)) return;
    latest.current.onCommit(g.perm.map((i) => latest.current.identityOf(g.snapshot[i])));
  }, []);

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
      setDraggedKey(g.key);
    }
    setOffset({ x: ev.clientX - g.startX, y: ev.clientY - g.startY });

    // 槽位命中（进入深度回滞）：被拖卡的视觉中心（当前槽位中心 + 指针位移）
    // 进入目标槽位 ENTER_FRAC 深度才换位，换回触发点对称留在原侧 —— 死区
    // 半槽宽，边界蹭动不触发。相邻槽位矩形相接（A.right === B.left），用
    // 「落入矩形（含边界）」做判据死区为零，边界抖动会来回换位震荡。
    // 槽位矩形在激活瞬间冻结（布局坐标，不随 FLIP/跟随 transform 变化）；
    // 斜向拖动按主轴判进深、副轴放宽同比例容差，避免斜穿时卡死
    const curPos = g.perm.findIndex((i) => g.keys[i] === g.key);
    const cur = g.slots[curPos];
    const curCx = cur.rect.left + cur.rect.width / 2;
    const curCy = cur.rect.top + cur.rect.height / 2;
    const cx = curCx + (ev.clientX - g.startX);
    const cy = curCy + (ev.clientY - g.startY);
    const to = g.slots.findIndex((s, idx) => {
      if (idx === curPos) return false;
      const r = s.rect;
      const midX = r.left + r.width / 2;
      const midY = r.top + r.height / 2;
      const slackX = r.width * ENTER_FRAC;
      const slackY = r.height * ENTER_FRAC;
      if (Math.abs(midX - curCx) >= Math.abs(midY - curCy)) {
        // 水平主轴：中心纵向落在槽内（放宽 slackY），且水平进入目标槽
        // 近侧边 slackX 深度
        if (cy < r.top - slackY || cy > r.bottom + slackY) return false;
        return midX > curCx ? cx >= r.left + slackX : cx <= r.right - slackX;
      }
      if (cx < r.left - slackX || cx > r.right + slackX) return false;
      return midY > curCy ? cy >= r.top + slackY : cy <= r.bottom - slackY;
    });
    if (to < 0) return;
    const from = curPos;

    // FLIP 基线：重排前的视口坐标
    prevRects.current.clear();
    cards.current.forEach((el, key) => prevRects.current.set(key, el.getBoundingClientRect()));

    const perm = g.perm.slice();
    perm.splice(to, 0, perm.splice(from, 1)[0]);
    g.perm = perm;
    setLive({ items: perm.map((i) => g.snapshot[i]), keys: perm.map((i) => g.keys[i]) });
  }, []);

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
