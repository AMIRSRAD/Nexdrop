import React, { useState } from 'react';
import { motion, AnimatePresence } from 'motion/react';
import {
  X,
  Globe,
  Loader2,
  Check,
  Search,
  Filter,
  Download,
  Folder,
  Layers,
  FileCode,
  Compass,
  ExternalLink,
  Sparkles,
} from 'lucide-react';
import { GrabbedLink, AddDownloadRequest } from '../../types/download';
import { api } from '../../services/api';
import { FileIcon } from '../downloads/FileIcon';

interface LinkGrabberModalProps {
  isOpen: boolean;
  onClose: () => void;
  onBatchAdd: (reqs: AddDownloadRequest[]) => Promise<void>;
  defaultDestination: string;
}

export const LinkGrabberModal: React.FC<LinkGrabberModalProps> = ({
  isOpen,
  onClose,
  onBatchAdd,
  defaultDestination,
}) => {
  const [activeTab, setActiveTab] = useState<'grabber' | 'extension'>('grabber');
  const [pageUrl, setPageUrl] = useState('');
  const [isScanning, setIsScanning] = useState(false);
  const [links, setLinks] = useState<GrabbedLink[]>([]);
  const [selectedUrls, setSelectedUrls] = useState<Set<string>>(new Set());
  const [categoryFilter, setCategoryFilter] = useState<string>('all');
  const [searchFilter, setSearchFilter] = useState('');
  const [destination, setDestination] = useState(defaultDestination);
  const [scanError, setScanError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);

  const handleScan = async (e?: React.FormEvent) => {
    if (e) e.preventDefault();
    if (!pageUrl.trim() || isScanning) return;

    setIsScanning(true);
    setScanError(null);
    setLinks([]);
    setSelectedUrls(new Set());

    try {
      let target = pageUrl.trim();
      if (!target.startsWith('http://') && !target.startsWith('https://')) {
        target = `https://${target}`;
        setPageUrl(target);
      }

      const results = await api.grabPageLinks(target);
      setLinks(results);
      // Select all by default
      setSelectedUrls(new Set(results.map((l) => l.url)));
      if (results.length === 0) {
        setScanError('No downloadable files or media streams detected on this page.');
      }
    } catch (err: any) {
      setScanError(err?.toString() || 'Failed to scan page for links');
    } finally {
      setIsScanning(false);
    }
  };

  const filteredLinks = links.filter((l) => {
    const matchesCategory =
      categoryFilter === 'all' || l.category.toLowerCase() === categoryFilter.toLowerCase();
    const matchesSearch =
      !searchFilter.trim() ||
      l.filename.toLowerCase().includes(searchFilter.toLowerCase()) ||
      l.url.toLowerCase().includes(searchFilter.toLowerCase());
    return matchesCategory && matchesSearch;
  });

  const toggleSelectAll = () => {
    if (selectedUrls.size === filteredLinks.length) {
      setSelectedUrls(new Set());
    } else {
      setSelectedUrls(new Set(filteredLinks.map((l) => l.url)));
    }
  };

  const toggleSelectUrl = (url: string) => {
    const next = new Set(selectedUrls);
    if (next.has(url)) {
      next.delete(url);
    } else {
      next.add(url);
    }
    setSelectedUrls(next);
  };

  const handleDownloadSelected = async () => {
    if (selectedUrls.size === 0 || isSubmitting) return;

    setIsSubmitting(true);
    try {
      const selectedLinks = links.filter((l) => selectedUrls.has(l.url));
      const reqs: AddDownloadRequest[] = selectedLinks.map((l) => ({
        url: l.url,
        filename: l.filename,
        destination_path: destination || undefined,
        connections: 8,
      }));

      await onBatchAdd(reqs);
      onClose();
    } catch (err: any) {
      alert(`Failed to add downloads: ${err}`);
    } finally {
      setIsSubmitting(false);
    }
  };

  if (!isOpen) return null;

  return (
    <AnimatePresence>
      <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/80 backdrop-blur-md">
        <motion.div
          initial={{ opacity: 0, scale: 0.96, y: 10 }}
          animate={{ opacity: 1, scale: 1, y: 0 }}
          exit={{ opacity: 0, scale: 0.96, y: 10 }}
          className="w-full max-w-2xl rounded-3xl bg-slate-900 border border-white/10 shadow-2xl flex flex-col max-h-[85vh] overflow-hidden"
        >
          {/* Header */}
          <div className="flex items-center justify-between px-6 py-4 border-b border-white/5 bg-slate-950/40">
            <div className="flex items-center gap-2.5">
              <div className="flex h-8 w-8 items-center justify-center rounded-xl bg-indigo-500/20 text-indigo-400">
                <Globe size={18} />
              </div>
              <div>
                <h2 className="text-sm font-semibold text-white">Browser Link &amp; Media Grabber</h2>
                <p className="text-[11px] text-slate-400">
                  Extract and batch download files, video streams, and archives from any web page
                </p>
              </div>
            </div>
            <button
              onClick={onClose}
              className="p-1.5 rounded-lg text-slate-400 hover:text-white hover:bg-slate-800 transition-colors"
            >
              <X size={16} />
            </button>
          </div>

          {/* Mode Tabs */}
          <div className="flex border-b border-white/5 px-6 bg-slate-950/20">
            <button
              onClick={() => setActiveTab('grabber')}
              className={`flex items-center gap-2 py-3 px-3 text-xs font-medium border-b-2 transition-colors ${
                activeTab === 'grabber'
                  ? 'border-indigo-500 text-indigo-300 font-semibold'
                  : 'border-transparent text-slate-400 hover:text-slate-200'
              }`}
            >
              <Globe size={14} /> Page URL Grabber
            </button>
            <button
              onClick={() => setActiveTab('extension')}
              className={`flex items-center gap-2 py-3 px-3 text-xs font-medium border-b-2 transition-colors ${
                activeTab === 'extension'
                  ? 'border-indigo-500 text-indigo-300 font-semibold'
                  : 'border-transparent text-slate-400 hover:text-slate-200'
              }`}
            >
              <Compass size={14} /> Browser Extension Integration
            </button>
          </div>

          {activeTab === 'grabber' ? (
            <div className="p-6 flex flex-col flex-1 overflow-hidden space-y-4">
              {/* URL Scanner Form */}
              <form onSubmit={handleScan} className="flex gap-2">
                <div className="relative flex-1">
                  <Globe
                    size={15}
                    className="absolute left-3.5 top-1/2 -translate-y-1/2 text-slate-400 pointer-events-none"
                  />
                  <input
                    type="text"
                    value={pageUrl}
                    onChange={(e) => setPageUrl(e.target.value)}
                    placeholder="https://releases.ubuntu.com/jammy/ or any website URL..."
                    className="w-full bg-slate-950/80 border border-white/10 rounded-xl pl-10 pr-4 py-2.5 text-xs text-slate-100 placeholder:text-slate-500 focus:outline-none focus:border-indigo-500"
                  />
                </div>
                <button
                  type="submit"
                  disabled={isScanning || !pageUrl.trim()}
                  className="px-5 py-2.5 rounded-xl text-xs font-semibold bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white shadow-md shadow-indigo-600/30 flex items-center gap-1.5 transition-all"
                >
                  {isScanning ? (
                    <>
                      <Loader2 size={13} className="animate-spin" />
                      <span>Scanning...</span>
                    </>
                  ) : (
                    <>
                      <Sparkles size={13} />
                      <span>Grab Links</span>
                    </>
                  )}
                </button>
              </form>

              {scanError && (
                <div className="p-3 rounded-xl bg-amber-500/10 border border-amber-500/20 text-xs text-amber-300">
                  {scanError}
                </div>
              )}

              {/* Category Pills & Search */}
              {links.length > 0 && (
                <div className="space-y-3 flex-1 flex flex-col overflow-hidden">
                  <div className="flex items-center justify-between gap-3">
                    <div className="flex items-center gap-1.5 overflow-x-auto pb-1 text-xs">
                      {['all', 'compressed', 'video', 'music', 'documents', 'programs'].map((cat) => (
                        <button
                          key={cat}
                          type="button"
                          onClick={() => setCategoryFilter(cat)}
                          className={`px-2.5 py-1 rounded-lg capitalize transition-colors ${
                            categoryFilter === cat
                              ? 'bg-indigo-600/40 text-indigo-200 border border-indigo-500/40 font-medium'
                              : 'bg-slate-950/60 text-slate-400 hover:text-slate-200 border border-white/5'
                          }`}
                        >
                          {cat}
                        </button>
                      ))}
                    </div>

                    <div className="relative w-44">
                      <Search
                        size={12}
                        className="absolute left-2.5 top-1/2 -translate-y-1/2 text-slate-500"
                      />
                      <input
                        type="text"
                        value={searchFilter}
                        onChange={(e) => setSearchFilter(e.target.value)}
                        placeholder="Filter list..."
                        className="w-full bg-slate-950 border border-white/10 rounded-lg pl-7 pr-2.5 py-1 text-[11px] text-slate-200 placeholder:text-slate-600 focus:outline-none focus:border-indigo-500"
                      />
                    </div>
                  </div>

                  {/* Links List */}
                  <div className="flex-1 overflow-y-auto border border-white/5 rounded-2xl bg-slate-950/60 p-2 space-y-1">
                    <div className="flex items-center justify-between px-3 py-1.5 text-[11px] text-slate-400 border-b border-white/5 font-medium">
                      <label className="flex items-center gap-2 cursor-pointer">
                        <input
                          type="checkbox"
                          checked={
                            filteredLinks.length > 0 && selectedUrls.size === filteredLinks.length
                          }
                          onChange={toggleSelectAll}
                          className="rounded accent-indigo-500"
                        />
                        <span>Select All ({filteredLinks.length} files)</span>
                      </label>
                      <span>Selected: {selectedUrls.size}</span>
                    </div>

                    {filteredLinks.map((item) => {
                      const isSelected = selectedUrls.has(item.url);
                      return (
                        <div
                          key={item.url}
                          onClick={() => toggleSelectUrl(item.url)}
                          className={`flex items-center gap-3 p-2 rounded-xl text-xs cursor-pointer transition-colors ${
                            isSelected
                              ? 'bg-indigo-600/10 border border-indigo-500/20 text-slate-100'
                              : 'hover:bg-slate-900 border border-transparent text-slate-300'
                          }`}
                        >
                          <input
                            type="checkbox"
                            checked={isSelected}
                            onChange={() => toggleSelectUrl(item.url)}
                            className="rounded accent-indigo-500 shrink-0"
                          />
                          <FileIcon
                            category={item.category as any}
                            filename={item.filename}
                            size={18}
                          />
                          <div className="min-w-0 flex-1">
                            <div className="font-medium text-slate-200 truncate" title={item.filename}>
                              {item.filename}
                            </div>
                            <div className="text-[10px] text-slate-500 font-mono truncate" title={item.url}>
                              {item.url}
                            </div>
                          </div>
                          <span className="text-[10px] font-mono uppercase bg-slate-800 text-slate-400 px-1.5 py-0.5 rounded">
                            .{item.extension}
                          </span>
                        </div>
                      );
                    })}
                  </div>
                </div>
              )}

              {/* Destination & Action Footer */}
              <div className="pt-3 border-t border-white/5 flex items-center justify-between">
                <div className="flex items-center gap-2 text-xs text-slate-400 flex-1 max-w-sm">
                  <Folder size={14} className="text-slate-500 shrink-0" />
                  <input
                    type="text"
                    value={destination}
                    onChange={(e) => setDestination(e.target.value)}
                    placeholder="Destination Folder"
                    className="w-full bg-slate-950 border border-white/10 rounded-lg px-2.5 py-1 text-xs text-slate-200 focus:outline-none focus:border-indigo-500"
                  />
                </div>

                <div className="flex items-center gap-2">
                  <button
                    type="button"
                    onClick={onClose}
                    className="px-4 py-1.5 rounded-xl text-xs text-slate-400 hover:text-slate-200 hover:bg-slate-800 transition-colors"
                  >
                    Cancel
                  </button>
                  <button
                    type="button"
                    disabled={selectedUrls.size === 0 || isSubmitting}
                    onClick={handleDownloadSelected}
                    className="flex items-center gap-1.5 px-4 py-1.5 rounded-xl text-xs font-semibold bg-indigo-600 hover:bg-indigo-500 disabled:opacity-40 text-white shadow-md shadow-indigo-600/30 transition-all"
                  >
                    {isSubmitting ? (
                      <>
                        <Loader2 size={13} className="animate-spin" />
                        <span>Adding...</span>
                      </>
                    ) : (
                      <>
                        <Download size={13} />
                        <span>Download Selected ({selectedUrls.size})</span>
                      </>
                    )}
                  </button>
                </div>
              </div>
            </div>
          ) : (
            /* Browser Extension Guide Tab */
            <div className="p-6 space-y-4 text-xs overflow-y-auto">
              <div className="p-4 rounded-2xl bg-slate-950/60 border border-white/5 space-y-3">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2 text-sm font-semibold text-slate-200">
                    <Compass size={18} className="text-indigo-400" />
                    <span>Nexdrop Browser Extension</span>
                  </div>
                  <span className="flex items-center gap-1.5 text-[11px] font-medium text-emerald-400 bg-emerald-500/10 px-2.5 py-0.5 rounded-full border border-emerald-500/20">
                    <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse" />
                    Listening on 127.0.0.1:18888
                  </span>
                </div>

                <p className="text-slate-400 leading-relaxed text-[11px]">
                  Nexdrop includes a native browser extension compatible with Google Chrome, Microsoft Edge, Brave, Opera, and Firefox. It automatically captures browser downloads and overlays a video download panel on streaming sites.
                </p>

                <div className="space-y-2 pt-2 border-t border-white/5">
                  <div className="font-semibold text-slate-300">How to Install in 30 Seconds:</div>
                  <ol className="list-decimal list-inside space-y-1.5 text-slate-400 text-[11px]">
                    <li>
                      Open <code className="text-indigo-300 bg-slate-900 px-1.5 py-0.5 rounded">chrome://extensions</code> (or <code className="text-indigo-300 bg-slate-900 px-1.5 py-0.5 rounded">edge://extensions</code>) in your browser.
                    </li>
                    <li>
                      Enable <strong>"Developer mode"</strong> in the top-right corner.
                    </li>
                    <li>
                      Click <strong>"Load unpacked"</strong> and select the directory:
                      <div className="mt-1 p-2 bg-slate-950 rounded-lg font-mono text-[10px] text-slate-300 select-all border border-white/5">
                        d:\Projects\Nexdrop\extensions\nexdrop-browser-extension
                      </div>
                    </li>
                  </ol>
                </div>
              </div>

              <div className="p-4 rounded-2xl bg-indigo-500/10 border border-indigo-500/20 space-y-2">
                <div className="font-semibold text-indigo-300">Capabilities Enabled:</div>
                <ul className="list-disc list-inside space-y-1 text-slate-300 text-[11px]">
                  <li>Right-click any link or media &rarr; <strong>"Download with Nexdrop"</strong></li>
                  <li>Right-click page &rarr; <strong>"Grab All Links on Page with Nexdrop"</strong></li>
                  <li>Floating <strong>"Download with Nexdrop"</strong> panel above HTML5 video players</li>
                  <li>Automatic download redirection for `.zip`, `.iso`, `.exe`, `.mp4` and large files</li>
                </ul>
              </div>
            </div>
          )}
        </motion.div>
      </div>
    </AnimatePresence>
  );
};
