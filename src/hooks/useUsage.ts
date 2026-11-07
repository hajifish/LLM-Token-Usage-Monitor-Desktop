import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { AppConfig, ProviderStatus, UsageSummary } from '../types';
import { providerLabel } from '../providerMeta';
import { reorderPicked, statusIdentity } from '../lib/providerOrder';

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

  /** 供应商变更后的刷新：先把「配置里有、上一轮结果里还没有」的启用供应商
   *  乐观合成为待刷新行（面板立即给出「加上了」的反馈，行内显示加载中），
   *  再走常规 refresh 等真实数据落盘替换。匹配按类型计数差额：同类型多账号、
   *  别名空回退显示名的口径差异都能对齐，不会把存量行误判为新增。
   *  配置读取失败则跳过乐观显示，退化为纯 refresh。 */
  const refreshAfterProviderChange = useCallback(async () => {
    try {
      const cfg = await invoke<AppConfig>('get_config');
      setSummary((cur) => {
        // 每个类型已出现在上一轮结果里的数量；配置遍历时逐个抵扣，差额即新增
        const seen = new Map<string, number>();
        for (const p of cur?.providers ?? []) {
          seen.set(p.name, (seen.get(p.name) ?? 0) + 1);
        }
        const pending: ProviderStatus[] = [];
        for (const pc of cfg.providers) {
          if (!pc.enabled) continue;
          const n = seen.get(pc.name) ?? 0;
          if (n > 0) {
            seen.set(pc.name, n - 1);
          } else {
            pending.push({
              name: pc.name,
              alias: pc.alias?.trim() || providerLabel(pc.name),
              enabled: true,
              balance: null,
              usage: null,
              error: null,
              quota_infos: null,
            });
          }
        }
        if (!pending.length) return cur;
        return {
          providers: [...(cur?.providers ?? []), ...pending],
          total_balance: cur?.total_balance ?? 0,
          last_updated: cur?.last_updated ?? '',
        };
      });
    } catch {
      // 配置读取失败：不做乐观显示，直接走刷新
    }
    await refresh();
  }, [refresh]);

  /** 拖拽排序的乐观本地应用：按身份顺序置换 summary 启用行（禁用行不动）。
   *  身份多重集对不上时静默不动 —— 面板与配置短暂不一致（如轮询竞态）时
   *  宁可不改也不错改，下轮 usage-updated 自然回到真实顺序。 */
  const applyLocalOrder = useCallback((orderedIdentities: string[]) => {
    setSummary((cur) => {
      if (!cur) return cur;
      const providers = reorderPicked(
        cur.providers,
        (p) => p.enabled,
        statusIdentity,
        orderedIdentities,
      );
      return providers ? { ...cur, providers } : cur;
    });
  }, []);

  return {
    summary,
    loading,
    refreshing,
    error,
    refresh,
    refreshAfterProviderChange,
    applyLocalOrder,
  };
}
