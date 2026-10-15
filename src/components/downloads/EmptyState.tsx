import React, { useState } from 'react';
import { ArrowDownToLine, Plus, Link2, Sparkles } from 'lucide-react';

interface EmptyStateProps {
  onOpenNewDownload: (initialUrl?: string) => void;
  tabName: string;
}

export const EmptyState: React.FC<EmptyStateProps> = ({ onOpenNewDownload, tabName }) => {
  const [isDragOver, setIsDragOver] = useState(false);

  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    setIsDragOver(true);
  };

  const handleDragLeave = () => {
    setIsDragOver(false);
  };

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setIsDragOver(false);

    // Check if a URL text was dragged and dropped
    const text = e.dataTransfer.getData('text');
    if (text && (text.startsWith('http://') || text.startsWith('https://'))) {
      onOpenNewDownload(text.trim());
    }
  };

  return (
    <div
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
      className={`my-8 flex flex-col items-center justify-center rounded-3xl border-2 border-dashed p-12 text-center transition-all duration-200 ${
        isDragOver
          ? 'border-indigo-500 bg-indigo-500/10 scale-[1.01]'
          : 'border-white/10 bg-slate-900/20 hover:border-white/20 hover:bg-slate-900/40'
      }`}
    >
      <div className="relative mb-5 flex h-16 w-16 items-center justify-center rounded-2xl bg-indigo-500/10 border border-indigo-500/20 text-indigo-400 shadow-inner">
        <ArrowDownToLine size={28} className={isDragOver ? 'animate-bounce' : ''} />
        <span className="absolute -top-1 -right-1 flex h-4 w-4 items-center justify-center rounded-full bg-indigo-500 text-[10px] text-white">
          <Sparkles size={10} />
        </span>
      </div>

      <h3 className="text-base font-semibold text-slate-100">
        {isDragOver ? 'Drop URL to start download' : `No ${tabName} downloads`}
      </h3>
      <p className="mt-1.5 max-w-sm text-xs text-slate-400">
        Drag and drop a download link here, or paste any URL to begin high-speed multi-threaded downloading.
      </p>

      <div className="mt-6 flex items-center gap-3">
        <button
          onClick={() => onOpenNewDownload()}
          className="flex items-center gap-2 rounded-xl bg-indigo-600 px-4 py-2 text-xs font-semibold text-white shadow-md shadow-indigo-600/30 hover:bg-indigo-500 transition-all active:scale-95"
        >
          <Plus size={15} />
          <span>Add Download</span>
        </button>

        <button
          onClick={async () => {
            try {
              const clipText = await navigator.clipboard.readText();
              if (clipText && (clipText.startsWith('http://') || clipText.startsWith('https://'))) {
                onOpenNewDownload(clipText.trim());
              } else {
                onOpenNewDownload();
              }
            } catch {
              onOpenNewDownload();
            }
          }}
          className="flex items-center gap-2 rounded-xl bg-slate-800/80 px-4 py-2 text-xs font-medium text-slate-300 border border-white/5 hover:bg-slate-800 hover:text-white transition-all"
        >
          <Link2 size={15} />
          <span>Paste from Clipboard</span>
        </button>
      </div>
    </div>
  );
};
