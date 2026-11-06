import { useEffect, useRef } from 'react';
import { LogicalSize } from '@tauri-apps/api/dpi';
import { getCurrentWindow } from '@tauri-apps/api/window';

/** 窗宽，与 tauri.conf.json 对齐；改窗宽需两处同步 */
const WIDTH = 720;

/** 收缩下限，防止窗口塌成不可用 */
const MIN_HEIGHT = 200;

/**
 * 主窗口高度自适应：内容用不满时窗口向上收缩，增多时长回，上限为启动时的
 * 窗口逻辑高。naturalHeight = 滚动容器底边（相对 .app 顶）+ 被裁剪的溢出量，
 * 本 hook 是全仓库唯一 setSize 写入点。加载/空态无滚动容器标记，自然跳过，
 * 窗口保持当前高度。自身 setSize 触发的 resized 回声会再测一次，
 * target 不变即终止，不成环。
 *
 * 触发即测、不经 rAF 合帧：托盘应用大部分时间窗口隐藏，隐藏态没有帧，
 * rAF 会让测量挂起直到窗口显示，表现为开窗先闪旧尺寸再跳正。
 * MutationObserver 本身按变更批交付，无需二次合并。
 */
export function useAutoWindowHeight(
  view: 'usage' | 'settings' | 'providers' | 'skills',
) {
  const maxHeightRef = useRef<number | null>(null);
  const lastAppliedRef = useRef(0);

  useEffect(() => {
    // 浏览器 / UI-mock 环境无 Tauri runtime，no-op 保证纯前端开发可用
    if (!('__TAURI_INTERNALS__' in window)) return;

    const win = getCurrentWindow();
    let disposed = false;
    let unlisten: (() => void) | undefined;

    const measure = () => {
      const scroller = document.querySelector<HTMLElement>(
        '[data-auto-height-scroller]',
      );
      const app = document.querySelector('.app');
      const maxHeight = maxHeightRef.current;
      if (!scroller || !app || maxHeight === null) return;
      const scrollerRect = scroller.getBoundingClientRect();
      const appRect = app.getBoundingClientRect();
      const natural =
        scrollerRect.bottom - appRect.top + (scroller.scrollHeight - scroller.clientHeight);
      const target = Math.min(
        Math.max(Math.round(natural), MIN_HEIGHT),
        Math.round(maxHeight),
      );
      if (Math.abs(target - lastAppliedRef.current) >= 1) {
        lastAppliedRef.current = target;
        win.setSize(new LogicalSize(WIDTH, target)).catch((e) => {
          console.error('调整窗口高度失败:', e);
        });
      }
    };

    // 上限只读一次（启动时窗口逻辑高）：effect 随 view 切换重跑，若每次
    // 重读 innerSize，会把已收缩的当前高度误当上限，窗口再也无法长回。
    // ref 存值不等于只读一次——读取动作本身也必须只发生一次。
    if (maxHeightRef.current === null) {
      Promise.all([win.innerSize(), win.scaleFactor()])
        .then(([size, factor]) => {
          if (disposed) return;
          maxHeightRef.current = size.toLogical(factor).height;
          measure();
        })
        .catch(console.error);
    } else {
      measure();
    }

    // 观察 body 而非 .app：ErrorBoundary「重试」会重建 .app 节点，
    // 盯旧节点会让自适应静默失灵；measure 内部对 .app 缺失已有空判
    const observer = new MutationObserver(measure);
    observer.observe(document.body, {
      childList: true,
      subtree: true,
      characterData: true,
    });
    win.onResized(() => measure()).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });

    return () => {
      disposed = true;
      observer.disconnect();
      unlisten?.();
    };
  }, [view]);
}
