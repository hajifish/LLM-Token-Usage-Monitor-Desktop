import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { UsageSummary } from '../types';

export type { BalanceData, QuotaInfo, UsageData, ProviderStatus, UsageSummary } from '../types';

export function useUsage() {
  const [summary, setSummary] = useState<UsageSummary | null>(null);
  // loading 仅表示首次加载（尚无任何数据）；手动刷新用 refreshing，避免整面板被 loading 替换
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // 注意：fetchUsage 内不做同步 setState（首行为即 await），
  // 否则触发 react-hooks/set-state-in-effect；清错误由 refresh 负责
  const fetchUsage = useCallback(async () => {
    try {
      const data = await invoke<UsageSummary>('get_usage');
      setSummary(data);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  const refresh = useCallback(async () => {
    setRefreshing(true);
    setError(null);
    try {
      // 先注册一次性事件监听，确保不遗漏后端发出的 usage-updated
      const eventPromise = new Promise<UsageSummary>((resolve) => {
        listen<UsageSummary>('usage-updated', (event) => {
          resolve(event.payload);
        });
      });

      // 通知后端立即轮询供应商
      await invoke('trigger_refresh');

      // 等待 usage-updated 事件（10 秒超时兜底）
      const timeout = new Promise<UsageSummary | null>((resolve) =>
        setTimeout(() => resolve(null), 10000),
      );

      const result = await Promise.race([eventPromise, timeout]);
      if (result) {
        setSummary(result);
      }
      await fetchUsage(); // 最终确认
    } catch {
      await fetchUsage();
    } finally {
      setRefreshing(false);
    }
  }, [fetchUsage]);

  useEffect(() => {
    // 初始加载用 promise 回调而非直接调用 fetchUsage：
    // 满足 react-hooks/set-state-in-effect（setState 仅出现在回调中），
    // 并通过 active 标记避免卸载后的 setState
    let active = true;
    invoke<UsageSummary>('get_usage')
      .then((data) => {
        if (active) setSummary(data);
      })
      .catch((e) => {
        if (active) setError(String(e));
      })
      .finally(() => {
        if (active) setLoading(false);
      });

    const unlisten = listen<UsageSummary>('usage-updated', (event) => {
      setSummary(event.payload);
      setLoading(false);
    });

    return () => {
      active = false;
      unlisten.then((fn) => fn());
    };
  }, []);

  return { summary, loading, refreshing, error, refresh };
}
