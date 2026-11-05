import { useState, useEffect } from 'react';
import { getVersion } from '@tauri-apps/api/app';
import './styles/app.css';
import UsagePanel from './components/UsagePanel';
import Settings from './components/Settings';
import Providers from './components/Providers';
import Skills from './components/Skills';
import UpdateNotice from './components/UpdateNotice';
import ErrorBoundary from './components/ErrorBoundary';
import { listen } from '@tauri-apps/api/event';
import { useUsage } from './hooks/useUsage';
import { useFrozenBackground } from './hooks/useFrozenBackground';

function App() {
  const [view, setView] = useState<'usage' | 'settings' | 'providers' | 'skills'>('usage');
  const { summary, loading, refreshing, error, refresh, refreshAfterProviderChange } = useUsage();
  const background = useFrozenBackground();
  const [appVersion, setAppVersion] = useState<string | null>(null);

  useEffect(() => {
    getVersion().then(setAppVersion).catch(console.error);
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
      {background.imageUrl && (
        <div
          className="background-frozen"
          style={{ backgroundImage: `url(${background.imageUrl})` }}
        />
      )}
      <div className="app">
        {view === 'usage' ? (
          <div className="usage-view">
            <div className="app-header" data-tauri-drag-region>
              <h1 className="app-title">
                <span className="app-title-logo-wrap">
                  <img src="/tray-logo.png" alt="" className="app-title-logo" />
                </span>
                <span className="app-title-wordmark">
                  Token<span className="wordmark-accent">Usage</span>
                </span>
              </h1>
              <div className="header-actions">
                {summary?.last_updated && (
                  // 主页只显时:分:秒（后端格式 "YYYY-MM-DD HH:MM:SS"，取空格后段；
                  // 日期冗余——用户关心的是数据新鲜度，不是哪天）
                  <span className="header-time">
                    {summary.last_updated.split(' ')[1] ?? summary.last_updated}
                  </span>
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
                  onClick={() => setView('skills')}
                  title="Skill 清单"
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
                    <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"></polygon>
                  </svg>
                </button>
                <button
                  className="settings-icon-btn"
                  onClick={() => setView('providers')}
                  title="供应商配置"
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
                    <line x1="12" y1="5" x2="12" y2="19"></line>
                    <line x1="5" y1="12" x2="19" y2="12"></line>
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
        ) : view === 'settings' ? (
          <Settings onBack={() => setView('usage')} background={background} />
        ) : view === 'providers' ? (
          <Providers
            onBack={() => {
              setView('usage');
              // 乐观显示：新供应商行立即出现在面板上（行内「加载中」），
              // 真实数据由随后的刷新轮补齐——慢供应商也能马上确认添加成功
              refreshAfterProviderChange();
            }}
          />
        ) : (
          <Skills onBack={() => setView('usage')} />
        )}
        {appVersion && <div className="version-tag">v{appVersion}</div>}
      </div>
    </ErrorBoundary>
  );
}

export default App;
