import React, { useState, useEffect } from 'react';
import {
  Inbox,
  ArrowDownCircle,
  CheckCircle2,
  PauseCircle,
  Clock,
  AlertCircle,
  FileArchive,
  FileText,
  Film,
  Music,
  AppWindow,
  File,
  Settings as SettingsIcon,
  Zap,
  Activity,
  Download,
} from 'lucide-react';
import { FileCategory } from '../../types/download';
import { formatSpeed } from '../../utils/format';
import { api } from '../../services/api';

interface SidebarProps {
  currentTab: string;
  onSelectTab: (tab: string) => void;
  selectedCategory: FileCategory;
  onSelectCategory: (category: FileCategory) => void;
  counts: {
    all: number;
    downloading: number;
    completed: number;
    paused: number;
    scheduled: number;
    failed: number;
  };
  aggregateSpeed: number;
  onOpenSettings: () => void;
}

export const Sidebar: React.FC<SidebarProps> = ({
  currentTab,
  onSelectTab,
  selectedCategory,
  onSelectCategory,
  counts,
  aggregateSpeed,
  onOpenSettings,
}) => {
  const [speedHistory, setSpeedHistory] = useState<number[]>([0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
  const [peakSpeed, setPeakSpeed] = useState(0);

  useEffect(() => {
    setSpeedHistory((prev) => {
      const next = [...prev.slice(1), aggregateSpeed];
      return next;
    });
    if (aggregateSpeed > peakSpeed) {
      setPeakSpeed(aggregateSpeed);
    }
  }, [aggregateSpeed]);

  const handleExport = async () => {
    try {
      const json = await api.exportDownloads();
      const blob = new Blob([json], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `nexdrop_downloads_${new Date().toISOString().slice(0, 10)}.json`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (err) {
      alert(`Export failed: ${err}`);
    }
  };

  const mainNav = [
    { id: 'all', label: 'All Downloads', icon: Inbox, count: counts.all },
    {
      id: 'downloading',
      label: 'Downloading',
      icon: ArrowDownCircle,
      count: counts.downloading,
      activeColor: 'text-indigo-400',
    },
    { id: 'completed', label: 'Completed', icon: CheckCircle2, count: counts.completed },
    { id: 'paused', label: 'Paused', icon: PauseCircle, count: counts.paused },
    { id: 'scheduled', label: 'Scheduled', icon: Clock, count: counts.scheduled },
    {
      id: 'failed',
      label: 'Failed',
      icon: AlertCircle,
      count: counts.failed,
      badgeColor: counts.failed > 0 ? 'bg-rose-500/20 text-rose-300' : undefined,
    },
    {
      id: 'analytics',
      label: 'Analytics & Stats',
      icon: Activity,
      count: undefined,
      activeColor: 'text-cyan-400',
    },
  ];

  const categories: { id: FileCategory; label: string; icon: React.ElementType }[] = [
    { id: 'all', label: 'All Files', icon: File },
    { id: 'compressed', label: 'Compressed', icon: FileArchive },
    { id: 'documents', label: 'Documents', icon: FileText },
    { id: 'music', label: 'Music', icon: Music },
    { id: 'videos', label: 'Videos', icon: Film },
    { id: 'programs', label: 'Programs', icon: AppWindow },
    { id: 'other', label: 'Other', icon: File },
  ];

  // SVG Sparkline calculation
  const maxHistory = Math.max(...speedHistory, 1024 * 1024);
  const sparklinePoints = speedHistory
    .map((val, idx) => {
      const x = (idx / (speedHistory.length - 1)) * 180;
      const y = 30 - (val / maxHistory) * 26;
      return `${x},${y}`;
    })
    .join(' ');

  return (
    <aside className="w-64 h-screen flex flex-col bg-slate-950/90 border-r border-white/5 select-none shrink-0">
      {/* Brand Header */}
      <div className="h-16 px-5 flex items-center justify-between border-b border-white/5">
        <div className="flex items-center gap-2.5">
          <div className="flex h-8 w-8 items-center justify-center rounded-xl bg-gradient-to-tr from-indigo-600 via-indigo-500 to-purple-500 shadow-md shadow-indigo-500/25">
            <Zap size={18} className="text-white fill-white" />
          </div>
          <div>
            <span className="font-bold text-base tracking-tight text-white flex items-center gap-1.5">
              Nexdrop
              <span className="text-[10px] font-mono font-medium px-1.5 py-0.5 rounded bg-slate-800 text-slate-400 border border-white/5">
                v0.1.0
              </span>
            </span>
          </div>
        </div>
      </div>

      {/* Navigation Links */}
      <div className="flex-1 overflow-y-auto px-3 py-4 space-y-6">
        <div>
          <div className="px-3 mb-2 text-[11px] font-semibold tracking-wider text-slate-500 uppercase">
            Transfers
          </div>
          <nav className="space-y-1">
            {mainNav.map((item) => {
              const Icon = item.icon;
              const isActive = currentTab === item.id;
              return (
                <button
                  key={item.id}
                  onClick={() => onSelectTab(item.id)}
                  className={`w-full flex items-center justify-between px-3 py-2 rounded-xl text-xs font-medium transition-all duration-150 ${
                    isActive
                      ? 'bg-indigo-600 text-white shadow-sm shadow-indigo-600/30'
                      : 'text-slate-400 hover:text-slate-200 hover:bg-slate-900/60'
                  }`}
                >
                  <div className="flex items-center gap-2.5">
                    <Icon
                      size={16}
                      className={isActive ? 'text-white' : item.activeColor || 'text-slate-400'}
                    />
                    <span>{item.label}</span>
                  </div>
                  {typeof item.count === 'number' && item.count > 0 && (
                    <span
                      className={`text-[11px] font-mono px-2 py-0.5 rounded-full ${
                        isActive
                          ? 'bg-white/20 text-white'
                          : item.badgeColor || 'bg-slate-800 text-slate-400'
                      }`}
                    >
                      {item.count}
                    </span>
                  )}
                </button>
              );
            })}
          </nav>
        </div>

        {/* Categories */}
        <div>
          <div className="px-3 mb-2 text-[11px] font-semibold tracking-wider text-slate-500 uppercase">
            Categories
          </div>
          <nav className="space-y-1">
            {categories.map((cat) => {
              const Icon = cat.icon;
              const isCatActive = selectedCategory === cat.id;
              return (
                <button
                  key={cat.id}
                  onClick={() => onSelectCategory(cat.id)}
                  className={`w-full flex items-center gap-2.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-colors ${
                    isCatActive
                      ? 'bg-slate-800 text-indigo-300 font-semibold'
                      : 'text-slate-400 hover:text-slate-200 hover:bg-slate-900/40'
                  }`}
                >
                  <Icon size={15} className={isCatActive ? 'text-indigo-400' : 'text-slate-500'} />
                  <span>{cat.label}</span>
                </button>
              );
            })}
          </nav>
        </div>
      </div>

      {/* Aggregate Bandwidth Gauge & Settings */}
      <div className="p-3 border-t border-white/5 space-y-2">
        {/* Live bandwidth monitor card with sparkline */}
        <div className="p-3 rounded-2xl bg-indigo-950/20 border border-indigo-500/20 space-y-2">
          <div className="flex items-center justify-between text-xs">
            <div className="flex items-center gap-1.5 text-slate-300">
              <Activity size={14} className={`text-indigo-400 ${aggregateSpeed > 0 ? 'animate-pulse' : ''}`} />
              <span className="font-medium text-[11px]">Speed Monitor</span>
            </div>
            <span className="font-mono text-xs font-bold text-indigo-300">
              {formatSpeed(aggregateSpeed)}
            </span>
          </div>

          {/* SVG Waveform Sparkline */}
          <div className="h-8 w-full bg-slate-950/60 rounded-lg p-1 overflow-hidden border border-white/5">
            <svg className="w-full h-full overflow-visible" viewBox="0 0 180 32">
              <polyline
                fill="none"
                stroke="rgba(99, 102, 241, 0.8)"
                strokeWidth="1.5"
                points={sparklinePoints}
              />
            </svg>
          </div>

          <div className="flex justify-between text-[10px] text-slate-500 font-mono">
            <span>Peak: {formatSpeed(peakSpeed)}</span>
            <span>Active: {counts.downloading}</span>
          </div>
        </div>

        <div className="flex gap-1.5 pt-1">
          <button
            onClick={onOpenSettings}
            className="flex-1 flex items-center justify-center gap-1.5 px-3 py-2 rounded-xl text-xs font-medium text-slate-400 hover:text-slate-100 hover:bg-slate-900/80 transition-colors border border-white/5"
            title="Settings (Ctrl+,)"
          >
            <SettingsIcon size={14} />
            <span>Settings</span>
          </button>
          <button
            onClick={handleExport}
            className="p-2 rounded-xl text-slate-400 hover:text-slate-100 hover:bg-slate-900/80 transition-colors border border-white/5"
            title="Export Downloads as JSON"
          >
            <Download size={14} />
          </button>
        </div>
      </div>
    </aside>
  );
};
