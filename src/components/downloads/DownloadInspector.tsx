import React, { useState } from 'react';
import {
  X,
  Copy,
  Check,
  FolderOpen,
  Play,
  Pause,
  RotateCcw,
  Terminal,
  ShieldCheck,
  ShieldAlert,
  Hash,
  Layers,
  Info,
  ExternalLink,
  FileCheck,
  Download,
  Loader2,
} from 'lucide-react';
import { DownloadTask } from '../../types/download';
import { formatBytes, formatDuration, formatSpeed } from '../../utils/format';
import { StatusBadge } from './StatusBadge';
import { FileIcon } from './FileIcon';
import { api } from '../../services/api';

interface DownloadInspectorProps {
  task: DownloadTask;
  onClose: () => void;
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onRetry: (id: string) => void;
  onOpenFile: (id: string) => void;
  onRevealFile: (id: string) => void;
}

export const DownloadInspector: React.FC<DownloadInspectorProps> = ({
  task,
  onClose,
  onPause,
  onResume,
  onRetry,
  onOpenFile,
  onRevealFile,
}) => {
  const [copiedUrl, setCopiedUrl] = useState(false);
  const [copiedCurl, setCopiedCurl] = useState(false);
  const [copiedHash, setCopiedHash] = useState(false);
  const [expectedHash, setExpectedHash] = useState('');
  const [verifying, setVerifying] = useState(false);
  const [exportingChecksum, setExportingChecksum] = useState(false);
  const [verifyResult, setVerifyResult] = useState<{ matches: boolean; hash: string } | null>(null);
  const [activeTab, setActiveTab] = useState<'info' | 'segments' | 'checksum'>('info');

  const copyToClipboard = async (text: string, setter: (val: boolean) => void) => {
    try {
      await navigator.clipboard.writeText(text);
      setter(true);
      setTimeout(() => setter(false), 2000);
    } catch {
      // ignore
    }
  };

  const handleCopyCurl = async () => {
    try {
      const curl = await api.getCurlCommand(task.id);
      copyToClipboard(curl, setCopiedCurl);
    } catch {
      copyToClipboard(`curl -C - -O "${task.url}"`, setCopiedCurl);
    }
  };

  const handleVerifyChecksum = async (algo: 'sha256' | 'md5') => {
    setVerifying(true);
    setVerifyResult(null);
    try {
      const res = await api.verifyChecksum(task.id, algo, expectedHash.trim() || undefined);
      setVerifyResult({
        matches: res.matches,
        hash: res.calculated_hash,
      });
    } catch (err: any) {
      alert(`Verification failed: ${err}`);
    } finally {
      setVerifying(false);
    }
  };

  const handleExportChecksum = async (algo: 'sha256' | 'md5') => {
    setExportingChecksum(true);
    try {
      const exportedPath = await api.exportChecksumFile(task.id, algo);
      alert(`Exported checksum verification file to:\n${exportedPath}`);
    } catch (err: any) {
      alert(`Failed to export checksum file: ${err}`);
    } finally {
      setExportingChecksum(false);
    }
  };

  const progress = task.total_bytes > 0
    ? Math.min(100, Math.round((task.downloaded_bytes / task.total_bytes) * 100))
    : 0;

  return (
    <div className="fixed inset-y-0 right-0 w-full max-w-md bg-[#13151f]/95 backdrop-blur-xl border-l border-white/10 shadow-2xl z-40 flex flex-col animate-in slide-in-from-right duration-200">
      {/* Top Header */}
      <div className="flex items-center justify-between p-4 border-b border-white/5">
        <div className="flex items-center gap-3 min-w-0">
          <FileIcon category={task.category} filename={task.filename} size={28} />
          <div className="min-w-0">
            <h3 className="text-sm font-semibold text-slate-100 truncate" title={task.filename}>
              {task.filename}
            </h3>
            <div className="flex items-center gap-2 mt-0.5 flex-wrap">
              <StatusBadge status={task.status} />
              {task.antivirus_status === 'clean' && (
                <span className="flex items-center gap-1 text-[10px] font-medium text-emerald-400 bg-emerald-500/10 px-2 py-0.5 rounded-full border border-emerald-500/20">
                  <ShieldCheck size={11} />
                  Defender Clean
                </span>
              )}
              {task.antivirus_status === 'scanning' && (
                <span className="flex items-center gap-1 text-[10px] font-medium text-amber-400 bg-amber-500/10 px-2 py-0.5 rounded-full border border-amber-500/20">
                  <Loader2 size={11} className="animate-spin" />
                  Scanning
                </span>
              )}
              {task.antivirus_status === 'threat_detected' && (
                <span className="flex items-center gap-1 text-[10px] font-medium text-rose-400 bg-rose-500/10 px-2 py-0.5 rounded-full border border-rose-500/20 animate-pulse">
                  <ShieldAlert size={11} />
                  Threat Detected!
                </span>
              )}
              <span className="text-xs text-slate-400">
                {formatBytes(task.downloaded_bytes)} of {formatBytes(task.total_bytes)}
              </span>
            </div>
          </div>
        </div>
        <button
          onClick={onClose}
          className="p-1.5 text-slate-400 hover:text-slate-200 rounded-lg hover:bg-white/5 transition-colors"
        >
          <X className="w-5 h-5" />
        </button>
      </div>

      {/* Action Toolbar */}
      <div className="flex items-center gap-2 p-3 bg-white/[0.02] border-b border-white/5 px-4">
        {task.status === 'downloading' && (
          <button
            onClick={() => onPause(task.id)}
            className="flex items-center gap-1.5 px-3 py-1.5 bg-amber-500/10 hover:bg-amber-500/20 text-amber-300 text-xs font-medium rounded-lg border border-amber-500/20 transition-colors"
          >
            <Pause className="w-3.5 h-3.5" /> Pause
          </button>
        )}
        {task.status === 'paused' && (
          <button
            onClick={() => onResume(task.id)}
            className="flex items-center gap-1.5 px-3 py-1.5 bg-emerald-500/10 hover:bg-emerald-500/20 text-emerald-300 text-xs font-medium rounded-lg border border-emerald-500/20 transition-colors"
          >
            <Play className="w-3.5 h-3.5" /> Resume
          </button>
        )}
        {task.status === 'failed' && (
          <button
            onClick={() => onRetry(task.id)}
            className="flex items-center gap-1.5 px-3 py-1.5 bg-indigo-500/10 hover:bg-indigo-500/20 text-indigo-300 text-xs font-medium rounded-lg border border-indigo-500/20 transition-colors"
          >
            <RotateCcw className="w-3.5 h-3.5" /> Retry
          </button>
        )}
        {task.status === 'completed' && (
          <button
            onClick={() => onOpenFile(task.id)}
            className="flex items-center gap-1.5 px-3 py-1.5 bg-indigo-500/10 hover:bg-indigo-500/20 text-indigo-300 text-xs font-medium rounded-lg border border-indigo-500/20 transition-colors"
          >
            <ExternalLink className="w-3.5 h-3.5" /> Open
          </button>
        )}
        <button
          onClick={() => onRevealFile(task.id)}
          className="flex items-center gap-1.5 px-3 py-1.5 bg-white/5 hover:bg-white/10 text-slate-300 text-xs font-medium rounded-lg border border-white/10 transition-colors"
        >
          <FolderOpen className="w-3.5 h-3.5" /> Folder
        </button>
        <button
          onClick={handleCopyCurl}
          className="flex items-center gap-1.5 px-3 py-1.5 bg-white/5 hover:bg-white/10 text-slate-300 text-xs font-medium rounded-lg border border-white/10 transition-colors ml-auto"
          title="Copy as cURL command"
        >
          <Terminal className="w-3.5 h-3.5" />
          {copiedCurl ? 'Copied!' : 'cURL'}
        </button>
      </div>

      {/* Nav Tabs */}
      <div className="flex border-b border-white/5 px-4">
        <button
          onClick={() => setActiveTab('info')}
          className={`flex items-center gap-2 py-3 px-3 text-xs font-medium border-b-2 transition-colors ${
            activeTab === 'info'
              ? 'border-indigo-500 text-indigo-300'
              : 'border-transparent text-slate-400 hover:text-slate-200'
          }`}
        >
          <Info className="w-3.5 h-3.5" /> General
        </button>
        <button
          onClick={() => setActiveTab('segments')}
          className={`flex items-center gap-2 py-3 px-3 text-xs font-medium border-b-2 transition-colors ${
            activeTab === 'segments'
              ? 'border-indigo-500 text-indigo-300'
              : 'border-transparent text-slate-400 hover:text-slate-200'
          }`}
        >
          <Layers className="w-3.5 h-3.5" /> Streams ({task.connections || 1})
        </button>
        <button
          onClick={() => setActiveTab('checksum')}
          className={`flex items-center gap-2 py-3 px-3 text-xs font-medium border-b-2 transition-colors ${
            activeTab === 'checksum'
              ? 'border-indigo-500 text-indigo-300'
              : 'border-transparent text-slate-400 hover:text-slate-200'
          }`}
        >
          <Hash className="w-3.5 h-3.5" /> Checksum
        </button>
      </div>

      {/* Body Content */}
      <div className="flex-1 overflow-y-auto p-4 space-y-4">
        {activeTab === 'info' && (
          <div className="space-y-4 text-xs">
            {/* Progress Bar */}
            <div className="p-3 bg-white/[0.02] border border-white/5 rounded-xl space-y-2">
              <div className="flex justify-between items-center text-slate-300">
                <span className="font-medium">Overall Progress</span>
                <span className="font-semibold">{progress}%</span>
              </div>
              <div className="h-2 w-full bg-slate-800 rounded-full overflow-hidden">
                <div
                  className="h-full bg-gradient-to-r from-indigo-500 to-cyan-400 transition-all duration-300"
                  style={{ width: `${progress}%` }}
                />
              </div>
              <div className="flex justify-between text-slate-400 pt-1">
                <span>Speed: {formatSpeed(task.speed_bps)}</span>
                <span>ETA: {formatDuration(task.eta_seconds)}</span>
              </div>
            </div>

            {/* URL Section */}
            <div className="space-y-1.5">
              <div className="flex items-center justify-between">
                <span className="text-slate-400 font-medium">Source URL</span>
                <button
                  onClick={() => copyToClipboard(task.url, setCopiedUrl)}
                  className="flex items-center gap-1 text-indigo-400 hover:text-indigo-300"
                >
                  {copiedUrl ? <Check className="w-3 h-3 text-emerald-400" /> : <Copy className="w-3 h-3" />}
                  <span>{copiedUrl ? 'Copied' : 'Copy'}</span>
                </button>
              </div>
              <div className="p-2.5 bg-black/30 border border-white/5 rounded-lg text-slate-300 break-all select-text font-mono text-[11px]">
                {task.url}
              </div>
            </div>

            {/* Destination */}
            <div className="space-y-1.5">
              <span className="text-slate-400 font-medium">Destination Path</span>
              <div className="p-2.5 bg-black/30 border border-white/5 rounded-lg text-slate-300 break-all select-text font-mono text-[11px]">
                {task.destination_path}/{task.filename}
              </div>
            </div>

            {/* Technical Metadata Grid */}
            <div className="grid grid-cols-2 gap-3 pt-2">
              <div className="p-3 bg-white/[0.02] border border-white/5 rounded-xl">
                <div className="text-slate-500 mb-1">MIME Type</div>
                <div className="font-medium text-slate-200 truncate">{task.mime_type || 'Unknown'}</div>
              </div>
              <div className="p-3 bg-white/[0.02] border border-white/5 rounded-xl">
                <div className="text-slate-500 mb-1">Range Resumable</div>
                <div className={`font-medium ${task.resumable ? 'text-emerald-400' : 'text-amber-400'}`}>
                  {task.resumable ? 'Supported (Yes)' : 'Single stream only'}
                </div>
              </div>
              <div className="p-3 bg-white/[0.02] border border-white/5 rounded-xl">
                <div className="text-slate-500 mb-1">ETag Header</div>
                <div className="font-mono text-slate-200 truncate" title={task.etag || 'None'}>
                  {task.etag || 'None'}
                </div>
              </div>
              <div className="p-3 bg-white/[0.02] border border-white/5 rounded-xl">
                <div className="text-slate-500 mb-1">Last Modified</div>
                <div className="text-slate-200 truncate" title={task.last_modified || 'None'}>
                  {task.last_modified || 'Not reported'}
                </div>
              </div>
              <div className="p-3 bg-white/[0.02] border border-white/5 rounded-xl">
                <div className="text-slate-500 mb-1">Proxy Routing</div>
                <div className="text-slate-200 capitalize truncate font-mono">
                  {task.proxy_mode || 'System Auto'}
                </div>
              </div>
              <div className="p-3 bg-white/[0.02] border border-white/5 rounded-xl">
                <div className="text-slate-500 mb-1">Min Chunk Size</div>
                <div className="text-slate-200 font-mono">
                  {task.min_chunk_size ? formatBytes(task.min_chunk_size) : '1 MB'}
                </div>
              </div>
            </div>

            {/* Parallel Mirrors */}
            {task.mirrors && task.mirrors.length > 0 && (
              <div className="space-y-1.5 pt-1">
                <span className="text-slate-400 font-medium">Active Mirror CDNs ({task.mirrors.length})</span>
                <div className="space-y-1">
                  {task.mirrors.map((m, idx) => (
                    <div
                      key={idx}
                      className="p-2 bg-black/30 border border-white/5 rounded-lg text-slate-300 break-all select-text font-mono text-[10px]"
                    >
                      {m}
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>
        )}

        {activeTab === 'segments' && (
          <div className="space-y-3">
            <div className="text-xs text-slate-400 mb-2">
              Concurrent segmented download streams utilizing HTTP Range byte requests:
            </div>
            {task.segments.length === 0 ? (
              <div className="p-4 text-center text-xs text-slate-500 bg-white/[0.02] rounded-xl border border-white/5">
                Single stream download (server does not support multi-range partitioning)
              </div>
            ) : (
              task.segments.map((seg) => {
                const segPercent = seg.total_bytes > 0
                  ? Math.min(100, Math.round((seg.downloaded_bytes / seg.total_bytes) * 100))
                  : 0;
                return (
                  <div
                    key={seg.segment_index}
                    className="p-3 bg-white/[0.02] border border-white/5 rounded-xl space-y-1.5"
                  >
                    <div className="flex justify-between items-center text-xs">
                      <div className="flex items-center gap-2">
                        <span className="font-semibold text-indigo-400">Worker #{seg.segment_index + 1}</span>
                        <span className="text-[10px] uppercase font-mono px-1.5 py-0.5 rounded bg-white/5 text-slate-400">
                          {seg.status}
                        </span>
                      </div>
                      <span className="font-mono text-slate-300">{segPercent}%</span>
                    </div>
                    <div className="h-1.5 w-full bg-slate-800 rounded-full overflow-hidden">
                      <div
                        className="h-full bg-cyan-400 transition-all duration-300"
                        style={{ width: `${segPercent}%` }}
                      />
                    </div>
                    <div className="flex justify-between text-[11px] text-slate-500 font-mono">
                      <span>{formatBytes(seg.start_byte)} – {formatBytes(seg.end_byte)}</span>
                      <span>{formatBytes(seg.downloaded_bytes)}</span>
                    </div>
                  </div>
                );
              })
            )}
          </div>
        )}

        {activeTab === 'checksum' && (
          <div className="space-y-4 text-xs">
            <div className="p-3 bg-white/[0.02] border border-white/5 rounded-xl space-y-2">
              <div className="font-medium text-slate-200 flex items-center gap-2">
                <ShieldCheck className="w-4 h-4 text-indigo-400" />
                <span>File Integrity Verification</span>
              </div>
              <p className="text-slate-400 leading-relaxed text-[11px]">
                Verify whether the downloaded file matches the cryptographic hash published by the author (ISO, archive, or binary).
              </p>
            </div>

            {/* Stored / Computed Checksums */}
            <div className="space-y-3">
              <div>
                <div className="flex justify-between items-center text-slate-400 mb-1">
                  <span>SHA-256 Hash</span>
                  {task.checksum_sha256 && (
                    <div className="flex items-center gap-2">
                      <button
                        onClick={() => handleExportChecksum('sha256')}
                        disabled={exportingChecksum}
                        className="text-indigo-400 hover:text-indigo-300 flex items-center gap-1 text-[11px]"
                        title="Save .sha256 file alongside download"
                      >
                        <FileCheck className="w-3 h-3" />
                        <span>{exportingChecksum ? 'Exporting...' : 'Export .sha256'}</span>
                      </button>
                      <button
                        onClick={() => copyToClipboard(task.checksum_sha256!, setCopiedHash)}
                        className="text-indigo-400 hover:text-indigo-300 flex items-center gap-1 text-[11px]"
                      >
                        <Copy className="w-3 h-3" /> Copy
                      </button>
                    </div>
                  )}
                </div>
                <div className="p-2.5 bg-black/30 border border-white/5 rounded-lg font-mono text-[11px] text-slate-300 break-all select-text">
                  {task.checksum_sha256 || 'Not computed yet (runs upon download completion)'}
                </div>
              </div>

              <div>
                <div className="flex justify-between items-center text-slate-400 mb-1">
                  <span>MD5 Hash</span>
                </div>
                <div className="p-2.5 bg-black/30 border border-white/5 rounded-lg font-mono text-[11px] text-slate-300 break-all select-text">
                  {task.checksum_md5 || 'Not computed yet'}
                </div>
              </div>
            </div>

            {/* Checksum Tester */}
            <div className="p-3 bg-white/[0.02] border border-white/5 rounded-xl space-y-3">
              <div className="font-medium text-slate-300">Compare with Expected Hash</div>
              <input
                type="text"
                value={expectedHash}
                onChange={(e) => setExpectedHash(e.target.value)}
                placeholder="Paste expected SHA-256 or MD5..."
                className="w-full px-3 py-2 bg-black/40 border border-white/10 rounded-lg text-slate-200 placeholder-slate-500 focus:outline-none focus:border-indigo-500 font-mono text-[11px]"
              />

              <div className="flex gap-2">
                <button
                  disabled={verifying}
                  onClick={() => handleVerifyChecksum('sha256')}
                  className="flex-1 py-1.5 bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white font-medium rounded-lg transition-colors text-xs"
                >
                  {verifying ? 'Calculating...' : 'Verify SHA-256'}
                </button>
                <button
                  disabled={verifying}
                  onClick={() => handleVerifyChecksum('md5')}
                  className="py-1.5 px-3 bg-white/5 hover:bg-white/10 text-slate-300 font-medium rounded-lg border border-white/10 transition-colors text-xs"
                >
                  Verify MD5
                </button>
              </div>

              {verifyResult && (
                <div
                  className={`p-2.5 rounded-lg border flex items-center gap-2 ${
                    verifyResult.matches
                      ? 'bg-emerald-500/10 border-emerald-500/30 text-emerald-300'
                      : 'bg-rose-500/10 border-rose-500/30 text-rose-300'
                  }`}
                >
                  {verifyResult.matches ? (
                    <>
                      <ShieldCheck className="w-4 h-4 text-emerald-400 shrink-0" />
                      <span>Valid! File matches expected checksum.</span>
                    </>
                  ) : (
                    <>
                      <ShieldAlert className="w-4 h-4 text-rose-400 shrink-0" />
                      <span>Checksum mismatch! File may be corrupt or modified.</span>
                    </>
                  )}
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
};
