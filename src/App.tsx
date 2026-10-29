import { useState, useEffect, useCallback, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import './styles/app.css';
import UsagePanel from './components/UsagePanel';
import Settings from './components/Settings';
import UpdateNotice from './components/UpdateNotice';
import ErrorBoundary from './components/ErrorBoundary';
import { listen } from '@tauri-apps/api/event';
import { useUsage } from './hooks/useUsage';
import type { AppConfig } from './types';

function App() {
  const [view, setView] = useState<'usage' | 'settings'>('usage');
  const { summary, loading, refreshing, error, refresh } = useUsage();
  const [trayTextEnabled, setTrayTextEnabled] = useState(true);
  /** 球开关保存进行中标记：get_config → save_config 为读改写，连点两次会两次读到旧值丢一次切换 */
  const togglingRef = useRef(false);
  const [toggling, setToggling] = useState(false);

  /** 拉取托盘文字开关；从设置页切回时也刷新，避免设置页保存后状态过期 */
  const fetchTrayEnabled = useCallback(async () => {
    try {
      const cfg = await invoke<AppConfig>('get_config');
      setTrayTextEnabled(cfg.tray_text_enabled);
    } catch (e) {
      console.error(e);
    }
  }, []);

  useEffect(() => {
    fetchTrayEnabled();
  }, [fetchTrayEnabled, view]);

  /** 球开关即时保存：save_config 唤醒调度器重建托盘，菜单栏立即生效；
   *  保存进行中直接忽略再次触发，避免读改写竞态丢失一次切换 */
  const toggleTrayText = useCallback(async () => {
    if (togglingRef.current) return;
    togglingRef.current = true;
    setToggling(true);
    try {
      const cfg = await invoke<AppConfig>('get_config');
      const next = { ...cfg, tray_text_enabled: !cfg.tray_text_enabled };
      await invoke('save_config', { newConfig: next });
      setTrayTextEnabled(next.tray_text_enabled);
    } catch (e) {
      console.error(e);
    } finally {
      togglingRef.current = false;
      setToggling(false);
    }
  }, []);

  useEffect(() => {
    const unlisten = listen<string>('navigate', (event) => {
      if (event.payload === 'settings') {
        setView('settings');
      } else if (event.payload === 'usage') {
        setView('usage');
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  return (
    <ErrorBoundary>
      <div className="app">
        {view === 'usage' ? (
          <div className="usage-view">
            <div className="app-header" data-tauri-drag-region>
              <h1 className="app-title">
                <span className="app-title-logo-wrap">
                  <img src="/tray-logo.png" alt="" className="app-title-logo" />
                </span>
                用量
              </h1>
              <div className="header-actions">
                <label
                  className="ball-toggle"
                  title="菜单栏显示开关"
                  aria-disabled={toggling}
                >
                  <input type="checkbox" checked={trayTextEnabled} onChange={toggleTrayText} />
                  <span className="ball-dot" aria-hidden="true" />
                  <span className="ball-label">状态栏</span>
                </label>
                {summary?.last_updated && (
                  <span className="header-time">{summary.last_updated}</span>
                )}
                <button
                  className={`header-refresh-btn${refreshing ? ' is-refreshing' : ''}`}
                  onClick={refresh}
                  title="刷新数据"
                >
                  <svg
                    width="14"
                    height="14"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2.2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <polyline points="23 4 23 10 17 10"></polyline>
                    <polyline points="1 20 1 14 7 14"></polyline>
                    <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"></path>
                  </svg>
                </button>
                <button
                  className="settings-icon-btn"
                  onClick={() => setView('settings')}
                  title="设置"
                >
                  <svg
                    width="18"
                    height="18"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <circle cx="12" cy="12" r="3"></circle>
                    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"></path>
                  </svg>
                </button>
              </div>
            </div>
            <UpdateNotice />
            <UsagePanel
              summary={summary}
              loading={loading}
              refreshing={refreshing}
              error={error}
              refresh={refresh}
            />
          </div>
        ) : (
          <Settings onBack={() => setView('usage')} />
        )}
      </div>
    </ErrorBoundary>
  );
}

export default App;
