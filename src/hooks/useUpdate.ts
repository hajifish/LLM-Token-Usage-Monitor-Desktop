import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { UpdateStatus } from '../types';

/**
 * 应用内更新状态。
 *
 * 状态由 Rust 侧持有并主动推送（update-status 事件），本 hook 只做订阅与命令触发，
 * 因此窗口隐藏时更新检查与下载进度也不会中断（不依赖 webview 定时器）。
 */
export function useUpdate() {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<UpdateStatus>('get_update_status')
      .then(setStatus)
      .catch(() => {});
    const unlisten = listen<UpdateStatus>('update-status', (event) => {
      setStatus(event.payload);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const check = useCallback(async () => {
    setError(null);
    try {
      setStatus(await invoke<UpdateStatus>('check_update'));
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const download = useCallback(async () => {
    setError(null);
    try {
      setStatus(await invoke<UpdateStatus>('download_update'));
    } catch (e) {
      setError(String(e));
    }
  }, []);

  /** 触发安装并重启；成功后应用会直接重启，不会返回。 */
  const install = useCallback(async () => {
    setError(null);
    try {
      await invoke('install_update');
    } catch (e) {
      setError(String(e));
    }
  }, []);

  return { status, error, check, download, install };
}
