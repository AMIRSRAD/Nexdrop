import React, { useEffect, useMemo, useRef, useState } from 'react';
import { ClipboardPaste, Download, FileUp, Folder, LoaderCircle, Magnet, TriangleAlert } from 'lucide-react';
import { api } from '../../services/api';
import type { AddTorrentRequest, AppSettings, TorrentFile, TorrentProbeInfo } from '../../types/download';
import { isMagnet, isTorrentUrl } from '../../lib/meta';
import { cn } from '../../lib/cn';
import { formatBytes, plural } from '../../utils/format';
import { Dialog } from '../ui/Dialog';
import { Button } from '../ui/Button';
import { Checkbox, Input, Label } from '../ui/Field';
import { errText } from '../../hooks/useDownloads';

interface AddTorrentDialogProps {
  open: boolean;
  /** A magnet link or link to a .torrent file to look at straight away. */
  initialSource: string;
  /** A .torrent file the user dropped or picked. */
  initialFile: File | null;
  settings: AppSettings;
  onClose: () => void;
  onSubmit: (req: AddTorrentRequest) => Promise<void>;
}

type Probe =
  | { kind: 'idle' }
  | { kind: 'loading'; label: string }
  | { kind: 'ok'; info: TorrentProbeInfo }
  | { kind: 'error'; message: string };

export const AddTorrentDialog: React.FC<AddTorrentDialogProps> = ({ open, onClose, ...rest }) => (
  <Dialog open={open} onClose={onClose} title="Add torrent" width={620} bare>
    {open && <AddTorrentForm onClose={onClose} {...rest} />}
  </Dialog>
);

const AddTorrentForm: React.FC<Omit<AddTorrentDialogProps, 'open'>> = ({ initialSource, initialFile, settings, onClose, onSubmit }) => {
  const [source, setSource] = useState(initialSource);
  const [fileName, setFileName] = useState<string | null>(null);
  const [probe, setProbe] = useState<Probe>({ kind: 'idle' });
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [destination, setDestination] = useState(settings.default_destination);
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const seq = useRef(0);
  const fileRef = useRef<HTMLInputElement>(null);

  const applyInfo = (info: TorrentProbeInfo) => {
    setProbe({ kind: 'ok', info });
    setSelected(new Set(info.files.map((f) => f.index)));
  };

  const probeUrl = async (url: string) => {
    const mine = ++seq.current;
    setProbe({ kind: 'loading', label: isMagnet(url) ? 'Finding peers and reading the file list…' : 'Downloading the .torrent file…' });
    try {
      const info = await api.probeTorrent({ url });
      if (mine === seq.current) applyInfo(info);
    } catch (err) {
      if (mine === seq.current) setProbe({ kind: 'error', message: errText(err) });
    }
  };

  const probeFile = async (file: File) => {
    const mine = ++seq.current;
    setFileName(file.name);
    setSource('');
    setProbe({ kind: 'loading', label: `Reading ${file.name}…` });
    try {
      if (file.size > 20 * 1024 * 1024) throw new Error('That file is too large to be a .torrent');
      const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
      const info = await api.probeTorrent({ bytes });
      if (mine === seq.current) applyInfo(info);
    } catch (err) {
      if (mine === seq.current) setProbe({ kind: 'error', message: errText(err) });
    }
  };

  // Start from whatever opened the dialog
  useEffect(() => {
    if (initialFile) probeFile(initialFile);
    else if (initialSource && (isMagnet(initialSource) || isTorrentUrl(initialSource))) probeUrl(initialSource.trim());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Probe while typing/pasting a link
  useEffect(() => {
    const text = source.trim();
    if (!text || (!isMagnet(text) && !isTorrentUrl(text))) return;
    if (text === initialSource.trim() && probe.kind !== 'idle') return;
    const t = window.setTimeout(() => probeUrl(text), 450);
    return () => window.clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source]);

  const pasteFromClipboard = async () => {
    try {
      const text = (await navigator.clipboard.readText()).trim();
      if (text) {
        setFileName(null);
        setSource(text);
      }
    } catch {
      /* clipboard permission denied */
    }
  };

  const info = probe.kind === 'ok' ? probe.info : null;
  const selectedBytes = useMemo(() => (info ? info.files.filter((f) => selected.has(f.index)).reduce((a, f) => a + f.length, 0) : 0), [info, selected]);
  const allSelected = !!info && selected.size === info.files.length;
  const canSubmit = !!info && selected.size > 0 && !submitting;

  const submit = async (e?: React.FormEvent) => {
    e?.preventDefault();
    if (!info || !canSubmit) return;
    setSubmitting(true);
    setSubmitError(null);
    try {
      await onSubmit({
        info_hash: info.info_hash,
        source: isMagnet(source) || isTorrentUrl(source) ? source.trim() : undefined,
        destination_path: destination.trim() || undefined,
        selected_files: allSelected ? undefined : [...selected].sort((a, b) => a - b),
      });
      onClose();
    } catch (err) {
      setSubmitError(errText(err));
      setSubmitting(false);
    }
  };

  return (
    <form onSubmit={submit} className="flex min-h-0 flex-col">
      <div className="min-h-0 space-y-4 overflow-y-auto px-5 pt-1 pb-5">
        {/* Source */}
        <div>
          <Input
            data-autofocus
            inputSize="lg"
            icon={Magnet}
            value={fileName ?? source}
            readOnly={!!fileName}
            onChange={(e) => {
              setFileName(null);
              setSource(e.target.value);
            }}
            placeholder="magnet:?xt=urn:btih:…  or a link to a .torrent file"
            suffix={
              fileName ? (
                <Button size="sm" variant="ghost" onClick={() => { setFileName(null); setProbe({ kind: 'idle' }); }} className="h-7 px-2 text-fg-3">
                  Clear
                </Button>
              ) : (
                <Button size="sm" variant="ghost" icon={ClipboardPaste} onClick={pasteFromClipboard} className="h-7 px-2 text-fg-3">
                  Paste
                </Button>
              )
            }
          />
          <div className="mt-2 flex items-center gap-2 text-[12px] text-fg-3">
            <input
              ref={fileRef}
              type="file"
              accept=".torrent,application/x-bittorrent"
              className="hidden"
              onChange={(e) => {
                const f = e.target.files?.[0];
                if (f) probeFile(f);
                e.target.value = '';
              }}
            />
            <Button size="sm" variant="secondary" icon={FileUp} onClick={() => fileRef.current?.click()}>
              Choose .torrent file
            </Button>
            <span>or drop one anywhere on the window</span>
          </div>
        </div>

        {/* Result */}
        {probe.kind === 'idle' && (
          <div className="rounded-lg bg-surface-2 p-4 text-[12.5px] text-fg-2 shadow-[inset_0_0_0_1px_var(--line)]">
            Paste a magnet link or choose a .torrent file. You'll see the files inside and can pick which ones to download.
          </div>
        )}

        {probe.kind === 'loading' && (
          <div className="flex items-center gap-3 rounded-lg bg-surface-2 p-4 shadow-[inset_0_0_0_1px_var(--line)]">
            <LoaderCircle size={18} className="animate-spin text-accent" />
            <div>
              <div className="text-[13px] font-medium text-fg">{probe.label}</div>
              <div className="text-[12px] text-fg-3">Magnet links can take up to a minute while peers are found.</div>
            </div>
          </div>
        )}

        {probe.kind === 'error' && (
          <div className="flex items-start gap-2.5 rounded-lg bg-danger/10 p-3.5 text-[12.5px] text-danger">
            <TriangleAlert size={16} className="mt-px shrink-0" />
            <span className="selectable">{probe.message}</span>
          </div>
        )}

        {info && (
          <>
            <div className="rounded-lg bg-surface-2 shadow-[inset_0_0_0_1px_var(--line)]">
              <div className="flex items-center gap-3 border-b border-line px-3.5 py-3">
                <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-accent-soft text-accent">
                  <Magnet size={20} />
                </div>
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[13.5px] font-medium text-fg" title={info.name}>
                    {info.name}
                  </div>
                  <div className="text-[12px] text-fg-3 tabular">
                    {plural(info.files.length, 'file')} · {formatBytes(info.total_bytes)} ·{' '}
                    <span className="font-mono">{info.info_hash.slice(0, 8)}…</span>
                  </div>
                </div>
              </div>

              <div className="flex items-center gap-2.5 border-b border-line px-3.5 py-2 text-[12px] text-fg-2">
                <Checkbox
                  checked={allSelected}
                  indeterminate={!allSelected && selected.size > 0}
                  onChange={(v) => setSelected(v ? new Set(info.files.map((f) => f.index)) : new Set())}
                  label="Select all files"
                />
                <span className="tabular">
                  {selected.size} of {info.files.length} selected · {formatBytes(selectedBytes)}
                </span>
              </div>

              <div className="max-h-[230px] overflow-y-auto p-1">
                {info.files.map((f) => (
                  <FileRow
                    key={f.index}
                    file={f}
                    on={selected.has(f.index)}
                    toggle={() =>
                      setSelected((prev) => {
                        const next = new Set(prev);
                        if (next.has(f.index)) next.delete(f.index);
                        else next.add(f.index);
                        return next;
                      })
                    }
                  />
                ))}
              </div>
            </div>

            <div>
              <Label>Save to</Label>
              <Input icon={Folder} mono value={destination} onChange={(e) => setDestination(e.target.value)} placeholder="Downloads" />
              <p className="mt-1.5 text-[11.5px] text-fg-3">
                {info.files.length > 1 ? `Files go into a folder called “${info.name}”.` : 'The file is saved directly in this folder.'}{' '}
                {settings.torrent_seed_after_download
                  ? 'After it finishes, Nexdrop keeps sharing it with other peers (change this in Settings → Torrents).'
                  : 'Sharing after the download is turned off.'}
              </p>
            </div>
          </>
        )}
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
          <span className="mr-auto text-[11.5px] text-fg-3">Your IP address is visible to other peers while a torrent is active.</span>
        )}
        <Button variant="ghost" onClick={onClose}>
          Cancel
        </Button>
        <Button type="submit" variant="primary" icon={Download} loading={submitting} disabled={!canSubmit}>
          Download
        </Button>
      </footer>
    </form>
  );
};

const FileRow: React.FC<{ file: TorrentFile; on: boolean; toggle: () => void }> = ({ file, on, toggle }) => {
  const parts = file.path.split('/');
  const name = parts.pop() ?? file.path;
  const dir = parts.join('/');
  return (
    <div onClick={toggle} className={cn('flex h-9 items-center gap-2.5 rounded-md px-2.5', on ? 'hover:bg-hover' : 'opacity-55 hover:bg-hover hover:opacity-80')}>
      <Checkbox checked={on} onChange={toggle} label={`Download ${file.path}`} />
      <div className="min-w-0 flex-1 truncate text-[12.5px]" title={file.path}>
        {dir && <span className="text-fg-3">{dir}/</span>}
        <span className="font-medium text-fg">{name}</span>
      </div>
      <span className="shrink-0 text-[12px] text-fg-3 tabular">{formatBytes(file.length)}</span>
    </div>
  );
};
