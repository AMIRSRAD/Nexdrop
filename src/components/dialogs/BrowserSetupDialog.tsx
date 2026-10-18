import React, { useEffect, useState } from 'react';
import { Check, Copy, Folder, Globe, LoaderCircle } from 'lucide-react';
import type { BrowserInfo, BrowserIntegration, BrowserSetupOutcome } from '../../types/download';
import { api } from '../../services/api';
import { Dialog } from '../ui/Dialog';
import { Button } from '../ui/Button';
import { Checkbox } from '../ui/Field';
import { cn } from '../../lib/cn';
import { errText } from '../../hooks/useDownloads';

interface Props {
  open: boolean;
  onClose: () => void;
  integration: BrowserIntegration | null;
}

type Step = { browser: BrowserInfo; outcome?: BrowserSetupOutcome; error?: string };

/** "Connect your browsers": pick the browsers, Nexdrop opens each one's extensions page, then waits for them to connect. */
export const BrowserSetupDialog: React.FC<Props> = ({ open, onClose, integration }) => {
  const installed = (integration?.browsers ?? []).filter((b) => b.installed);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [steps, setSteps] = useState<Step[] | null>(null);
  const [busy, setBusy] = useState(false);

  // Start from the browsers that aren't connected yet.
  useEffect(() => {
    if (!open) return;
    setSteps(null);
    setBusy(false);
    setPicked(new Set(installed.filter((b) => !b.connected).map((b) => b.id)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const live = (id: string) => integration?.browsers.find((b) => b.id === id);

  const start = async () => {
    setBusy(true);
    const chosen = installed.filter((b) => picked.has(b.id));
    const results: Step[] = [];
    for (const browser of chosen) {
      try {
        results.push({ browser, outcome: await api.setupBrowser(browser.id) });
      } catch (err) {
        results.push({ browser, error: errText(err) });
      }
    }
    setSteps(results);
    setBusy(false);
  };

  const allConnected = steps?.every((s) => live(s.browser.id)?.connected);

  return (
    <Dialog
      open={open}
      onClose={onClose}
      width={520}
      title="Connect your browsers"
      description="Nexdrop catches downloads and videos in your browser through a small extension."
      footer={
        steps ? (
          <>
            <span className="mr-auto text-[12.5px] text-fg-3">{allConnected ? 'All set.' : 'This page updates by itself once a browser connects.'}</span>
            <Button variant="primary" onClick={onClose}>
              {allConnected ? 'Done' : 'Close'}
            </Button>
          </>
        ) : (
          <>
            <span className="mr-auto" />
            <Button variant="ghost" onClick={onClose}>
              Not now
            </Button>
            <Button variant="primary" loading={busy} disabled={picked.size === 0} onClick={start}>
              {picked.size > 1 ? `Set up ${picked.size} browsers` : 'Set up'}
            </Button>
          </>
        )
      }
    >
      {!steps ? (
        installed.length === 0 ? (
          <p className="text-[13px] text-fg-2">No supported browser (Chrome, Edge, Brave or Firefox) was found on this computer.</p>
        ) : (
          <div className="space-y-2">
            {installed.map((b) => {
              const state = live(b.id) ?? b;
              return (
                <label
                  key={b.id}
                  className="flex cursor-default items-center gap-3 rounded-lg bg-surface-2 px-3 py-2.5 shadow-[inset_0_0_0_1px_var(--line)]"
                >
                  <Checkbox
                    checked={picked.has(b.id)}
                    onChange={(on) =>
                      setPicked((prev) => {
                        const next = new Set(prev);
                        if (on) next.add(b.id);
                        else next.delete(b.id);
                        return next;
                      })
                    }
                    label={`Set up ${b.name}`}
                  />
                  <Globe size={16} className="text-fg-3" />
                  <span className="flex-1 text-[13px] font-medium text-fg">{b.name}</span>
                  <Status info={state} />
                </label>
              );
            })}
            <p className="pt-1 text-[12px] text-fg-3">
              Nexdrop opens each browser’s extensions page and shows what to click. Nothing is installed without your say-so in the browser.
            </p>
          </div>
        )
      ) : (
        <div className="space-y-3">
          {steps.map((s) => (
            <StepCard key={s.browser.id} step={s} info={live(s.browser.id)} folder={integration?.extension_dir ?? undefined} />
          ))}
        </div>
      )}
    </Dialog>
  );
};

const Status: React.FC<{ info: BrowserInfo }> = ({ info }) =>
  info.connected ? (
    <span className="flex items-center gap-1.5 text-[12px] font-medium text-success">
      <Check size={13} /> Connected
    </span>
  ) : info.ever_connected ? (
    <span className="text-[12px] text-fg-3">Connected earlier</span>
  ) : (
    <span className="text-[12px] text-fg-3">Not connected</span>
  );

const StepCard: React.FC<{ step: Step; info?: BrowserInfo; folder?: string }> = ({ step, info, folder }) => {
  const [copied, setCopied] = useState(false);
  const { browser, outcome, error } = step;
  const connected = !!info?.connected;
  const dir = outcome?.folder ?? folder;

  return (
    <div className={cn('rounded-lg p-3.5 shadow-[inset_0_0_0_1px_var(--line)]', connected ? 'bg-success/8' : 'bg-surface-2')}>
      <div className="mb-2 flex items-center gap-2">
        <span className="flex-1 text-[13px] font-semibold text-fg">{browser.name}</span>
        {connected ? (
          <span className="flex items-center gap-1.5 text-[12px] font-medium text-success">
            <Check size={13} /> Connected
          </span>
        ) : !error ? (
          <span className="flex items-center gap-1.5 text-[12px] text-fg-3">
            <LoaderCircle size={13} className="animate-spin" /> Waiting for {browser.name}…
          </span>
        ) : null}
      </div>
      {error && <p className="text-[12.5px] text-danger">{error}</p>}
      {!connected && outcome?.mode === 'unpacked' && (
        <ol className="space-y-1.5 text-[12.5px] text-fg-2">
          <li>1. On the extensions page that just opened, turn on <span className="font-medium text-fg">Developer mode</span> (top right).</li>
          <li>2. Click <span className="font-medium text-fg">Load unpacked</span> and choose this folder:</li>
        </ol>
      )}
      {!connected && outcome?.mode === 'xpi' && (
        <p className="text-[12.5px] text-fg-2">
          {browser.name} is asking to add Nexdrop. Click <span className="font-medium text-fg">Add</span> in its prompt.
        </p>
      )}
      {!connected && outcome?.mode === 'temporary' && (
        <ol className="space-y-1.5 text-[12.5px] text-fg-2">
          <li>1. On the page that just opened, click <span className="font-medium text-fg">Load Temporary Add-on…</span></li>
          <li>2. Choose <span className="font-mono text-[12px] text-fg">manifest.json</span> in this folder:</li>
          <li className="text-fg-3">Firefox forgets temporary add-ons when it closes. A permanent install needs the add-on signed by Mozilla.</li>
        </ol>
      )}
      {!connected && dir && (outcome?.mode === 'unpacked' || outcome?.mode === 'temporary') && (
        <div className="mt-2 flex items-center gap-2 rounded-lg bg-canvas py-1 pr-1 pl-3 shadow-[inset_0_0_0_1px_var(--line)]">
          <span className="selectable min-w-0 flex-1 truncate font-mono text-[12px] text-fg">{dir}</span>
          <Button
            size="sm"
            variant="ghost"
            icon={copied ? Check : Copy}
            onClick={() =>
              navigator.clipboard.writeText(dir).then(() => {
                setCopied(true);
                setTimeout(() => setCopied(false), 1500);
              })
            }
          >
            {copied ? 'Copied' : 'Copy'}
          </Button>
          <Button size="sm" variant="ghost" icon={Folder} onClick={() => api.openExtensionDir()}>
            Open
          </Button>
        </div>
      )}
    </div>
  );
};
