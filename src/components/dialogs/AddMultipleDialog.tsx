import React, { useEffect, useMemo, useRef, useState } from 'react';
import { FileUp, Folder, Globe, Hash, ListPlus, Search, Sparkles } from 'lucide-react';
import { api } from '../../services/api';
import type { AddDownloadRequest, AppSettings, BrowserRequest, GrabbedLink } from '../../types/download';
import { CATEGORIES, categoryMeta, extractUrls, isHttpUrl } from '../../lib/meta';
import { cn } from '../../lib/cn';
import { Dialog } from '../ui/Dialog';
import { Button } from '../ui/Button';
import { Checkbox, Input, Label, NumberInput, Segmented, Textarea } from '../ui/Field';
import { FileTile } from '../transfers/FileTile';
import { errText } from '../../hooks/useDownloads';
import { plural } from '../../utils/format';

export type AddMultipleTab = 'paste' | 'pattern' | 'page';

interface AddMultipleDialogProps {
  open: boolean;
  initialTab: AddMultipleTab;
  initialText: string;
  /** Links handed over by the browser (the page's links, or the ones the user selected). */
  initialLinks?: GrabbedLink[];
  /** "all": preselect every link. "files": only links that look like files. */
  preselect?: 'all' | 'files';
  /** Referer, user agent and cookies from the browser. */
  initialRequest?: BrowserRequest;
  settings: AppSettings;
  onClose: () => void;
  onSubmit: (reqs: AddDownloadRequest[]) => Promise<void>;
}

export const AddMultipleDialog: React.FC<AddMultipleDialogProps> = ({ open, onClose, ...rest }) => (
  <Dialog open={open} onClose={onClose} title="Add multiple downloads" width={640} bare>
    {open && <AddMultipleBody onClose={onClose} {...rest} />}
  </Dialog>
);

const AddMultipleBody: React.FC<Omit<AddMultipleDialogProps, 'open'>> = ({ initialTab, initialText, initialLinks, preselect, initialRequest, settings, onClose, onSubmit }) => {
  const [tab, setTab] = useState<AddMultipleTab>(initialTab);
  const [destination, setDestination] = useState(settings.default_destination);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const firstTab = useRef(true);

  // Move focus into the newly selected source (initial focus is handled by the dialog)
  useEffect(() => {
    if (firstTab.current) {
      firstTab.current = false;
      return;
    }
    panelRef.current?.querySelector<HTMLElement>('[data-autofocus]')?.focus();
  }, [tab]);

  // Paste
  const [text, setText] = useState(initialText);
  const pastedUrls = useMemo(() => extractUrls(text), [text]);
  const fileRef = useRef<HTMLInputElement>(null);

  // Pattern
  const [template, setTemplate] = useState('https://example.com/files/part-[*].zip');
  const [from, setFrom] = useState(1);
  const [to, setTo] = useState(10);
  const [pad, setPad] = useState(2);
  const patternUrls = useMemo(() => {
    const lo = Math.min(from, to);
    const hi = Math.min(Math.max(from, to), lo + 9999);
    const out: string[] = [];
    for (let i = lo; i <= hi; i++) {
      const n = String(i).padStart(pad, '0');
      out.push(/\[[^\]]*\]/.test(template) ? template.replace(/\[[^\]]*\]/, n) : `${template}${n}`);
    }
    return out.filter(isHttpUrl);
  }, [template, from, to, pad]);

  // Page grabber
  const [pageUrl, setPageUrl] = useState('');
  const [scanning, setScanning] = useState(false);
  const [links, setLinks] = useState<GrabbedLink[] | null>(initialLinks ?? null);
  const [picked, setPicked] = useState<Set<string>>(() => {
    if (!initialLinks) return new Set();
    const files = initialLinks.filter((l) => l.category !== 'other' || preselect === 'all');
    // Nothing looks like a file: offer everything rather than an empty selection.
    return new Set((preselect === 'all' || files.length === 0 ? initialLinks : files).map((l) => l.url));
  });
  const [catFilter, setCatFilter] = useState<string>('all');
  const [query, setQuery] = useState('');

  const visibleLinks = useMemo(
    () =>
      (links ?? []).filter((l) => {
        if (catFilter !== 'all' && categoryMeta(l.category).id !== catFilter) return false;
        const q = query.trim().toLowerCase();
        return !q || l.filename.toLowerCase().includes(q) || l.url.toLowerCase().includes(q);
      }),
    [links, catFilter, query],
  );
  const availableCats = useMemo(() => new Set((links ?? []).map((l) => categoryMeta(l.category).id)), [links]);

  const scan = async (e?: React.FormEvent) => {
    e?.preventDefault();
    let target = pageUrl.trim();
    if (!target || scanning) return;
    if (!/^https?:\/\//i.test(target)) {
      target = `https://${target}`;
      setPageUrl(target);
    }
    setScanning(true);
    setError(null);
    setLinks(null);
    try {
      const res = await api.grabPageLinks(target);
      setLinks(res);
      setPicked(new Set(res.map((l) => l.url)));
      setCatFilter('all');
    } catch (err) {
      setError(`Couldn't scan that page — ${errText(err)}`);
    } finally {
      setScanning(false);
    }
  };

  const loadFile = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => setText((prev) => [prev.trim(), String(reader.result ?? '')].filter(Boolean).join('\n'));
    reader.readAsText(file);
    e.target.value = '';
  };

  const requests: AddDownloadRequest[] = useMemo(() => {
    const base = {
      destination_path: destination.trim() || undefined,
      connections: settings.default_connections || 8,
      // From the browser: the page it was on and the login it used (host-limited by the engine).
      referer: initialRequest?.referer,
      user_agent: initialRequest?.userAgent,
      custom_headers: initialRequest?.headers && Object.keys(initialRequest.headers).length ? initialRequest.headers : undefined,
    };
    if (tab === 'paste') return pastedUrls.map((url) => ({ url, ...base }));
    if (tab === 'pattern') return patternUrls.map((url) => ({ url, ...base }));
    return (links ?? []).filter((l) => picked.has(l.url)).map((l) => ({ url: l.url, filename: l.filename, ...base }));
  }, [tab, pastedUrls, patternUrls, links, picked, destination, settings.default_connections, initialRequest]);

  const submit = async () => {
    if (!requests.length || submitting) return;
    setSubmitting(true);
    setError(null);
    try {
      await onSubmit(requests);
      onClose();
    } catch (err) {
      setError(errText(err));
      setSubmitting(false);
    }
  };

  const visibleAllPicked = visibleLinks.length > 0 && visibleLinks.every((l) => picked.has(l.url));

  return (
    <div className="flex min-h-0 flex-col">
      <div className="px-5 pb-3">
        <Segmented
          value={tab}
          onChange={(t) => {
            setTab(t);
            setError(null);
          }}
          className="w-full"
          options={[
            { value: 'paste', label: 'Paste links', icon: ListPlus },
            { value: 'pattern', label: 'Number sequence', icon: Hash },
            { value: 'page', label: 'From a web page', icon: Globe },
          ]}
        />
      </div>

      <div ref={panelRef} className="h-[340px] min-h-0 overflow-y-auto px-5 pb-4">
        {tab === 'paste' && (
          <div className="flex h-full flex-col">
            <Textarea
              data-autofocus
              mono
              value={text}
              onChange={(e) => setText(e.target.value)}
              placeholder={'Paste links, one per line — or any text containing links.\n\nhttps://example.com/file-1.zip\nhttps://example.com/file-2.zip'}
              className="min-h-0 flex-1"
            />
            <div className="mt-2 flex items-center justify-between text-[12px]">
              <span className={cn('tabular', pastedUrls.length ? 'text-fg-2' : 'text-fg-3')}>
                {pastedUrls.length ? `${plural(pastedUrls.length, 'link')} found` : 'No links yet'}
              </span>
              <input ref={fileRef} type="file" accept=".txt,.csv,.text,.list" className="hidden" onChange={loadFile} />
              <Button size="sm" variant="ghost" icon={FileUp} onClick={() => fileRef.current?.click()}>
                Import from file
              </Button>
            </div>
          </div>
        )}

        {tab === 'pattern' && (
          <div className="space-y-4">
            <div>
              <Label hint={<span>Use <code className="font-mono text-fg-2">[*]</code> where the number goes</span>}>Address template</Label>
              <Input data-autofocus mono value={template} onChange={(e) => setTemplate(e.target.value)} />
            </div>
            <div className="grid grid-cols-3 gap-3">
              <div>
                <Label>From</Label>
                <NumberInput value={from} onChange={setFrom} min={0} max={99999} />
              </div>
              <div>
                <Label>To</Label>
                <NumberInput value={to} onChange={setTo} min={0} max={99999} />
              </div>
              <div>
                <Label>Digits</Label>
                <NumberInput value={pad} onChange={setPad} min={1} max={8} />
              </div>
            </div>
            <div>
              <Label hint={plural(patternUrls.length, 'link')}>Preview</Label>
              <div className="rounded-lg bg-surface-2 px-3 py-2 font-mono text-[11.5px] leading-6 text-fg-2 shadow-[inset_0_0_0_1px_var(--line)]">
                {patternUrls.length === 0 ? (
                  <span className="text-fg-3">Template must be an http(s) address</span>
                ) : (
                  <>
                    {patternUrls.slice(0, 3).map((u) => (
                      <div key={u} className="truncate">
                        {u}
                      </div>
                    ))}
                    {patternUrls.length > 4 && <div className="text-fg-3">⋯ {patternUrls.length - 4} more</div>}
                    {patternUrls.length > 3 && <div className="truncate">{patternUrls[patternUrls.length - 1]}</div>}
                  </>
                )}
              </div>
            </div>
          </div>
        )}

        {tab === 'page' && (
          <div className="flex h-full flex-col">
            <form onSubmit={scan} className="flex gap-2">
              <Input
                data-autofocus
                icon={Globe}
                value={pageUrl}
                onChange={(e) => setPageUrl(e.target.value)}
                placeholder="https://releases.example.com/downloads/"
                className="flex-1"
              />
              <Button type="submit" variant="primary" icon={Sparkles} loading={scanning} disabled={!pageUrl.trim()}>
                Find files
              </Button>
            </form>

            {links === null ? (
              <div className="flex flex-1 flex-col items-center justify-center text-center text-[12.5px] text-fg-3">
                {scanning ? 'Scanning page for downloadable files…' : 'Nexdrop will list every file, archive and media link on the page.'}
              </div>
            ) : links.length === 0 ? (
              <div className="flex flex-1 items-center justify-center text-[12.5px] text-fg-3">No downloadable files found on that page.</div>
            ) : (
              <div className="mt-3 flex min-h-0 flex-1 flex-col rounded-lg shadow-[inset_0_0_0_1px_var(--line)]">
                <div className="flex items-center gap-2 border-b border-line px-3 py-2">
                  <Checkbox
                    checked={visibleAllPicked}
                    indeterminate={!visibleAllPicked && visibleLinks.some((l) => picked.has(l.url))}
                    onChange={(v) =>
                      setPicked((prev) => {
                        const next = new Set(prev);
                        visibleLinks.forEach((l) => (v ? next.add(l.url) : next.delete(l.url)));
                        return next;
                      })
                    }
                    label="Select all"
                  />
                  <div className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto">
                    {[{ id: 'all', label: 'All' }, ...CATEGORIES.filter((c) => availableCats.has(c.id))].map((c) => (
                      <button
                        key={c.id}
                        type="button"
                        onClick={() => setCatFilter(c.id)}
                        className={cn(
                          'h-6 shrink-0 rounded-md px-2 text-[12px] font-medium',
                          catFilter === c.id ? 'bg-surface-3 text-fg' : 'text-fg-3 hover:text-fg-2',
                        )}
                      >
                        {c.label}
                      </button>
                    ))}
                  </div>
                  <div className="relative">
                    <Search size={13} className="pointer-events-none absolute top-1/2 left-2 -translate-y-1/2 text-fg-3" />
                    <input
                      value={query}
                      onChange={(e) => setQuery(e.target.value)}
                      placeholder="Filter"
                      className="h-6 w-32 rounded-md bg-surface-2 pr-2 pl-7 text-[12px] outline-none focus:shadow-[inset_0_0_0_1px_var(--accent)]"
                    />
                  </div>
                </div>
                <div className="min-h-0 flex-1 overflow-y-auto p-1">
                  {visibleLinks.map((l) => {
                    const on = picked.has(l.url);
                    return (
                      <div
                        key={l.url}
                        onClick={() =>
                          setPicked((prev) => {
                            const next = new Set(prev);
                            if (on) next.delete(l.url);
                            else next.add(l.url);
                            return next;
                          })
                        }
                        className={cn('flex h-11 items-center gap-3 rounded-md px-2', on ? 'bg-accent-soft' : 'hover:bg-hover')}
                      >
                        <Checkbox
                          checked={on}
                          onChange={(v) =>
                            setPicked((prev) => {
                              const next = new Set(prev);
                              if (v) next.add(l.url);
                              else next.delete(l.url);
                              return next;
                            })
                          }
                          label={`Select ${l.filename}`}
                        />
                        <FileTile category={l.category} size={26} />
                        <div className="min-w-0 flex-1">
                          <div className="truncate text-[12.5px] font-medium text-fg">{l.filename}</div>
                          <div className="truncate font-mono text-[11px] text-fg-3">{l.url}</div>
                        </div>
                        {l.extension && (
                          <span className="shrink-0 rounded bg-surface-3 px-1.5 font-mono text-[10.5px] leading-[18px] text-fg-2 uppercase">
                            {l.extension}
                          </span>
                        )}
                      </div>
                    );
                  })}
                </div>
              </div>
            )}
          </div>
        )}
      </div>

      <footer className="flex items-center gap-3 border-t border-line px-5 py-3">
        <Input icon={Folder} mono value={destination} onChange={(e) => setDestination(e.target.value)} className="min-w-0 flex-1" aria-label="Save to" />
        {error && (
          <span className="max-w-[160px] truncate text-[12px] text-danger" title={error}>
            {error}
          </span>
        )}
        <Button variant="ghost" onClick={onClose}>
          Cancel
        </Button>
        <Button variant="primary" onClick={submit} loading={submitting} disabled={requests.length === 0}>
          {requests.length ? `Add ${plural(requests.length, 'download')}` : 'Add downloads'}
        </Button>
      </footer>
    </div>
  );
};
