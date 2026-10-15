import React from 'react';
import {
  Search,
  Plus,
  LayoutGrid,
  List,
  Layers,
  ArrowUpDown,
  Play,
  Pause,
  Globe,
  Radio,
} from 'lucide-react';

interface HeaderProps {
  searchQuery: string;
  onSearchChange: (q: string) => void;
  viewMode: 'grid' | 'list';
  onViewModeChange: (mode: 'grid' | 'list') => void;
  sortBy: 'date' | 'size' | 'name' | 'speed';
  onSortByChange: (sort: 'date' | 'size' | 'name' | 'speed') => void;
  onOpenNewDownload: () => void;
  onOpenBatchDownload: () => void;
  onOpenLinkGrabber: () => void;
  onPauseAll: () => void;
  onResumeAll: () => void;
  hasActiveDownloads: boolean;
  hasPausedDownloads: boolean;
}

export const Header: React.FC<HeaderProps> = ({
  searchQuery,
  onSearchChange,
  viewMode,
  onViewModeChange,
  sortBy,
  onSortByChange,
  onOpenNewDownload,
  onOpenBatchDownload,
  onOpenLinkGrabber,
  onPauseAll,
  onResumeAll,
  hasActiveDownloads,
  hasPausedDownloads,
}) => {
  return (
    <header className="h-16 px-6 flex items-center justify-between border-b border-white/5 bg-slate-950/60 backdrop-blur-md sticky top-0 z-20">
      {/* Search Bar */}
      <div className="relative w-80">
        <Search
          size={16}
          className="absolute left-3 top-1/2 -translate-y-1/2 text-slate-400 pointer-events-none"
        />
        <input
          type="text"
          value={searchQuery}
          onChange={(e) => onSearchChange(e.target.value)}
          placeholder="Search downloads or URLs..."
          className="w-full bg-slate-900/80 border border-white/10 rounded-xl pl-9 pr-4 py-1.5 text-xs text-slate-200 placeholder:text-slate-500 focus:outline-none focus:border-indigo-500/60 focus:ring-1 focus:ring-indigo-500/30 transition-all"
        />
      </div>

      {/* Center/Right Toolbar */}
      <div className="flex items-center gap-3">
        {/* Pause/Resume All */}
        {hasActiveDownloads && (
          <button
            onClick={onPauseAll}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-medium bg-slate-800/80 hover:bg-slate-800 text-slate-300 border border-white/5 transition-colors"
            title="Pause all active transfers"
          >
            <Pause size={13} />
            <span>Pause All</span>
          </button>
        )}

        {hasPausedDownloads && (
          <button
            onClick={onResumeAll}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-medium bg-slate-800/80 hover:bg-slate-800 text-emerald-300 border border-white/5 transition-colors"
            title="Resume all transfers"
          >
            <Play size={13} />
            <span>Resume All</span>
          </button>
        )}

        {/* Sort Selector */}
        <div className="flex items-center gap-1.5 bg-slate-900/80 px-2.5 py-1 rounded-xl border border-white/5 text-xs text-slate-400">
          <ArrowUpDown size={13} className="text-slate-500" />
          <select
            value={sortBy}
            onChange={(e) => onSortByChange(e.target.value as any)}
            className="bg-transparent text-slate-300 text-xs focus:outline-none cursor-pointer"
          >
            <option value="date" className="bg-slate-900 text-slate-200">Date Added</option>
            <option value="size" className="bg-slate-900 text-slate-200">File Size</option>
            <option value="name" className="bg-slate-900 text-slate-200">Name</option>
            <option value="speed" className="bg-slate-900 text-slate-200">Speed</option>
          </select>
        </div>

        {/* View Mode Switcher */}
        <div className="flex items-center bg-slate-900/80 p-0.5 rounded-xl border border-white/5">
          <button
            onClick={() => onViewModeChange('grid')}
            className={`p-1.5 rounded-lg transition-colors ${
              viewMode === 'grid'
                ? 'bg-indigo-600 text-white'
                : 'text-slate-400 hover:text-slate-200'
            }`}
            title="Card View"
          >
            <LayoutGrid size={15} />
          </button>
          <button
            onClick={() => onViewModeChange('list')}
            className={`p-1.5 rounded-lg transition-colors ${
              viewMode === 'list'
                ? 'bg-indigo-600 text-white'
                : 'text-slate-400 hover:text-slate-200'
            }`}
            title="Compact List View"
          >
            <List size={15} />
          </button>
        </div>

        {/* Link Grabber / Browser Integration */}
        <button
          onClick={onOpenLinkGrabber}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-medium text-slate-300 hover:text-white bg-slate-900/80 hover:bg-slate-800 border border-white/5 hover:border-white/10 transition-all"
          title="Scan any webpage for video streams, archives, and files"
        >
          <Globe size={13} className="text-indigo-400" />
          <span>Link Grabber</span>
        </button>

        {/* Batch URLs Button */}
        <button
          onClick={onOpenBatchDownload}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-medium text-slate-300 hover:text-white bg-slate-900/80 hover:bg-slate-800 border border-white/5 hover:border-white/10 transition-all"
          title="Batch add multiple URLs or pattern sequences"
        >
          <Layers size={13} className="text-slate-400" />
          <span>Batch</span>
        </button>

        {/* New Download Action Button */}
        <button
          onClick={onOpenNewDownload}
          className="flex items-center gap-1.5 px-4 py-1.5 rounded-xl text-xs font-semibold bg-indigo-600 hover:bg-indigo-500 text-white shadow-sm shadow-indigo-600/30 transition-all active:scale-[0.98]"
        >
          <Plus size={14} strokeWidth={2.5} />
          <span>New Download</span>
        </button>
      </div>
    </header>
  );
};
