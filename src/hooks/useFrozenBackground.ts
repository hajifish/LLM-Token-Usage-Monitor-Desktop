import { useCallback, useEffect, useState } from 'react';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { appDataDir, join } from '@tauri-apps/api/path';
import type { AppConfig } from '../types';

export interface FrozenBackgroundState {
  mode: 'follow' | 'frozen';
  imageUrl: string | null;
  busy: boolean;
  /** 启动时配置为固定背景但图片加载失败（缺失/损坏）——设置页显示横幅。 */
  loadFailed: boolean;
  setMode: (mode: 'follow' | 'frozen') => Promise<void>;
}

/** 等待 WebView 完成重绘：双 rAF 保证合成帧提交，再留 150ms 给 WKWebView 落屏。 */
async function nextPaint() {
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
  await new Promise<void>((resolve) => setTimeout(resolve, 150));
}

/** 预加载探测：URL 能否真的取回图（assetProtocol/文件缺失都会走 onerror）。 */
function preload(url: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve();
    img.onerror = () => reject(new Error('背景图加载失败'));
    img.src = url;
  });
}

/** 数据目录 background.png → assetProtocol URL；时间戳 query 防会话内重截后读到缓存。 */
async function buildUrl(): Promise<string> {
  const path = await join(await appDataDir(), 'background.png');
  return `${convertFileSrc(path)}?v=${Date.now()}`;
}

/**
 * 背景固化状态与切换编排。
 *
 * 编排时序（spec「截取流程」）：进入捕获态（html.capture-blank 隐藏全部内容，
 * 只剩窗口材质）→ 等重绘 → capture_background → 读 fresh 配置改本字段落盘 →
 * 预加载新图 → 铺图退出捕获态 → apply_window_effects。
 * 失败路径：退出捕获态、抛错、状态不翻转（开关回弹）、配置不落盘。
 */
export function useFrozenBackground(): FrozenBackgroundState {
  const [mode, setModeState] = useState<'follow' | 'frozen'>('follow');
  const [imageUrl, setImageUrl] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [loadFailed, setLoadFailed] = useState(false);

  // 启动：配置为 frozen 时直接加载存量图（不重截，避免闪屏）；
  // 加载失败回退跟随外观并置 loadFailed 供设置页横幅提示。
  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const cfg = await invoke<AppConfig>('get_config');
        if (!alive || cfg.background_mode !== 'frozen') return;
        try {
          const url = await buildUrl();
          await preload(url);
          if (!alive) return;
          setImageUrl(url);
        } catch {
          if (alive) setLoadFailed(true);
        }
      } catch {
        // get_config 失败：保持跟随外观，配置问题由设置页既有链路提示
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  const setMode = useCallback(async (next: 'follow' | 'frozen') => {
    if (next === 'frozen') {
      setBusy(true);
      document.documentElement.classList.add('capture-blank');
      try {
        await nextPaint();
        await invoke<string>('capture_background');
        // 读最新配置 → 仅改本字段 → 落盘（与设置页其他即时生效区块同纪律）
        const fresh = await invoke<AppConfig>('get_config');
        fresh.background_mode = 'frozen';
        await invoke('save_config', { newConfig: fresh });
        const url = await buildUrl();
        await preload(url);
        setModeState('frozen');
        setImageUrl(url);
        setLoadFailed(false);
      } finally {
        document.documentElement.classList.remove('capture-blank');
        setBusy(false);
      }
      // 材质切换失败为软失败：背景已固定，仅提示（spec 错误处理表）
      await invoke('apply_window_effects', { mode: 'frozen' }).catch((e) => {
        throw new Error(`背景已固定，但窗口材质切换失败：${e}`);
      });
    } else {
      setBusy(true);
      try {
        const fresh = await invoke<AppConfig>('get_config');
        fresh.background_mode = 'follow';
        await invoke('save_config', { newConfig: fresh });
        setModeState('follow');
        setImageUrl(null);
        setLoadFailed(false);
      } finally {
        setBusy(false);
      }
      await invoke('apply_window_effects', { mode: 'follow' }).catch((e) => {
        throw new Error(`已恢复跟随系统，但窗口材质切换失败：${e}`);
      });
    }
  }, []);

  return { mode, imageUrl, busy, loadFailed, setMode };
}
