import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { AppConfig } from '../types';

export interface FrozenBackgroundState {
  mode: 'follow' | 'frozen';
  imageUrl: string | null;
  busy: boolean;
  setMode: (mode: 'follow' | 'frozen') => Promise<void>;
}

/** 内置固定背景图（开发期一次性截取的半透明效果，随包分发）。 */
const FROZEN_BACKGROUND_URL = '/default-background.png';

/** 预加载：确认图真正可取回后再铺图/切材质，避免铺图与材质移除之间闪透明。 */
function preload(url: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve();
    img.onerror = () => reject(new Error('背景图加载失败'));
    img.src = url;
  });
}

/**
 * 背景模式状态与切换。
 *
 * 固定背景 = 铺内置图并移除窗口材质（不随桌面壁纸变化，默认）；跟随系统 = 实时
 * hudWindow 半透明（应用现状）。切换走即时生效纪律：读 fresh 配置 → 仅改
 * background_mode → 落盘；失败时状态不翻转（开关回弹）。
 *
 * 时序（两个方向都保证无闪透明）：
 * - frozen：落盘 → 预加载 → 铺图（盖住材质）→ 移除材质
 * - follow：落盘 → 恢复材质（在图之下，不可见）→ 撤图露出材质
 */
export function useFrozenBackground(): FrozenBackgroundState {
  const [mode, setModeState] = useState<'follow' | 'frozen'>('follow');
  const [imageUrl, setImageUrl] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // 启动：配置为 frozen → 预加载内置图 → 铺图 → 移除材质（失败仅记录，保持跟随观感）
  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const cfg = await invoke<AppConfig>('get_config');
        if (!alive || cfg.background_mode !== 'frozen') return;
        await preload(FROZEN_BACKGROUND_URL);
        if (!alive) return;
        setModeState('frozen');
        setImageUrl(FROZEN_BACKGROUND_URL);
        invoke('apply_window_effects', { mode: 'frozen' }).catch((e) => {
          console.error('移除窗口材质失败:', e);
        });
      } catch {
        // get_config / 预加载失败：保持跟随外观（材质未动），配置问题由设置页既有链路提示
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  const setMode = useCallback(async (next: 'follow' | 'frozen') => {
    setBusy(true);
    try {
      const fresh = await invoke<AppConfig>('get_config');
      fresh.background_mode = next;
      await invoke('save_config', { newConfig: fresh });

      if (next === 'frozen') {
        await preload(FROZEN_BACKGROUND_URL);
        setModeState('frozen');
        setImageUrl(FROZEN_BACKGROUND_URL);
        // 软失败：背景已固定，仅提示（材质残留只影响圆角外观）
        await invoke('apply_window_effects', { mode: 'frozen' }).catch((e) => {
          throw new Error(`背景已固定，但窗口材质切换失败：${e}`);
        });
      } else {
        await invoke('apply_window_effects', { mode: 'follow' }).catch((e) => {
          // 恢复材质失败则不撤图（撤了就是全透明窗口），保持固定观感并提示
          throw new Error(`配置已改为跟随系统，但恢复半透明失败：${e}`);
        });
        setModeState('follow');
        setImageUrl(null);
      }
    } finally {
      setBusy(false);
    }
  }, []);

  return { mode, imageUrl, busy, setMode };
}
