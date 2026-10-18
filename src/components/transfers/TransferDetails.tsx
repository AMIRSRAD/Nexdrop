import React, { useState } from 'react';
import { Check, Copy, FileCheck, FolderOpen, ShieldAlert, ShieldCheck, Terminal } from 'lucide-react';
import type { DownloadTask } from '../../types/download';
import { api } from '../../services/api';
import { formatBytes, formatDate, joinPath } from '../../utils/format';
import { PRIORITIES, STATUS_COLOR, STATUS_LABEL, progressOf } from '../../lib/meta';
import { cn } from '../../lib/cn';
import { Button } from '../ui/Button';
import { Input } from '../ui/Field';
import { useToast } from '../ui/Toast';
import { ProgressTrack } from './ProgressTrack';
import type { TaskHandlers } from './taskMenu';
import { errText } from '../../hooks/useDownloads';

type Tab = 'overview' | 'connections' | 'integrity';

export const TransferDetails: React.FC<{ task: DownloadTask; handlers: TaskHandlers }> = ({ task, handlers }) => {
  const [tab, setTab] = useState<Tab>('overview');
  const tabs: { id: Tab; label: string }[] = [
    { id: 'overview', label: 'Overview' },
    { id: 'connections', label: `Connections${task.segments.length > 1 ? ` · ${task.segments.length}` : ''}` },
    { id: 'integrity', label: 'Integrity' },
  ];

  return (
    <div className="pt-1 pr-6 pb-5 pl-[80px]" onClick={(e) => e.stopPropagation()}>
      <div className="mb-4 flex items-center gap-1 border-b border-line">
        {tabs.map((t) => (
          <button
            key={t.id}
            type="button"
            onClick={() => setTab(t.id)}
            className={cn(
              'relative -mb-px h-8 px-2.5 text-[12.5px] font-medium transition-colors',
              tab === t.id ? 'text-fg' : 'text-fg-3 hover:text-fg-2',
            )}
          >
            {t.label}
            {tab === t.id && <span className="absolute inset-x-2 bottom-0 h-[2px] rounded-full bg-accent" />}
          </button>
        ))}
        <div className="ml-auto flex items-center gap-1 pb-1">
          <Button size="sm" variant="ghost" icon={Copy} onClick={() => handlers.copyLink(task)}>
            Copy link
          </Button>
          <Button size="sm" variant="ghost" icon={Terminal} onClick={() => handlers.copyCurl(task)}>
            cURL
          </Button>
        </div>
      </div>

      {tab === 'overview' && <Overview task={task} handlers={handlers} />}
      {tab === 'connections' && <Connections task={task} />}
      {tab === 'integrity' && <Integrity task={task} />}
    </div>
  );
};

const Row: React.FC<{ label: string; children: React.ReactNode; mono?: boolean; className?: string }> = ({ label, children, mono, className }) => (
  <div className={cn('min-w-0', className)}>
    <dt className="mb-0.5 text-[11.5px] text-fg-3">{label}</dt>
    <dd className={cn('selectable truncate text-[12.5px] text-fg', mono && 'font-mono text-[11.5px]')}>{children}</dd>
  </div>
);

const Overview: React.FC<{ task: DownloadTask; handlers: TaskHandlers }> = ({ task, handlers }) => {
  const path = joinPath(task.destination_path, task.filename);
  return (
    <div className="space-y-4">
      <div className="grid grid-cols-[1fr_auto] items-end gap-3">
        <Row label="Source" mono>
          <span title={task.final_url || task.url}>{task.final_url || task.url}</span>
        </Row>
        <span />
        <Row label="Saved to" mono>
          <span title={path}>{path}</span>
        </Row>
        <Button size="sm" variant="secondary" icon={FolderOpen} onClick={() => handlers.reveal(task.id)}>
          Show in folder
        </Button>
      </div>

      <dl className="grid grid-cols-4 gap-x-6 gap-y-3 rounded-lg bg-surface-2 p-3.5 shadow-[inset_0_0_0_1px_var(--line)]">
        <Row label="Status">
          <span className="font-medium" style={{ color: STATUS_COLOR[task.status] }}>
            {STATUS_LABEL[task.status]}
          </span>
        </Row>
        <Row label="Downloaded">
          {formatBytes(task.downloaded_bytes)} {task.total_bytes > 0 && <span className="text-fg-3">of {formatBytes(task.total_bytes)}</span>}
        </Row>
        <Row label="Resume support">{task.resumable ? 'Yes — segmented' : 'No — single stream'}</Row>
        <Row label="Connections">{task.connections || 1}</Row>
        <Row label="Priority">{PRIORITIES.find((p) => p.value === task.priority)?.label ?? 'Normal'}</Row>
        <Row label="Speed limit">{task.speed_limit > 0 ? `${formatBytes(task.speed_limit)}/s` : 'None'}</Row>
        <Row label="Proxy">
          <span className="capitalize">{task.proxy_mode || 'system'}</span>
        </Row>
        <Row label="Type" mono>
          {task.mime_type || '—'}
        </Row>
        <Row label="Added">{formatDate(task.created_at)}</Row>
        <Row label="Finished">{task.completed_at ? formatDate(task.completed_at) : '—'}</Row>
        <Row label="Last modified">{task.last_modified || '—'}</Row>
        <Row label="ETag" mono>
          {task.etag || '—'}
        </Row>
      </dl>

      {task.antivirus_status && task.antivirus_status !== 'skipped' && (
        <div
          className={cn(
            'flex items-center gap-2 text-[12.5px]',
            task.antivirus_status === 'clean' && 'text-success',
            task.antivirus_status === 'threat_detected' && 'text-danger',
            task.antivirus_status === 'scanning' && 'text-fg-2',
          )}
        >
          {task.antivirus_status === 'threat_detected' ? <ShieldAlert size={15} /> : <ShieldCheck size={15} />}
          {task.antivirus_status === 'clean' && 'Scanned — no threats found'}
          {task.antivirus_status === 'scanning' && 'Scanning with antivirus…'}
          {task.antivirus_status === 'threat_detected' && 'Threat detected — do not open this file'}
        </div>
      )}

      {task.mirrors && task.mirrors.length > 0 && (
        <div>
          <div className="mb-1 text-[11.5px] text-fg-3">Mirrors ({task.mirrors.length})</div>
          <ul className="space-y-0.5">
            {task.mirrors.map((m) => (
              <li key={m} className="selectable truncate font-mono text-[11.5px] text-fg-2">
                {m}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
};

const Connections: React.FC<{ task: DownloadTask }> = ({ task }) => {
  if (task.segments.length === 0) {
    return (
      <div className="rounded-lg bg-surface-2 p-4 text-[12.5px] text-fg-2 shadow-[inset_0_0_0_1px_var(--line)]">
        {task.resumable
          ? 'Connection details appear once the download starts.'
          : 'This server doesn’t support byte ranges, so the file is fetched over a single connection.'}
      </div>
    );
  }
  return (
    <div className="space-y-4">
      <div>
        <div className="mb-1.5 flex justify-between text-[11.5px] text-fg-3">
          <span>File map</span>
          <span className="tabular">{Math.round(progressOf(task) * 100)}%</span>
        </div>
        <ProgressTrack progress={progressOf(task)} status={task.status} segments={task.segments} totalBytes={task.total_bytes} height={10} className="rounded-md" />
      </div>
      <div className="grid grid-cols-2 gap-x-6 gap-y-2.5 xl:grid-cols-4">
        {task.segments.map((seg) => {
          const p = seg.total_bytes > 0 ? Math.min(1, seg.downloaded_bytes / seg.total_bytes) : 0;
          const segStatus = p >= 1 ? 'completed' : seg.status === 'downloading' && task.status === 'downloading' ? 'downloading' : task.status === 'failed' ? 'failed' : 'paused';
          return (
            <div key={seg.segment_index} className="min-w-0">
              <div className="mb-1 flex items-baseline justify-between text-[11.5px] tabular">
                <span className="font-medium text-fg-2">#{seg.segment_index + 1}</span>
                <span className="text-fg-3">
                  {formatBytes(seg.downloaded_bytes)} / {formatBytes(seg.total_bytes)}
                </span>
              </div>
              <ProgressTrack progress={p} status={segStatus as DownloadTask['status']} totalBytes={seg.total_bytes} height={3} />
            </div>
          );
        })}
      </div>
    </div>
  );
};

const Integrity: React.FC<{ task: DownloadTask }> = ({ task }) => {
  const toast = useToast();
  const [expected, setExpected] = useState('');
  const [busy, setBusy] = useState<string | null>(null);
  const [result, setResult] = useState<{ matches: boolean; hash: string; algo: string } | null>(null);

  const verify = async (algo: 'sha256' | 'md5') => {
    setBusy(algo);
    setResult(null);
    try {
      const res = await api.verifyChecksum(task.id, algo, expected.trim() || undefined);
      setResult({ matches: res.matches, hash: res.calculated_hash, algo });
    } catch (err) {
      toast.show({ tone: 'error', title: 'Verification failed', description: errText(err) });
    } finally {
      setBusy(null);
    }
  };

  const exportFile = async () => {
    setBusy('export');
    try {
      const path = await api.exportChecksumFile(task.id, 'sha256');
      toast.show({ tone: 'success', title: 'Checksum file saved', description: path });
    } catch (err) {
      toast.show({ tone: 'error', title: "Couldn't export checksum", description: errText(err) });
    } finally {
      setBusy(null);
    }
  };

  if (task.status !== 'completed') {
    return (
      <div className="rounded-lg bg-surface-2 p-4 text-[12.5px] text-fg-2 shadow-[inset_0_0_0_1px_var(--line)]">
        Checksums are calculated when the download finishes.
      </div>
    );
  }

  return (
    <div className="grid grid-cols-2 gap-6">
      <div className="space-y-3">
        <HashRow label="SHA-256" value={task.checksum_sha256} />
        <HashRow label="MD5" value={task.checksum_md5} />
        {task.checksum_sha256 && (
          <Button size="sm" variant="ghost" icon={FileCheck} loading={busy === 'export'} onClick={exportFile} className="-ml-2">
            Save .sha256 file next to download
          </Button>
        )}
      </div>
      <div>
        <div className="mb-1.5 text-[11.5px] text-fg-3">Compare with published hash</div>
        <Input mono value={expected} onChange={(e) => setExpected(e.target.value)} placeholder="Paste SHA-256 or MD5" />
        <div className="mt-2 flex gap-2">
          <Button size="sm" variant="primary" loading={busy === 'sha256'} disabled={!!busy} onClick={() => verify('sha256')}>
            Verify SHA-256
          </Button>
          <Button size="sm" variant="secondary" loading={busy === 'md5'} disabled={!!busy} onClick={() => verify('md5')}>
            Verify MD5
          </Button>
        </div>
        {result && (
          <div className={cn('mt-3 flex items-start gap-2 text-[12.5px]', result.matches ? 'text-success' : 'text-danger')}>
            {result.matches ? <ShieldCheck size={15} className="mt-px shrink-0" /> : <ShieldAlert size={15} className="mt-px shrink-0" />}
            <span>
              {expected.trim()
                ? result.matches
                  ? 'Match — the file is intact.'
                  : 'Mismatch — the file may be corrupt or tampered with.'
                : `${result.algo.toUpperCase()} calculated.`}
            </span>
          </div>
        )}
      </div>
    </div>
  );
};

const HashRow: React.FC<{ label: string; value?: string }> = ({ label, value }) => {
  const [copied, setCopied] = useState(false);
  return (
    <div>
      <div className="mb-0.5 flex items-center justify-between text-[11.5px] text-fg-3">
        <span>{label}</span>
        {value && (
          <button
            type="button"
            onClick={() => {
              navigator.clipboard.writeText(value).then(() => {
                setCopied(true);
                setTimeout(() => setCopied(false), 1500);
              });
            }}
            className="flex items-center gap-1 rounded px-1 text-fg-3 hover:text-fg"
          >
            {copied ? <Check size={12} className="text-success" /> : <Copy size={12} />}
            {copied ? 'Copied' : 'Copy'}
          </button>
        )}
      </div>
      <div className="selectable font-mono text-[11.5px] break-all text-fg-2">{value || 'Not calculated'}</div>
    </div>
  );
};
