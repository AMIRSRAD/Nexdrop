import React, { useEffect, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { ChevronRight, ClipboardPaste, Download, Film, Folder, Link2, ListPlus, TriangleAlert, Terminal, Zap } from 'lucide-react';
import { api } from '../../services/api';
import type { AddDownloadRequest, AppSettings, BrowserRequest, QueueInfo, StreamProbe, UrlProbeResult } from '../../types/download';
import { formatBytes, formatDuration } from '../../utils/format';
import { CHUNK_SIZES, PRIORITIES, USER_AGENTS, categoryMeta, isHttpUrl, looksLikeCurl } from '../../lib/meta';
import { cn } from '../../lib/cn';
import { Dialog } from '../ui/Dialog';
import { Button } from '../ui/Button';
import { Input, Label, NumberInput, Segmented, Select, Textarea } from '../ui/Field';
import { FileTile } from '../transfers/FileTile';
import { errText } from '../../hooks/useDownloads';

interface AddDownloadDialogProps {
  open: boolean;
  initialText: string;
  settings: AppSettings;
  onClose: () => void;
  onSubmit: (req: AddDownloadRequest) => Promise<void>;
  queues?: QueueInfo[];
  /** Queue preselected in the picker. */
  defaultQueue?: string;
  /** What the browser sent with the link: name, page, user agent and cookies. */
  initialRequest?: BrowserRequest;
}

const STREAM_URL_RE = /\.(m3u8?|mpd)(\?|#|$)/i;
const STREAM_MIME_RE = /mpegurl|dash\+xml/i;

type ProbeState =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'ok'; result: UrlProbeResult }
  | { kind: 'error'; message: string };

export const AddDownloadDialog: React.FC<AddDownloadDialogProps> = ({ open, initialText, settings, onClose, onSubmit, queues, defaultQueue, initialRequest }) => (
  <Dialog open={open} onClose={onClose} title="New download" width={560} bare>
    {open && (
      <AddDownloadForm
        initialText={initialText}
        settings={settings}
        onClose={onClose}
        onSubmit={onSubmit}
        queues={queues}
        defaultQueue={defaultQueue}
        initialRequest={initialRequest}
      />
    )}
  </Dialog>
);

const AddDownloadForm: React.FC<Omit<AddDownloadDialogProps, 'open'>> = ({ initialText, settings, onClose, onSubmit, queues = [], defaultQueue, initialRequest }) => {
  // A streaming-video link (HLS / DASH): shows a quality picker instead of file details.
  const [stream, setStream] = useState<StreamProbe | null>(null);
  const [variantId, setVariantId] = useState('');
  const [queueId, setQueueId] = useState(defaultQueue && queues.some((q) => q.id === defaultQueue) ? defaultQueue : 'main');
  const [url, setUrl] = useState('');
  const [filename, setFilename] = useState(initialRequest?.filename ?? '');
  const filenameTouched = useRef(!!initialRequest?.filename);
  const [destination, setDestination] = useState(settings.default_destination);
  const [probe, setProbe] = useState<ProbeState>({ kind: 'idle' });
  const [curlNote, setCurlNote] = useState<string | null>(null);
  const [customHeaders, setCustomHeaders] = useState<Record<string, string>>(initialRequest?.headers ?? {});

  const [when, setWhen] = useState<'now' | 'later'>('now');
  const [scheduledAt, setScheduledAt] = useState(defaultScheduleValue(settings.scheduler_start_time));
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [connections, setConnections] = useState(settings.default_connections || 8);
  const [priority, setPriority] = useState(0);
  const [speedLimit, setSpeedLimit] = useState(0);
  const [minChunk, setMinChunk] = useState(settings.min_chunk_size || 1048576);
  const [proxyMode, setProxyMode] = useState<AddDownloadRequest['proxy_mode']>(settings.proxy_mode || 'system');
  const [proxyUrl, setProxyUrl] = useState(settings.custom_proxy_url ?? '');
  const [mirrors, setMirrors] = useState('');
  const [checksum, setChecksum] = useState(initialRequest?.checksum ?? '');
  const [referer, setReferer] = useState(initialRequest?.referer ?? '');
  const [userAgent, setUserAgent] = useState(initialRequest?.userAgent ?? '');

  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const probeSeq = useRef(0);

  const runProbe = async (target: string) => {
    const seq = ++probeSeq.current;
    if (!isHttpUrl(target)) {
      setProbe({ kind: 'idle' });
      return;
    }
    setProbe({ kind: 'loading' });
    try {
      let result: UrlProbeResult | null = null;
      let isStream = STREAM_URL_RE.test(target);
      if (!isStream) {
        result = await api.probeUrl(target, { referer: referer.trim() || undefined, userAgent: userAgent || undefined, headers: customHeaders });
        isStream = STREAM_MIME_RE.test(result.mime_type ?? '');
      }
      if (isStream) {
        const info = await api.probeStream(target, referer.trim() || undefined);
        if (seq !== probeSeq.current) return;
        setStream(info);
        setVariantId(info.variants[0]?.id ?? '');
        setProbe({
          kind: 'ok',
          result: { url: target, filename: info.filename, total_bytes: 0, resumable: true, category: 'videos', default_destination: '' },
        });
        if (!filenameTouched.current) setFilename(info.filename);
        return;
      }
      if (seq !== probeSeq.current) return;
      setStream(null);
      setProbe({ kind: 'ok', result: result! });
      if (!filenameTouched.current) setFilename(result!.filename);
    } catch (err) {
      if (seq !== probeSeq.current) return;
      setProbe({ kind: 'error', message: errText(err) });
    }
  };

  const importCurl = async (text: string) => {
    try {
      const parsed = await api.parseCurlCommand(text.trim());
      setUrl(parsed.url);
      if (parsed.filename) {
        setFilename(parsed.filename);
        filenameTouched.current = true;
      }
      if (parsed.user_agent) setUserAgent(parsed.user_agent);
      if (parsed.referer) setReferer(parsed.referer);
      if (parsed.custom_proxy_url) {
        setProxyMode('custom');
        setProxyUrl(parsed.custom_proxy_url);
      }
      const headerCount = Object.keys(parsed.custom_headers ?? {}).length;
      setCustomHeaders(parsed.custom_headers ?? {});
      const imported = [
        parsed.user_agent && 'user agent',
        parsed.referer && 'referer',
        parsed.custom_proxy_url && 'proxy',
        headerCount > 0 && `${headerCount} header${headerCount === 1 ? '' : 's'} (cookies/auth are kept private)`,
      ].filter(Boolean);
      setCurlNote(imported.length ? `Imported from cURL: ${imported.join(', ')}` : 'Imported from cURL command');
      runProbe(parsed.url);
    } catch (err) {
      setProbe({ kind: 'error', message: `Couldn't read that cURL command — ${errText(err)}` });
    }
  };

  const handleUrlInput = (text: string) => {
    if (looksLikeCurl(text)) {
      importCurl(text);
      return;
    }
    setUrl(text);
    setCurlNote(null);
  };

  // Initial text from omnibox / clipboard / drag-drop / the browser
  useEffect(() => {
    if (initialRequest && Object.keys(initialRequest.headers ?? {}).length > 0) {
      setCurlNote('Sent from your browser, with your login for this site');
    } else if (initialRequest) {
      setCurlNote('Sent from your browser');
    }
    if (initialText) handleUrlInput(initialText.trim());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Debounced probing while typing
  useEffect(() => {
    if (!url.trim()) {
      setProbe({ kind: 'idle' });
      return;
    }
    const t = window.setTimeout(() => runProbe(url.trim()), 350);
    return () => window.clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [url]);

  const pasteFromClipboard = async () => {
    try {
      const text = (await navigator.clipboard.readText()).trim();
      if (text) handleUrlInput(text);
    } catch {
      /* clipboard permission denied */
    }
  };

  const result = probe.kind === 'ok' ? probe.result : null;
  const resumable = result?.resumable ?? true;
  const validUrl = isHttpUrl(url);
  const canSubmit = validUrl && !submitting && (when === 'now' || !!scheduledAt) && !(stream && (stream.unsupported || !variantId));

  const submit = async (e?: React.FormEvent, hold = false) => {
    e?.preventDefault();
    if (!canSubmit) return;
    setSubmitting(true);
    setSubmitError(null);
    try {
      const mirrorList = mirrors
        .split(/\s+/)
        .map((s) => s.trim())
        .filter(isHttpUrl);
      await onSubmit({
        url: url.trim(),
        filename: filename.trim() || undefined,
        destination_path: destination.trim() || undefined,
        connections: resumable ? connections : 1,
        speed_limit: speedLimit > 0 ? speedLimit * 1024 : undefined,
        priority,
        scheduled_time: when === 'later' && scheduledAt ? Math.floor(new Date(scheduledAt).getTime() / 1000) : undefined,
        user_agent: userAgent || undefined,
        referer: referer.trim() || undefined,
        expected_checksum: checksum.trim() || undefined,
        mirrors: mirrorList.length ? mirrorList : undefined,
        min_chunk_size: minChunk,
        proxy_mode: proxyMode,
        custom_proxy_url: proxyMode === 'custom' && proxyUrl.trim() ? proxyUrl.trim() : undefined,
        custom_headers: Object.keys(customHeaders).length > 0 ? customHeaders : undefined,
        queue_id: queues.length > 0 ? queueId : undefined,
        hold: hold && when === 'now' ? true : undefined,
        stream_variant: stream ? variantId : undefined,
      });
      onClose();
    } catch (err) {
      setSubmitError(errText(err));
      setSubmitting(false);
    }
  };

  const meta = categoryMeta(result?.category);

  return (
    <form onSubmit={submit} className="flex min-h-0 flex-col">
      <div className="min-h-0 space-y-4 overflow-y-auto px-5 pt-1 pb-5">
        {/* URL */}
        <div>
          <Input
            data-autofocus
            inputSize="lg"
            icon={looksLikeCurl(url) || curlNote ? Terminal : Link2}
            value={url}
            onChange={(e) => handleUrlInput(e.target.value)}
            onPaste={(e) => {
              const text = e.clipboardData.getData('text');
              if (looksLikeCurl(text)) {
                e.preventDefault();
                importCurl(text);
              }
            }}
            placeholder="https://example.com/file.zip — or paste a cURL command"
            suffix={
              <Button size="sm" variant="ghost" icon={ClipboardPaste} onClick={pasteFromClipboard} className="h-7 px-2 text-fg-3">
                Paste
              </Button>
            }
          />
          {curlNote && <div className="mt-1.5 text-[12px] text-accent">{curlNote}</div>}
        </div>

        {/* File preview */}
        <div className="flex items-center gap-3 rounded-lg bg-surface-2 p-3 shadow-[inset_0_0_0_1px_var(--line)]">
          {probe.kind === 'loading' ? (
            <div className="h-10 w-10 shrink-0 animate-pulse rounded-lg bg-surface-3" />
          ) : (
            <FileTile category={result?.category ?? 'other'} size={40} className={cn(!result && 'opacity-40')} />
          )}
          <div className="min-w-0 flex-1">
            <input
              value={filename}
              onChange={(e) => {
                setFilename(e.target.value);
                filenameTouched.current = e.target.value.length > 0;
              }}
              placeholder={probe.kind === 'loading' ? 'Checking link…' : 'File name'}
              spellCheck={false}
              aria-label="File name"
              className="-mx-1.5 h-7 w-[calc(100%+12px)] rounded-md bg-transparent px-1.5 text-[13.5px] font-medium text-fg outline-none hover:bg-hover focus:bg-canvas focus:shadow-[0_0_0_1px_var(--accent)]"
            />
            <div className="flex h-5 items-center gap-1.5 text-[12px] text-fg-3">
              {probe.kind === 'loading' && <span>Fetching file info…</span>}
              {probe.kind === 'idle' && <span>Enter a link to see file details</span>}
              {probe.kind === 'error' && (
                <span className="flex min-w-0 items-center gap-1 text-danger">
                  <TriangleAlert size={12} className="shrink-0" />
                  <span className="truncate" title={probe.message}>
                    {probe.message}
                  </span>
                </span>
              )}
              {result && stream && (
                <>
                  <Film size={12} className="shrink-0" />
                  <span>{stream.kind === 'dash' ? 'DASH video stream' : 'HLS video stream'}</span>
                  {stream.duration_secs > 0 && (
                    <>
                      <span>·</span>
                      <span className="tabular">{formatDuration(Math.round(stream.duration_secs))}</span>
                    </>
                  )}
                </>
              )}
              {result && !stream && (
                <>
                  <span className="tabular">{result.total_bytes > 0 ? formatBytes(result.total_bytes) : 'Unknown size'}</span>
                  <span>·</span>
                  <span>{meta.label}</span>
                  <span>·</span>
                  {result.resumable ? (
                    <span className="flex items-center gap-1 text-success">
                      <Zap size={11} /> Resumable, multi-connection
                    </span>
                  ) : (
                    <span className="text-warning">Single connection, can’t resume</span>
                  )}
                </>
              )}
            </div>
          </div>
        </div>

        {/* Quality (streaming video only) */}
        {stream && (
          <div>
            <div className="flex items-center gap-3">
              <Label className="mb-0 w-24 shrink-0">Quality</Label>
              <Select
                value={variantId}
                onValueChange={setVariantId}
                options={stream.variants.map((v) => ({ value: v.id, label: v.label }))}
                className="w-64"
                disabled={!!stream.unsupported}
              />
              {stream.variants.find((v) => v.id === variantId)?.separate_audio && !stream.unsupported && (
                <span className="text-[12px] text-fg-3">Video and audio get joined into one file</span>
              )}
            </div>
            {stream.unsupported && (
              <div className="mt-2 flex items-center gap-1.5 text-[12.5px] text-danger">
                <TriangleAlert size={13} className="shrink-0" />
                {stream.unsupported}
              </div>
            )}
          </div>
        )}

        {/* Destination */}
        <div>
          <Label>Save to</Label>
          <Input icon={Folder} mono value={destination} onChange={(e) => setDestination(e.target.value)} placeholder="Downloads" />
        </div>

        {/* Queue */}
        {queues.length > 0 && (
          <div className="flex items-center gap-3">
            <Label className="mb-0 w-24 shrink-0">Queue</Label>
            <Select value={queueId} onValueChange={setQueueId} options={queues.map((q) => ({ value: q.id, label: q.name }))} className="w-56" />
            {(() => {
              const q = queues.find((x) => x.id === queueId);
              if (!q) return null;
              if (!q.open) return <span className="text-[12px] text-fg-3">Waits for its schedule</span>;
              const waiting = q.active + q.queued;
              return waiting > 0 ? <span className="text-[12px] text-fg-3 tabular">{waiting} in line</span> : null;
            })()}
          </div>
        )}

        {/* When */}
        <div className="flex items-center gap-3">
          <Segmented
            value={when}
            onChange={setWhen}
            options={[
              { value: 'now', label: 'Start now' },
              { value: 'later', label: 'Schedule' },
            ]}
          />
          {when === 'later' && (
            <input
              type="datetime-local"
              value={scheduledAt}
              onChange={(e) => setScheduledAt(e.target.value)}
              className="h-8 rounded-lg border border-line-strong bg-surface-2 px-2.5 text-[12.5px] text-fg outline-none focus:border-accent"
            />
          )}
        </div>

        {/* Advanced */}
        <div className="rounded-lg shadow-[inset_0_0_0_1px_var(--line)]">
          <button
            type="button"
            onClick={() => setShowAdvanced((v) => !v)}
            className="flex h-9 w-full items-center gap-2 px-3 text-[12.5px] font-medium text-fg-2 hover:text-fg"
          >
            <ChevronRight size={14} className={cn('transition-transform duration-150', showAdvanced && 'rotate-90')} />
            Advanced options
            {!showAdvanced && (
              <span className="ml-auto truncate font-normal text-fg-3 tabular">
                {resumable ? connections : 1} connections · {PRIORITIES.find((p) => p.value === priority)?.label} priority
                {speedLimit > 0 && ` · ${speedLimit} KB/s`}
              </span>
            )}
          </button>
          <AnimatePresence initial={false}>
            {showAdvanced && (
              <motion.div
                initial={{ height: 0, opacity: 0 }}
                animate={{ height: 'auto', opacity: 1 }}
                exit={{ height: 0, opacity: 0 }}
                transition={{ duration: 0.18 }}
                className="overflow-hidden"
              >
                <div className="space-y-4 border-t border-line px-3 pt-3.5 pb-4">
                  <div>
                    <Label hint={!resumable ? 'Server allows only 1' : undefined}>Connections</Label>
                    <div className="flex items-center gap-3">
                      <input
                        type="range"
                        min={1}
                        max={32}
                        value={resumable ? connections : 1}
                        disabled={!resumable}
                        onChange={(e) => setConnections(parseInt(e.target.value, 10))}
                        className="h-1 flex-1 cursor-pointer accent-[var(--accent)] disabled:opacity-40"
                      />
                      <span className="w-6 text-right text-[13px] font-medium text-fg tabular">{resumable ? connections : 1}</span>
                    </div>
                  </div>

                  <div className="grid grid-cols-2 gap-3">
                    <div>
                      <Label>Priority</Label>
                      <Segmented
                        value={priority}
                        onChange={setPriority}
                        className="w-full"
                        options={PRIORITIES.map((p) => ({ value: p.value, label: p.label }))}
                      />
                    </div>
                    <div>
                      <Label>Speed limit</Label>
                      <NumberInput value={speedLimit} onChange={setSpeedLimit} unit="KB/s" placeholder="Unlimited" />
                    </div>
                    <div>
                      <Label>Minimum segment</Label>
                      <Select value={minChunk} onValueChange={(v) => setMinChunk(parseInt(v, 10))} options={CHUNK_SIZES} />
                    </div>
                    <div>
                      <Label>Proxy</Label>
                      <Select
                        value={proxyMode}
                        onValueChange={(v) => setProxyMode(v as AddDownloadRequest['proxy_mode'])}
                        options={[
                          { value: 'system', label: 'System / VPN' },
                          { value: 'direct', label: 'Direct' },
                          { value: 'custom', label: 'Custom…' },
                        ]}
                      />
                    </div>
                  </div>

                  {proxyMode === 'custom' && (
                    <div>
                      <Label>Proxy URL</Label>
                      <Input mono value={proxyUrl} onChange={(e) => setProxyUrl(e.target.value)} placeholder="socks5://127.0.0.1:1080" />
                    </div>
                  )}

                  <div className="grid grid-cols-2 gap-3">
                    <div>
                      <Label>Referer</Label>
                      <Input value={referer} onChange={(e) => setReferer(e.target.value)} placeholder="https://…" />
                    </div>
                    <div>
                      <Label>User agent</Label>
                      <Select value={userAgent} onValueChange={setUserAgent} options={USER_AGENTS} />
                    </div>
                  </div>

                  <div>
                    <Label hint="One per line">Mirrors</Label>
                    <Textarea mono rows={2} value={mirrors} onChange={(e) => setMirrors(e.target.value)} placeholder="https://mirror.example.com/file.iso" />
                  </div>

                  <div>
                    <Label hint="Verified after download">Expected checksum</Label>
                    <Input mono value={checksum} onChange={(e) => setChecksum(e.target.value)} placeholder="SHA-256 or MD5" />
                  </div>
                </div>
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </div>

      <footer className="flex items-center gap-2 border-t border-line px-5 py-3">
        {submitError ? (
          <span className="mr-auto flex min-w-0 items-center gap-1.5 text-[12px] text-danger">
            <TriangleAlert size={13} className="shrink-0" />
            <span className="truncate" title={submitError}>
              {submitError}
            </span>
          </span>
        ) : (
          <span className="mr-auto" />
        )}
        <Button variant="ghost" onClick={onClose}>
          Cancel
        </Button>
        {when === 'now' && queues.length > 0 && (
          <Button
            variant="secondary"
            icon={ListPlus}
            disabled={!canSubmit}
            onClick={() => submit(undefined, true)}
            title="Add to the selected queue without starting. It downloads when that queue starts."
          >
            Add to queue
          </Button>
        )}
        <Button type="submit" variant="primary" icon={Download} loading={submitting} disabled={!canSubmit}>
          {when === 'later' ? 'Schedule' : 'Download'}
        </Button>
      </footer>
    </form>
  );
};

function defaultScheduleValue(time?: string): string {
  const d = new Date();
  const [h, m] = (time || '02:00').split(':').map((n) => parseInt(n, 10));
  d.setHours(h || 0, m || 0, 0, 0);
  if (d.getTime() < Date.now()) d.setDate(d.getDate() + 1);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}
