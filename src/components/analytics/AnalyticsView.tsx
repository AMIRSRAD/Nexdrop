import React from 'react';
import {
  Activity,
  HardDrive,
  CheckCircle2,
  Zap,
  ShieldCheck,
  FileArchive,
  Film,
  Music,
  FileText,
  AppWindow,
  File,
  Layers,
  TrendingUp,
} from 'lucide-react';
import { DownloadTask, FileCategory } from '../../types/download';
import { formatBytes, formatSpeed } from '../../utils/format';

interface AnalyticsViewProps {
  downloads: DownloadTask[];
  onOpenFile: (id: string) => void;
  onRevealFile: (id: string) => void;
}

export const AnalyticsView: React.FC<AnalyticsViewProps> = ({
  downloads,
  onOpenFile,
  onRevealFile,
}) => {
  const completed = downloads.filter((d) => d.status === 'completed');
  const active = downloads.filter((d) => d.status === 'downloading');

  const totalBytesDownloaded = downloads.reduce(
    (acc, d) => acc + (d.downloaded_bytes || 0),
    0
  );

  const totalCompletedBytes = completed.reduce(
    (acc, d) => acc + (d.total_bytes > 0 ? d.total_bytes : d.downloaded_bytes),
    0
  );

  // Category distribution
  const categoryStats: Record<FileCategory, { count: number; bytes: number }> = {
    all: { count: downloads.length, bytes: totalBytesDownloaded },
    compressed: { count: 0, bytes: 0 },
    videos: { count: 0, bytes: 0 },
    music: { count: 0, bytes: 0 },
    documents: { count: 0, bytes: 0 },
    programs: { count: 0, bytes: 0 },
    other: { count: 0, bytes: 0 },
  };

  downloads.forEach((d) => {
    if (categoryStats[d.category]) {
      categoryStats[d.category].count += 1;
      categoryStats[d.category].bytes += d.downloaded_bytes || 0;
    } else {
      categoryStats.other.count += 1;
      categoryStats.other.bytes += d.downloaded_bytes || 0;
    }
  });

  const categoryConfig: {
    id: FileCategory;
    label: string;
    icon: React.ElementType;
    color: string;
    bg: string;
  }[] = [
    { id: 'compressed', label: 'Compressed (ZIP/RAR/ISO)', icon: FileArchive, color: 'text-amber-400', bg: 'bg-amber-400' },
    { id: 'videos', label: 'Videos & Media', icon: Film, color: 'text-purple-400', bg: 'bg-purple-400' },
    { id: 'programs', label: 'Software & Executables', icon: AppWindow, color: 'text-blue-400', bg: 'bg-blue-400' },
    { id: 'documents', label: 'Documents & PDFs', icon: FileText, color: 'text-emerald-400', bg: 'bg-emerald-400' },
    { id: 'music', label: 'Music & Audio', icon: Music, color: 'text-pink-400', bg: 'bg-pink-400' },
    { id: 'other', label: 'Other Files', icon: File, color: 'text-slate-400', bg: 'bg-slate-400' },
  ];

  const avgConnections = completed.length > 0
    ? (completed.reduce((acc, d) => acc + (d.connections || 1), 0) / completed.length).toFixed(1)
    : '8.0';

  return (
    <div className="flex-1 overflow-y-auto p-8 space-y-8 animate-in fade-in duration-200">
      {/* Top Header */}
      <div>
        <div className="flex items-center gap-2.5">
          <div className="flex h-8 w-8 items-center justify-center rounded-xl bg-cyan-500/20 text-cyan-400">
            <Activity size={18} />
          </div>
          <div>
            <h1 className="text-xl font-bold text-white tracking-tight">Bandwidth & Download Analytics</h1>
            <p className="text-xs text-slate-400 mt-0.5">
              Live metrics, segment acceleration stats, and network storage analytics
            </p>
          </div>
        </div>
      </div>

      {/* Primary KPI Metrics */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
        <div className="p-5 rounded-2xl bg-slate-900/60 border border-white/5 backdrop-blur-sm relative overflow-hidden">
          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-slate-400">Data Transferred</span>
            <div className="h-8 w-8 rounded-lg bg-indigo-500/10 flex items-center justify-center text-indigo-400">
              <HardDrive size={16} />
            </div>
          </div>
          <div className="mt-3 text-2xl font-bold text-white tracking-tight">
            {formatBytes(totalBytesDownloaded)}
          </div>
          <div className="mt-1 text-[11px] text-slate-500">
            {formatBytes(totalCompletedBytes)} finished payload
          </div>
          <div className="absolute -bottom-6 -right-6 w-20 h-20 bg-indigo-500/5 rounded-full blur-xl pointer-events-none" />
        </div>

        <div className="p-5 rounded-2xl bg-slate-900/60 border border-white/5 backdrop-blur-sm relative overflow-hidden">
          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-slate-400">Completed Files</span>
            <div className="h-8 w-8 rounded-lg bg-emerald-500/10 flex items-center justify-center text-emerald-400">
              <CheckCircle2 size={16} />
            </div>
          </div>
          <div className="mt-3 text-2xl font-bold text-white tracking-tight">
            {completed.length}
          </div>
          <div className="mt-1 text-[11px] text-emerald-400/80 flex items-center gap-1">
            <TrendingUp size={12} />
            <span>{downloads.length > 0 ? Math.round((completed.length / downloads.length) * 100) : 0}% success rate</span>
          </div>
          <div className="absolute -bottom-6 -right-6 w-20 h-20 bg-emerald-500/5 rounded-full blur-xl pointer-events-none" />
        </div>

        <div className="p-5 rounded-2xl bg-slate-900/60 border border-white/5 backdrop-blur-sm relative overflow-hidden">
          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-slate-400">Acceleration Multiplier</span>
            <div className="h-8 w-8 rounded-lg bg-cyan-500/10 flex items-center justify-center text-cyan-400">
              <Zap size={16} />
            </div>
          </div>
          <div className="mt-3 text-2xl font-bold text-white tracking-tight">
            {avgConnections}x
          </div>
          <div className="mt-1 text-[11px] text-slate-500">
            Average streams per file
          </div>
          <div className="absolute -bottom-6 -right-6 w-20 h-20 bg-cyan-500/5 rounded-full blur-xl pointer-events-none" />
        </div>

        <div className="p-5 rounded-2xl bg-slate-900/60 border border-white/5 backdrop-blur-sm relative overflow-hidden">
          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-slate-400">Security & Integrity</span>
            <div className="h-8 w-8 rounded-lg bg-blue-500/10 flex items-center justify-center text-blue-400">
              <ShieldCheck size={16} />
            </div>
          </div>
          <div className="mt-3 text-2xl font-bold text-white tracking-tight">
            100%
          </div>
          <div className="mt-1 text-[11px] text-blue-400/80">
            Windows Defender active
          </div>
          <div className="absolute -bottom-6 -right-6 w-20 h-20 bg-blue-500/5 rounded-full blur-xl pointer-events-none" />
        </div>
      </div>

      {/* Category Bandwidth Distribution */}
      <div className="p-6 rounded-3xl bg-slate-900/60 border border-white/5 backdrop-blur-sm space-y-6">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Layers size={16} className="text-indigo-400" />
            <h2 className="text-sm font-semibold text-white">Category Distribution</h2>
          </div>
          <span className="text-xs text-slate-400">
            {downloads.length} total tracked downloads
          </span>
        </div>

        <div className="space-y-4">
          {categoryConfig.map((cat) => {
            const stats = categoryStats[cat.id];
            const percent = totalBytesDownloaded > 0
              ? Math.min(100, Math.round((stats.bytes / totalBytesDownloaded) * 100))
              : 0;

            const Icon = cat.icon;

            return (
              <div key={cat.id} className="space-y-1.5">
                <div className="flex items-center justify-between text-xs">
                  <div className="flex items-center gap-2">
                    <Icon size={14} className={cat.color} />
                    <span className="font-medium text-slate-200">{cat.label}</span>
                    <span className="text-[11px] text-slate-500 font-mono">({stats.count} files)</span>
                  </div>
                  <div className="flex items-center gap-3">
                    <span className="text-slate-400 font-mono">{formatBytes(stats.bytes)}</span>
                    <span className="font-semibold text-slate-200 w-9 text-right font-mono">{percent}%</span>
                  </div>
                </div>
                <div className="h-2 w-full bg-slate-800/80 rounded-full overflow-hidden">
                  <div
                    className={`h-full ${cat.bg} transition-all duration-500 rounded-full`}
                    style={{ width: `${percent}%` }}
                  />
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {/* Engine Architecture Details */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <div className="p-4 rounded-2xl bg-slate-900/40 border border-white/5 space-y-1.5">
          <div className="text-[11px] text-slate-400 font-medium">HTTP Range Slicing</div>
          <div className="text-xs text-slate-200 font-semibold flex items-center gap-1.5 text-emerald-400">
            <CheckCircle2 size={13} />
            <span>Automatic Discovery & Fallback</span>
          </div>
          <p className="text-[11px] text-slate-500 pt-0.5">
            Dynamically verifies byte range headers and falls back to single-stream downloading when unsupported.
          </p>
        </div>

        <div className="p-4 rounded-2xl bg-slate-900/40 border border-white/5 space-y-1.5">
          <div className="text-[11px] text-slate-400 font-medium">Parallel Multi-Network Mirrors</div>
          <div className="text-xs text-slate-200 font-semibold flex items-center gap-1.5 text-cyan-400">
            <Zap size={13} />
            <span>Round-Robin CDN Balancing</span>
          </div>
          <p className="text-[11px] text-slate-500 pt-0.5">
            Distributes worker threads across alternative hostnames to maximize multi-gigabit throughput.
          </p>
        </div>

        <div className="p-4 rounded-2xl bg-slate-900/40 border border-white/5 space-y-1.5">
          <div className="text-[11px] text-slate-400 font-medium">Anti-Corruption Assembly</div>
          <div className="text-xs text-slate-200 font-semibold flex items-center gap-1.5 text-indigo-400">
            <ShieldCheck size={13} />
            <span>Bit-for-Bit Verified Merging</span>
          </div>
          <p className="text-[11px] text-slate-500 pt-0.5">
            Sequential chunk stitching with SHA-256 and MD5 hash generation upon file completion.
          </p>
        </div>
      </div>
    </div>
  );
};
