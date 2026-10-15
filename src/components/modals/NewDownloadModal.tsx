import React, { useState, useEffect, useRef } from 'react';
import { motion, AnimatePresence } from 'motion/react';
import {
  X,
  Link,
  Folder,
  Sliders,
  Zap,
  Loader2,
  CheckCircle2,
  AlertTriangle,
  Clock,
  Sparkles,
  Terminal,
} from 'lucide-react';
import { api } from '../../services/api';
import { AddDownloadRequest, UrlProbeResult } from '../../types/download';
import { formatBytes } from '../../utils/format';
import { FileIcon } from '../downloads/FileIcon';

interface NewDownloadModalProps {
  isOpen: boolean;
  onClose: () => void;
  onAdd: (req: AddDownloadRequest) => Promise<void>;
  initialUrl?: string;
  defaultDestination: string;
}

export const NewDownloadModal: React.FC<NewDownloadModalProps> = ({
  isOpen,
  onClose,
  onAdd,
  initialUrl = '',
  defaultDestination,
}) => {
  const [url, setUrl] = useState(initialUrl);
  const [filename, setFilename] = useState('');
  const [destination, setDestination] = useState(defaultDestination);
  const [connections, setConnections] = useState(8);
  const [priority, setPriority] = useState(1);
  const [speedLimit, setSpeedLimit] = useState(0);
  const [isScheduled, setIsScheduled] = useState(false);
  const [scheduledDate, setScheduledDate] = useState('');
  const [expectedChecksum, setExpectedChecksum] = useState('');
  const [userAgent, setUserAgent] = useState('');
  const [referer, setReferer] = useState('');
  const [mirrorsText, setMirrorsText] = useState('');
  const [minChunkSize, setMinChunkSize] = useState<number>(1048576);
  const [proxyMode, setProxyMode] = useState<'system' | 'direct' | 'custom'>('system');
  const [customProxyUrl, setCustomProxyUrl] = useState('');
  const [showCurlModal, setShowCurlModal] = useState(false);
  const [curlCommand, setCurlCommand] = useState('');

  const [showAdvanced, setShowAdvanced] = useState(false);
  const [isProbing, setIsProbing] = useState(false);
  const [probeResult, setProbeResult] = useState<UrlProbeResult | null>(null);
  const [probeError, setProbeError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);

  const urlInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (isOpen) {
      setUrl(initialUrl);
      setFilename('');
      setDestination(defaultDestination);
      setProbeResult(null);
      setProbeError(null);
      setIsSubmitting(false);
      setMirrorsText('');
      setMinChunkSize(1048576);
      setProxyMode('system');
      setCustomProxyUrl('');
      setShowCurlModal(false);
      setCurlCommand('');

      if (initialUrl && initialUrl.startsWith('http')) {
        handleProbe(initialUrl);
      } else {
        // Attempt clipboard check
        navigator.clipboard?.readText().then((clip) => {
          if (clip && (clip.startsWith('http://') || clip.startsWith('https://')) && !url) {
            setUrl(clip.trim());
            handleProbe(clip.trim());
          }
        }).catch(() => {});
      }

      setTimeout(() => urlInputRef.current?.focus(), 50);
    }
  }, [isOpen, initialUrl, defaultDestination]);

  const handleProbe = async (targetUrl: string) => {
    if (!targetUrl || (!targetUrl.startsWith('http://') && !targetUrl.startsWith('https://'))) {
      return;
    }

    setIsProbing(true);
    setProbeError(null);

    try {
      const res = await api.probeUrl(targetUrl);
      setProbeResult(res);
      setFilename(res.filename);
      if (res.default_destination && !destination) {
        setDestination(res.default_destination);
      }
    } catch (err: any) {
      setProbeError(err?.toString() || 'Failed to probe URL');
    } finally {
      setIsProbing(false);
    }
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!url.trim() || isSubmitting) return;

    setIsSubmitting(true);
    try {
      let scheduledTime: number | undefined = undefined;
      if (isScheduled && scheduledDate) {
        scheduledTime = Math.floor(new Date(scheduledDate).getTime() / 1000);
      }

      const mirrors = mirrorsText
        .split('\n')
        .map((s) => s.trim())
        .filter((s) => s.length > 0 && (s.startsWith('http://') || s.startsWith('https://')));

      await onAdd({
        url: url.trim(),
        filename: filename.trim() || undefined,
        destination_path: destination.trim() || undefined,
        connections: probeResult?.resumable ? connections : 1,
        speed_limit: speedLimit > 0 ? speedLimit * 1024 : undefined,
        priority,
        scheduled_time: scheduledTime,
        user_agent: userAgent.trim() || undefined,
        referer: referer.trim() || undefined,
        expected_checksum: expectedChecksum.trim() || undefined,
        mirrors: mirrors.length > 0 ? mirrors : undefined,
        min_chunk_size: minChunkSize,
        proxy_mode: proxyMode,
        custom_proxy_url: proxyMode === 'custom' && customProxyUrl.trim() ? customProxyUrl.trim() : undefined,
      });

      onClose();
    } catch (err: any) {
      setProbeError(err?.toString() || 'Failed to start download');
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
          transition={{ duration: 0.18 }}
          className="w-full max-w-xl rounded-3xl bg-slate-900 border border-white/10 shadow-2xl overflow-hidden"
        >
          {/* Header */}
          <div className="flex items-center justify-between px-6 py-4 border-b border-white/5">
            <div className="flex items-center gap-2">
              <div className="flex h-7 w-7 items-center justify-center rounded-lg bg-indigo-500/20 text-indigo-400">
                <Sparkles size={16} />
              </div>
              <h2 className="text-sm font-semibold text-white">Add New Download</h2>
            </div>
            <button
              onClick={onClose}
              className="p-1 rounded-lg text-slate-400 hover:text-white hover:bg-slate-800 transition-colors"
            >
              <X size={16} />
            </button>
          </div>

          <form onSubmit={handleSubmit} className="p-6 space-y-4">
            {/* URL Input */}
            <div>
              <div className="flex items-center justify-between mb-1.5">
                <label className="text-xs font-medium text-slate-300">
                  {showCurlModal ? 'Paste cURL Command' : 'Download URL'}
                </label>
                <button
                  type="button"
                  onClick={() => setShowCurlModal(!showCurlModal)}
                  className="flex items-center gap-1.5 text-[11px] font-medium text-indigo-400 hover:text-indigo-300 transition-colors"
                >
                  <Terminal size={12} />
                  <span>{showCurlModal ? 'Switch to Standard URL' : 'Paste cURL Command'}</span>
                </button>
              </div>

              {showCurlModal ? (
                <div className="space-y-2">
                  <textarea
                    rows={4}
                    value={curlCommand}
                    onChange={(e) => setCurlCommand(e.target.value)}
                    placeholder={`curl "https://example.com/file.zip" \\\n  -H "User-Agent: Mozilla/5.0..." \\\n  -H "Referer: https://example.com" \\\n  --output custom_name.zip`}
                    className="w-full bg-slate-950/80 border border-indigo-500/30 rounded-xl p-3 text-xs font-mono text-slate-200 placeholder:text-slate-600 focus:outline-none focus:border-indigo-500"
                  />
                  <div className="flex justify-between items-center text-[11px]">
                    <span className="text-slate-400">Right-click network request in DevTools &rarr; Copy as cURL</span>
                    <button
                      type="button"
                      onClick={async () => {
                        if (!curlCommand.trim()) return;
                        try {
                          const parsed = await api.parseCurlCommand(curlCommand.trim());
                          setUrl(parsed.url);
                          if (parsed.filename) setFilename(parsed.filename);
                          if (parsed.user_agent) setUserAgent(parsed.user_agent);
                          if (parsed.referer) setReferer(parsed.referer);
                          if (parsed.custom_proxy_url) {
                            setProxyMode('custom');
                            setCustomProxyUrl(parsed.custom_proxy_url);
                          }
                          setShowCurlModal(false);
                          handleProbe(parsed.url);
                        } catch (err: any) {
                          setProbeError(err?.toString() || 'Failed to parse cURL command');
                        }
                      }}
                      className="px-3 py-1 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg font-medium transition-colors"
                    >
                      Parse & Fill Fields
                    </button>
                  </div>
                </div>
              ) : (
                <div className="relative">
                  <Link
                    size={15}
                    className="absolute left-3.5 top-1/2 -translate-y-1/2 text-slate-400 pointer-events-none"
                  />
                  <input
                    ref={urlInputRef}
                    type="text"
                    required
                    value={url}
                    onChange={(e) => {
                      const newUrl = e.target.value;
                      setUrl(newUrl);
                      if (newUrl.startsWith('http://') || newUrl.startsWith('https://')) {
                        handleProbe(newUrl);
                      }
                    }}
                    placeholder="https://example.com/file.zip"
                    className="w-full bg-slate-950/80 border border-white/10 rounded-xl pl-10 pr-24 py-2.5 text-xs text-slate-100 placeholder:text-slate-500 focus:outline-none focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500/30"
                  />
                  {isProbing && (
                    <div className="absolute right-3 top-1/2 -translate-y-1/2 flex items-center gap-1.5 text-xs text-indigo-400">
                      <Loader2 size={13} className="animate-spin" />
                      <span>Probing...</span>
                    </div>
                  )}
                </div>
              )}
            </div>

            {/* Probe info card */}
            {probeResult && (
              <motion.div
                initial={{ opacity: 0, height: 0 }}
                animate={{ opacity: 1, height: 'auto' }}
                className="p-3.5 rounded-2xl bg-slate-950/60 border border-white/5 space-y-2.5"
              >
                <div className="flex items-center gap-3">
                  <FileIcon
                    category={probeResult.category}
                    filename={probeResult.filename}
                    size={20}
                  />
                  <div className="min-w-0 flex-1">
                    <div className="text-xs font-semibold text-slate-200 truncate">
                      {probeResult.filename}
                    </div>
                    <div className="text-[11px] text-slate-400 flex items-center gap-2 mt-0.5">
                      <span>{probeResult.total_bytes > 0 ? formatBytes(probeResult.total_bytes) : 'Unknown size'}</span>
                      <span>•</span>
                      <span className="capitalize">{probeResult.category}</span>
                    </div>
                  </div>

                  {probeResult.resumable ? (
                    <span className="flex items-center gap-1 text-[11px] font-medium text-emerald-400 bg-emerald-500/10 px-2 py-0.5 rounded-full border border-emerald-500/20">
                      <Zap size={11} />
                      Accelerated Range Support
                    </span>
                  ) : (
                    <span className="flex items-center gap-1 text-[11px] font-medium text-amber-400 bg-amber-500/10 px-2 py-0.5 rounded-full border border-amber-500/20">
                      <AlertTriangle size={11} />
                      Single Stream Only
                    </span>
                  )}
                </div>
              </motion.div>
            )}

            {probeError && (
              <div className="p-3 rounded-xl bg-rose-500/10 border border-rose-500/20 text-xs text-rose-300">
                {probeError}
              </div>
            )}

            {/* Filename override */}
            <div>
              <label className="block text-xs font-medium text-slate-300 mb-1.5">
                Save As (Filename)
              </label>
              <input
                type="text"
                value={filename}
                onChange={(e) => setFilename(e.target.value)}
                placeholder={probeResult?.filename || 'download.bin'}
                className="w-full bg-slate-950/80 border border-white/10 rounded-xl px-3.5 py-2 text-xs text-slate-100 placeholder:text-slate-500 focus:outline-none focus:border-indigo-500"
              />
            </div>

            {/* Destination directory */}
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

            {/* Advanced toggle */}
            <div>
              <button
                type="button"
                onClick={() => setShowAdvanced(!showAdvanced)}
                className="flex items-center gap-1.5 text-xs text-indigo-400 hover:text-indigo-300 transition-colors"
              >
                <Sliders size={13} />
                <span>{showAdvanced ? 'Hide Options' : 'More Options (Streams, Limits, Scheduling)'}</span>
              </button>
            </div>

            {/* Advanced section */}
            {showAdvanced && (
              <motion.div
                initial={{ opacity: 0, height: 0 }}
                animate={{ opacity: 1, height: 'auto' }}
                className="pt-2 space-y-3.5 border-t border-white/5"
              >
                {/* Connection Slider */}
                <div>
                  <div className="flex items-center justify-between text-xs mb-1.5">
                    <span className="text-slate-300">Concurrent Segments</span>
                    <span className="font-mono text-indigo-300 font-semibold">{connections} streams</span>
                  </div>
                  <input
                    type="range"
                    min={1}
                    max={32}
                    step={1}
                    value={connections}
                    onChange={(e) => setConnections(parseInt(e.target.value, 10))}
                    className="w-full accent-indigo-500 cursor-pointer"
                  />
                  <div className="flex justify-between text-[10px] text-slate-500 mt-0.5">
                    <span>1 (Single)</span>
                    <span>8 (Optimal)</span>
                    <span>16 (Fast)</span>
                    <span>32 (Max)</span>
                  </div>
                </div>

                {/* Priority */}
                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <label className="block text-xs font-medium text-slate-300 mb-1">
                      Download Priority
                    </label>
                    <select
                      value={priority}
                      onChange={(e) => setPriority(parseInt(e.target.value, 10))}
                      className="w-full bg-slate-950/80 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 focus:outline-none"
                    >
                      <option value={0} className="bg-slate-900">Normal (0)</option>
                      <option value={1} className="bg-slate-900">High (1)</option>
                      <option value={2} className="bg-slate-900">Urgent (2)</option>
                    </select>
                  </div>

                  <div>
                    <label className="block text-xs font-medium text-slate-300 mb-1">
                      Speed Cap (KB/s)
                    </label>
                    <input
                      type="number"
                      min={0}
                      value={speedLimit}
                      onChange={(e) => setSpeedLimit(parseInt(e.target.value, 10) || 0)}
                      placeholder="0 = Unlimited"
                      className="w-full bg-slate-950/80 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 focus:outline-none"
                    />
                  </div>
                </div>

                {/* Chunk Size & Proxy Routing */}
                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <label className="block text-xs font-medium text-slate-300 mb-1">
                      Minimum Chunk Size
                    </label>
                    <select
                      value={minChunkSize}
                      onChange={(e) => setMinChunkSize(parseInt(e.target.value, 10))}
                      className="w-full bg-slate-950/80 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 focus:outline-none"
                    >
                      <option value={262144}>256 KB (High Granularity)</option>
                      <option value={524288}>512 KB</option>
                      <option value={1048576}>1 MB (Recommended)</option>
                      <option value={2097152}>2 MB</option>
                      <option value={5242880}>5 MB (Large Files)</option>
                      <option value={10485760}>10 MB</option>
                    </select>
                  </div>

                  <div>
                    <label className="block text-xs font-medium text-slate-300 mb-1">
                      Proxy Routing
                    </label>
                    <select
                      value={proxyMode}
                      onChange={(e) => setProxyMode(e.target.value as any)}
                      className="w-full bg-slate-950/80 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 focus:outline-none"
                    >
                      <option value="system">Auto System / VPN Proxy</option>
                      <option value="direct">Direct (No Proxy)</option>
                      <option value="custom">Custom Proxy</option>
                    </select>
                  </div>
                </div>

                {proxyMode === 'custom' && (
                  <div>
                    <label className="block text-xs font-medium text-slate-300 mb-1">
                      Custom Proxy URL
                    </label>
                    <input
                      type="text"
                      value={customProxyUrl}
                      onChange={(e) => setCustomProxyUrl(e.target.value)}
                      placeholder="http://127.0.0.1:10808 or socks5://127.0.0.1:1080"
                      className="w-full bg-slate-950/80 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 font-mono placeholder:text-slate-600 focus:outline-none focus:border-indigo-500"
                    />
                  </div>
                )}

                {/* Multi-Network Mirror Sources */}
                <div>
                  <div className="flex items-center justify-between text-xs mb-1">
                    <span className="font-medium text-slate-300">Multi-Network Mirrors / CDNs</span>
                    <span className="text-[10px] text-slate-500">1 URL per line</span>
                  </div>
                  <textarea
                    rows={2}
                    value={mirrorsText}
                    onChange={(e) => setMirrorsText(e.target.value)}
                    placeholder="https://mirror1.example.com/file.iso&#10;https://mirror2.example.com/file.iso"
                    className="w-full bg-slate-950/80 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 font-mono placeholder:text-slate-600 focus:outline-none focus:border-indigo-500 resize-none"
                  />
                  <p className="text-[10px] text-slate-500 mt-0.5">
                    Segments will be distributed concurrently across primary URL and mirror hosts.
                  </p>
                </div>

                {/* Expected Checksum */}
                <div>
                  <label className="block text-xs font-medium text-slate-300 mb-1">
                    Expected Hash / Checksum (SHA-256 or MD5)
                  </label>
                  <input
                    type="text"
                    value={expectedChecksum}
                    onChange={(e) => setExpectedChecksum(e.target.value)}
                    placeholder="Optional: e.g. e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                    className="w-full bg-slate-950/80 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 font-mono placeholder:text-slate-600 focus:outline-none focus:border-indigo-500"
                  />
                </div>

                {/* HTTP Headers & User-Agent */}
                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <label className="block text-xs font-medium text-slate-300 mb-1">
                      Custom Referer URL
                    </label>
                    <input
                      type="text"
                      value={referer}
                      onChange={(e) => setReferer(e.target.value)}
                      placeholder="https://source-site.com"
                      className="w-full bg-slate-950/80 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 placeholder:text-slate-600 focus:outline-none focus:border-indigo-500"
                    />
                  </div>
                  <div>
                    <label className="block text-xs font-medium text-slate-300 mb-1">
                      User-Agent Preset
                    </label>
                    <select
                      value={userAgent}
                      onChange={(e) => setUserAgent(e.target.value)}
                      className="w-full bg-slate-950/80 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 focus:outline-none focus:border-indigo-500"
                    >
                      <option value="">Default (Nexdrop)</option>
                      <option value="Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36">
                        Chrome 133 (Windows)
                      </option>
                      <option value="Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:135.0) Gecko/20100101 Firefox/135.0">
                        Firefox 135 (Windows)
                      </option>
                      <option value="Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.3 Safari/605.1.15">
                        Safari 18 (macOS)
                      </option>
                      <option value="curl/8.10.1">curl / CLI</option>
                    </select>
                  </div>
                </div>

                {/* Schedule toggle */}
                <div className="space-y-2 pt-1">
                  <label className="flex items-center gap-2 cursor-pointer text-xs text-slate-300">
                    <input
                      type="checkbox"
                      checked={isScheduled}
                      onChange={(e) => setIsScheduled(e.target.checked)}
                      className="rounded accent-indigo-500"
                    />
                    <span>Schedule this download for later</span>
                  </label>

                  {isScheduled && (
                    <input
                      type="datetime-local"
                      value={scheduledDate}
                      onChange={(e) => setScheduledDate(e.target.value)}
                      className="w-full bg-slate-950 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 focus:outline-none"
                    />
                  )}
                </div>
              </motion.div>
            )}

            {/* Footer Buttons */}
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
                disabled={isSubmitting || !url.trim()}
                className="flex items-center gap-2 px-5 py-2 rounded-xl text-xs font-semibold bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white shadow-md shadow-indigo-600/30 transition-all active:scale-95"
              >
                {isSubmitting ? (
                  <>
                    <Loader2 size={13} className="animate-spin" />
                    <span>Starting...</span>
                  </>
                ) : (
                  <>
                    <Zap size={14} />
                    <span>Download Now</span>
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
