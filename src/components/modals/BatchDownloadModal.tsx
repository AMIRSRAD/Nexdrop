import React, { useState, useRef } from 'react';
import { motion, AnimatePresence } from 'motion/react';
import { X, Layers, Folder, Loader2, Check, Sparkles, ListFilter, FileUp } from 'lucide-react';
import { AddDownloadRequest } from '../../types/download';

interface BatchDownloadModalProps {
  isOpen: boolean;
  onClose: () => void;
  onBatchAdd: (reqs: AddDownloadRequest[]) => Promise<void>;
  defaultDestination: string;
}

export const BatchDownloadModal: React.FC<BatchDownloadModalProps> = ({
  isOpen,
  onClose,
  onBatchAdd,
  defaultDestination,
}) => {
  const [activeTab, setActiveTab] = useState<'list' | 'pattern'>('list');
  const [text, setText] = useState('');
  const [destination, setDestination] = useState(defaultDestination);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Pattern Generator state
  const [patternUrl, setPatternUrl] = useState('https://example.com/files/chapter_[01-10].pdf');
  const [rangeStart, setRangeStart] = useState(1);
  const [rangeEnd, setRangeEnd] = useState(10);
  const [wildcardDigits, setWildcardDigits] = useState(2);

  const handleFileSelect = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = (event) => {
      const content = event.target?.result as string;
      if (content) {
        setText((prev) => (prev.trim() ? `${prev.trim()}\n${content}` : content));
      }
    };
    reader.readAsText(file);
    e.target.value = '';
  };

  // Extract all http/https URLs from raw text
  const detectedUrls = Array.from(
    new Set(
      text
        .split('\n')
        .map((line) => line.trim())
        .filter((line) => line.startsWith('http://') || line.startsWith('https://'))
    )
  );

  const generatePatternUrls = () => {
    const urls: string[] = [];
    const from = Math.min(rangeStart, rangeEnd);
    const to = Math.max(rangeStart, rangeEnd);

    for (let i = from; i <= to; i++) {
      const numStr = String(i).padStart(wildcardDigits, '0');
      // replace [01-10] or [*] with number
      let formatted = patternUrl;
      if (formatted.includes('[') && formatted.includes(']')) {
        formatted = formatted.replace(/\[.*?\]/, numStr);
      } else {
        formatted = `${formatted}${numStr}`;
      }
      urls.push(formatted);
    }

    setText((prev) => {
      const existing = prev.trim();
      return existing ? `${existing}\n${urls.join('\n')}` : urls.join('\n');
    });
    setActiveTab('list');
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (detectedUrls.length === 0 || isSubmitting) return;

    setIsSubmitting(true);
    try {
      const reqs: AddDownloadRequest[] = detectedUrls.map((url) => ({
        url,
        destination_path: destination || undefined,
        connections: 8,
      }));

      await onBatchAdd(reqs);
      setText('');
      onClose();
    } catch (err) {
      console.error(err);
    } finally {
      setIsSubmitting(false);
    }
  };

  if (!isOpen) return null;

  return (
    <AnimatePresence>
      <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/75 backdrop-blur-md">
        <motion.div
          initial={{ opacity: 0, scale: 0.95, y: 10 }}
          animate={{ opacity: 1, scale: 1, y: 0 }}
          exit={{ opacity: 0, scale: 0.95, y: 10 }}
          className="w-full max-w-lg rounded-3xl bg-slate-900 border border-white/10 shadow-2xl overflow-hidden"
        >
          {/* Header */}
          <div className="flex items-center justify-between px-6 py-4 border-b border-white/5">
            <div className="flex items-center gap-2">
              <div className="flex h-7 w-7 items-center justify-center rounded-lg bg-indigo-500/20 text-indigo-400">
                <Layers size={16} />
              </div>
              <h2 className="text-sm font-semibold text-white">Batch Add Downloads</h2>
            </div>
            <button
              onClick={onClose}
              className="p-1 rounded-lg text-slate-400 hover:text-white hover:bg-slate-800 transition-colors"
            >
              <X size={16} />
            </button>
          </div>

          {/* Mode Switcher */}
          <div className="flex border-b border-white/5 px-6 bg-slate-950/40">
            <button
              type="button"
              onClick={() => setActiveTab('list')}
              className={`flex items-center gap-2 py-3 px-3 text-xs font-medium border-b-2 transition-colors ${
                activeTab === 'list'
                  ? 'border-indigo-500 text-indigo-300'
                  : 'border-transparent text-slate-400 hover:text-slate-200'
              }`}
            >
              <ListFilter className="w-3.5 h-3.5" /> URL List
            </button>
            <button
              type="button"
              onClick={() => setActiveTab('pattern')}
              className={`flex items-center gap-2 py-3 px-3 text-xs font-medium border-b-2 transition-colors ${
                activeTab === 'pattern'
                  ? 'border-indigo-500 text-indigo-300'
                  : 'border-transparent text-slate-400 hover:text-slate-200'
              }`}
            >
              <Sparkles className="w-3.5 h-3.5" /> Pattern Sequence
            </button>
          </div>

          <form onSubmit={handleSubmit} className="p-6 space-y-4">
            {activeTab === 'list' ? (
              <div>
                <div className="flex items-center justify-between mb-1.5">
                  <label className="text-xs font-medium text-slate-300">
                    Paste Multiple URLs (one per line)
                  </label>
                  <div className="flex items-center gap-3">
                    <input
                      type="file"
                      ref={fileInputRef}
                      onChange={handleFileSelect}
                      accept=".txt,.csv,.text"
                      className="hidden"
                    />
                    <button
                      type="button"
                      onClick={() => fileInputRef.current?.click()}
                      className="flex items-center gap-1 text-[11px] font-medium text-indigo-400 hover:text-indigo-300 transition-colors"
                    >
                      <FileUp size={12} />
                      <span>Import File (.txt, .csv)</span>
                    </button>
                    {detectedUrls.length > 0 && (
                      <span className="text-xs text-indigo-400 font-mono font-medium">
                        {detectedUrls.length} valid URL{detectedUrls.length > 1 ? 's' : ''} detected
                      </span>
                    )}
                  </div>
                </div>
                <textarea
                  rows={6}
                  value={text}
                  onChange={(e) => setText(e.target.value)}
                  placeholder="https://example.com/file1.zip&#10;https://example.com/file2.iso&#10;https://example.com/file3.mp4"
                  className="w-full bg-slate-950/80 border border-white/10 rounded-xl p-3 text-xs text-slate-100 placeholder:text-slate-500 font-mono focus:outline-none focus:border-indigo-500"
                />
              </div>
            ) : (
              <div className="space-y-3">
                <div>
                  <label className="block text-xs font-medium text-slate-300 mb-1">
                    URL Template (use brackets like <code className="text-indigo-400">[01-10]</code>)
                  </label>
                  <input
                    type="text"
                    value={patternUrl}
                    onChange={(e) => setPatternUrl(e.target.value)}
                    placeholder="https://example.com/video_[01-20].mp4"
                    className="w-full bg-slate-950/80 border border-white/10 rounded-xl px-3 py-2 text-xs text-slate-100 placeholder:text-slate-500 font-mono focus:outline-none focus:border-indigo-500"
                  />
                </div>

                <div className="grid grid-cols-3 gap-3">
                  <div>
                    <label className="block text-xs font-medium text-slate-400 mb-1">From</label>
                    <input
                      type="number"
                      min={0}
                      max={9999}
                      value={rangeStart}
                      onChange={(e) => setRangeStart(parseInt(e.target.value) || 0)}
                      className="w-full bg-slate-950/80 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-100 font-mono focus:outline-none focus:border-indigo-500"
                    />
                  </div>
                  <div>
                    <label className="block text-xs font-medium text-slate-400 mb-1">To</label>
                    <input
                      type="number"
                      min={0}
                      max={9999}
                      value={rangeEnd}
                      onChange={(e) => setRangeEnd(parseInt(e.target.value) || 0)}
                      className="w-full bg-slate-950/80 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-100 font-mono focus:outline-none focus:border-indigo-500"
                    />
                  </div>
                  <div>
                    <label className="block text-xs font-medium text-slate-400 mb-1">Zero Pad Digits</label>
                    <input
                      type="number"
                      min={1}
                      max={6}
                      value={wildcardDigits}
                      onChange={(e) => setWildcardDigits(parseInt(e.target.value) || 1)}
                      className="w-full bg-slate-950/80 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-100 font-mono focus:outline-none focus:border-indigo-500"
                    />
                  </div>
                </div>

                <button
                  type="button"
                  onClick={generatePatternUrls}
                  className="w-full py-2 bg-indigo-600/30 hover:bg-indigo-600/50 text-indigo-300 font-medium rounded-xl border border-indigo-500/30 text-xs transition-colors"
                >
                  Generate {Math.abs(rangeEnd - rangeStart) + 1} URLs and Add to List
                </button>
              </div>
            )}

            <div>
              <label className="block text-xs font-medium text-slate-300 mb-1.5">
                Destination Folder
              </label>
              <div className="relative">
                <Folder
                  size={15}
                  className="absolute left-3.5 top-1/2 -translate-y-1/2 text-slate-400 pointer-events-none"
                />
                <input
                  type="text"
                  value={destination}
                  onChange={(e) => setDestination(e.target.value)}
                  placeholder="Downloads"
                  className="w-full bg-slate-950/80 border border-white/10 rounded-xl pl-10 pr-4 py-2 text-xs text-slate-100 placeholder:text-slate-500 focus:outline-none focus:border-indigo-500"
                />
              </div>
            </div>

            {/* Footer */}
            <div className="flex items-center justify-end gap-3 pt-3 border-t border-white/5">
              <button
                type="button"
                onClick={onClose}
                className="px-4 py-2 rounded-xl text-xs font-medium text-slate-400 hover:text-slate-200 hover:bg-slate-800 transition-colors"
              >
                Cancel
              </button>
              <button
                type="submit"
                disabled={detectedUrls.length === 0 || isSubmitting}
                className="flex items-center gap-2 px-5 py-2 rounded-xl text-xs font-semibold bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white shadow-md shadow-indigo-600/30 transition-all active:scale-95"
              >
                {isSubmitting ? (
                  <>
                    <Loader2 size={13} className="animate-spin" />
                    <span>Queuing...</span>
                  </>
                ) : (
                  <>
                    <Check size={14} />
                    <span>Start {detectedUrls.length} Downloads</span>
                  </>
                )}
              </button>
            </div>
          </form>
        </motion.div>
      </div>
    </AnimatePresence>
  );
};
