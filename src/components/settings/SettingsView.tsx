import React, { useEffect, useRef, useState } from 'react';
import { motion } from 'motion/react';
import {
  ArrowLeft,
  Bell,
  CalendarClock,
  Check,
  Copy,
  Folder,
  Key,
  Magnet,
  Monitor,
  Moon,
  Network,
  Plus,
  Puzzle,
  ShieldCheck,
  SlidersHorizontal,
  Sun,
  Trash,
  Zap,
  type LucideIcon,
} from 'lucide-react';
import type { AppSettings, SiteCredential } from '../../types/download';
import { api } from '../../services/api';
import { CHUNK_SIZES, EXTENSION_DIR, GRABBER_PORT } from '../../lib/meta';
import { cn } from '../../lib/cn';
import { Button, IconButton } from '../ui/Button';
import { Input, Label, NumberInput, Segmented, Select, Switch } from '../ui/Field';
import { useToast } from '../ui/Toast';
import { errText } from '../../hooks/useDownloads';

export type SettingsSection =
  | 'general'
  | 'connections'
  | 'network'
  | 'scheduler'
  | 'notifications'
  | 'security'
  | 'torrents'
  | 'logins'
  | 'integrations';

const SECTIONS: { id: SettingsSection; label: string; icon: LucideIcon }[] = [
  { id: 'general', label: 'General', icon: SlidersHorizontal },
  { id: 'connections', label: 'Connections', icon: Zap },
  { id: 'network', label: 'Proxy', icon: Network },
  { id: 'scheduler', label: 'Scheduler', icon: CalendarClock },
  { id: 'notifications', label: 'When finished', icon: Bell },
  { id: 'security', label: 'Security', icon: ShieldCheck },
  { id: 'torrents', label: 'Torrents', icon: Magnet },
  { id: 'logins', label: 'Site logins', icon: Key },
  { id: 'integrations', label: 'Browser', icon: Puzzle },
];

interface SettingsViewProps {
  settings: AppSettings;
  onChange: (patch: Partial<AppSettings>) => void;
  onClose: () => void;
  section: SettingsSection;
  onSection: (s: SettingsSection) => void;
  saveState: 'idle' | 'saving' | 'saved';
}

export const SettingsView: React.FC<SettingsViewProps> = ({ settings, onChange, onClose, section, onSection, saveState }) => {
  const scrollRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: 0 });
  }, [section]);
  const current = SECTIONS.find((s) => s.id === section)!;

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.12 }}
      className="absolute inset-0 z-10 flex bg-canvas"
    >
      <nav className="flex w-[208px] shrink-0 flex-col border-r border-line bg-surface px-2.5 py-3">
        <button
          type="button"
          onClick={onClose}
          className="mb-3 flex h-8 items-center gap-2 rounded-md px-2 text-[13px] font-medium text-fg-2 hover:bg-hover hover:text-fg"
        >
          <ArrowLeft size={15} />
          Back to downloads
        </button>
        <div className="space-y-0.5">
          {SECTIONS.map((s) => {
            const Icon = s.icon;
            const active = s.id === section;
            return (
              <button
                key={s.id}
                type="button"
                onClick={() => onSection(s.id)}
                className={cn(
                  'flex h-8 w-full items-center gap-2.5 rounded-md px-2 text-[13px] transition-colors',
                  active ? 'bg-press font-medium text-fg' : 'text-fg-2 hover:bg-hover hover:text-fg',
                )}
              >
                <Icon size={15} className={active ? 'text-accent' : 'text-fg-3'} />
                {s.label}
              </button>
            );
          })}
        </div>
        <div className="mt-auto flex h-6 items-center gap-1.5 px-2 text-[12px] text-fg-3">
          {saveState === 'saving' && 'Saving…'}
          {saveState === 'saved' && (
            <>
              <Check size={13} className="text-success" /> All changes saved
            </>
          )}
          {saveState === 'idle' && 'Changes save automatically'}
        </div>
      </nav>

      <div ref={scrollRef} className="min-w-0 flex-1 overflow-y-auto">
        <div className="mx-auto max-w-[680px] px-8 pt-7 pb-16">
          <h1 className="mb-5 text-[19px] font-semibold tracking-[-0.01em] text-fg">{current.label}</h1>
          {section === 'general' && <General s={settings} set={onChange} />}
          {section === 'connections' && <Connections s={settings} set={onChange} />}
          {section === 'network' && <Proxy s={settings} set={onChange} />}
          {section === 'scheduler' && <Scheduler s={settings} set={onChange} />}
          {section === 'notifications' && <Notifications s={settings} set={onChange} />}
          {section === 'security' && <Security s={settings} set={onChange} />}
          {section === 'torrents' && <Torrents s={settings} set={onChange} />}
          {section === 'logins' && <Logins />}
          {section === 'integrations' && <Integrations />}
        </div>
      </div>
    </motion.div>
  );
};

/* ───────────── building blocks ───────────── */

const Group: React.FC<{ title?: string; children: React.ReactNode; className?: string }> = ({ title, children, className }) => (
  <section className={cn('mb-6', className)}>
    {title && <h2 className="mb-2 px-0.5 text-[12px] font-medium text-fg-3">{title}</h2>}
    <div className="divide-y divide-line overflow-hidden rounded-xl bg-surface shadow-[0_0_0_1px_var(--line)]">{children}</div>
  </section>
);

const SettingRow: React.FC<{ title: string; description?: React.ReactNode; children?: React.ReactNode; stacked?: boolean }> = ({
  title,
  description,
  children,
  stacked,
}) => (
  <div className={cn('px-4 py-3.5', stacked ? 'space-y-2.5' : 'flex items-center gap-6')}>
    <div className="min-w-0 flex-1">
      <div className="text-[13px] font-medium text-fg">{title}</div>
      {description && <div className="mt-0.5 text-[12.5px] leading-relaxed text-fg-3">{description}</div>}
    </div>
    {children && <div className={cn(!stacked && 'shrink-0')}>{children}</div>}
  </div>
);

type SectionProps = { s: AppSettings; set: (p: Partial<AppSettings>) => void };

const kb = (bytes: number) => (bytes ? Math.round(bytes / 1024) : 0);

/* ───────────── sections ───────────── */

const General: React.FC<SectionProps> = ({ s, set }) => (
  <>
    <Group title="Appearance">
      <SettingRow title="Theme" description="Follow your system or pick one.">
        <Segmented
          value={(s.theme as 'system' | 'dark' | 'light') || 'dark'}
          onChange={(theme) => set({ theme })}
          options={[
            { value: 'system', label: 'System', icon: Monitor },
            { value: 'light', label: 'Light', icon: Sun },
            { value: 'dark', label: 'Dark', icon: Moon },
          ]}
        />
      </SettingRow>
    </Group>
    <Group title="Saving files">
      <SettingRow title="Download folder" description="Where new downloads are saved unless you choose otherwise." stacked>
        <Input icon={Folder} mono value={s.default_destination} onChange={(e) => set({ default_destination: e.target.value })} />
      </SettingRow>
      <SettingRow title="Sort into folders by type" description="Archives, Video, Audio, Documents and Programs each get their own subfolder.">
        <Switch checked={s.auto_categorize} onChange={(v) => set({ auto_categorize: v })} label="Sort into folders by type" />
      </SettingRow>
    </Group>
    <Group title="Behavior">
      <SettingRow title="Resume unfinished downloads on launch">
        <Switch checked={s.auto_resume_on_launch} onChange={(v) => set({ auto_resume_on_launch: v })} label="Resume on launch" />
      </SettingRow>
      <SettingRow title="Watch the clipboard" description="Offer to download when you copy a link to a file.">
        <Switch checked={s.clipboard_watch} onChange={(v) => set({ clipboard_watch: v })} label="Watch the clipboard" />
      </SettingRow>
    </Group>
  </>
);

const Connections: React.FC<SectionProps> = ({ s, set }) => (
  <>
    <Group title="Downloads">
      <SettingRow title="Simultaneous downloads" description="Others wait in the queue.">
        <NumberInput value={s.max_active_downloads} min={1} max={10} onChange={(v) => set({ max_active_downloads: v })} className="w-24" />
      </SettingRow>
      <SettingRow title="Global speed limit" description="Shared across all downloads. Leave empty for unlimited.">
        <NumberInput
          value={kb(s.global_speed_limit)}
          unit="KB/s"
          placeholder="Unlimited"
          onChange={(v) => set({ global_speed_limit: v * 1024 })}
          className="w-36"
        />
      </SettingRow>
    </Group>
    <Group title="Acceleration">
      <SettingRow title="Connections per download" description="More connections are usually faster on servers that allow it.">
        <Select
          value={s.default_connections}
          onValueChange={(v) => set({ default_connections: parseInt(v, 10) })}
          options={[1, 2, 4, 8, 12, 16, 24, 32].map((n) => ({ value: n, label: String(n) }))}
          className="w-24"
        />
      </SettingRow>
      <SettingRow title="Connections per server" description="Cap to avoid being rate-limited by a host.">
        <Select
          value={s.max_connections_per_server || 8}
          onValueChange={(v) => set({ max_connections_per_server: parseInt(v, 10) })}
          options={[1, 2, 4, 8, 12, 16, 24, 32].map((n) => ({ value: n, label: String(n) }))}
          className="w-24"
        />
      </SettingRow>
      <SettingRow title="Minimum segment size" description="Files smaller than this aren't split further.">
        <Select
          value={s.min_chunk_size || 1048576}
          onValueChange={(v) => set({ min_chunk_size: parseInt(v, 10) })}
          options={CHUNK_SIZES}
          className="w-28"
        />
      </SettingRow>
    </Group>
  </>
);

const Proxy: React.FC<SectionProps> = ({ s, set }) => {
  const options: { value: AppSettings['proxy_mode']; title: string; description: string }[] = [
    { value: 'system', title: 'Use system proxy', description: 'Follows Windows proxy and VPN settings. Recommended.' },
    { value: 'direct', title: 'Connect directly', description: 'Ignore all proxies.' },
    { value: 'custom', title: 'Custom proxy', description: 'Route downloads through a specific HTTP or SOCKS5 proxy.' },
  ];
  return (
    <Group>
      {options.map((o) => (
        <label key={o.value} className="flex cursor-default items-start gap-3 px-4 py-3.5 hover:bg-hover">
          <input
            type="radio"
            name="proxy_mode"
            checked={s.proxy_mode === o.value}
            onChange={() => set({ proxy_mode: o.value })}
            className="mt-0.5 h-4 w-4 accent-[var(--accent)]"
          />
          <div className="min-w-0 flex-1">
            <div className="text-[13px] font-medium text-fg">{o.title}</div>
            <div className="mt-0.5 text-[12.5px] text-fg-3">{o.description}</div>
            {o.value === 'custom' && s.proxy_mode === 'custom' && (
              <Input
                mono
                className="mt-2.5"
                value={s.custom_proxy_url ?? ''}
                onChange={(e) => set({ custom_proxy_url: e.target.value })}
                placeholder="http://127.0.0.1:10808 or socks5://127.0.0.1:1080"
              />
            )}
          </div>
        </label>
      ))}
    </Group>
  );
};

const DAYS = [
  { id: 'mon', label: 'Mon' },
  { id: 'tue', label: 'Tue' },
  { id: 'wed', label: 'Wed' },
  { id: 'thu', label: 'Thu' },
  { id: 'fri', label: 'Fri' },
  { id: 'sat', label: 'Sat' },
  { id: 'sun', label: 'Sun' },
];

const Scheduler: React.FC<SectionProps> = ({ s, set }) => {
  const days = s.scheduler_days || [];
  return (
    <>
      <Group>
        <SettingRow title="Run the queue on a schedule" description="Start queued downloads at a set time and stop them later — handy for off-peak hours.">
          <Switch checked={s.scheduler_enabled} onChange={(v) => set({ scheduler_enabled: v })} label="Enable scheduler" />
        </SettingRow>
      </Group>
      <div className={cn('transition-opacity', !s.scheduler_enabled && 'pointer-events-none opacity-45')}>
        <Group title="Window">
          <SettingRow title="Days" stacked>
            <div className="flex gap-1.5">
              {DAYS.map((d) => {
                const on = days.includes(d.id);
                return (
                  <button
                    key={d.id}
                    type="button"
                    onClick={() => set({ scheduler_days: on ? days.filter((x) => x !== d.id) : [...days, d.id] })}
                    className={cn(
                      'h-8 flex-1 rounded-lg text-[12.5px] font-medium transition-colors',
                      on ? 'bg-accent text-accent-fg' : 'bg-surface-2 text-fg-2 shadow-[inset_0_0_0_1px_var(--line)] hover:text-fg',
                    )}
                  >
                    {d.label}
                  </button>
                );
              })}
            </div>
          </SettingRow>
          <SettingRow title="Start at">
            <TimeInput value={s.scheduler_start_time || '02:00'} onChange={(v) => set({ scheduler_start_time: v })} />
          </SettingRow>
          <SettingRow title="Stop at">
            <TimeInput value={s.scheduler_stop_time || '07:00'} onChange={(v) => set({ scheduler_stop_time: v })} />
          </SettingRow>
        </Group>
        <Group title="While scheduled">
          <SettingRow title="Speed limit" description="Applies only inside the window.">
            <NumberInput
              value={kb(s.scheduler_speed_limit)}
              unit="KB/s"
              placeholder="Unlimited"
              onChange={(v) => set({ scheduler_speed_limit: v * 1024 })}
              className="w-36"
            />
          </SettingRow>
          <SettingRow title="When the window ends">
            <Select
              value={s.scheduler_action || 'none'}
              onValueChange={(v) => set({ scheduler_action: v as AppSettings['scheduler_action'] })}
              options={[
                { value: 'none', label: 'Pause the queue' },
                { value: 'sleep', label: 'Sleep computer' },
                { value: 'shutdown', label: 'Shut down computer' },
                { value: 'exit', label: 'Quit Nexdrop' },
              ]}
              className="w-48"
            />
          </SettingRow>
        </Group>
      </div>
    </>
  );
};

const TimeInput: React.FC<{ value: string; onChange: (v: string) => void }> = ({ value, onChange }) => (
  <input
    type="time"
    value={value}
    onChange={(e) => onChange(e.target.value)}
    className="h-8 w-32 rounded-lg border border-line-strong bg-surface-2 px-2.5 text-[13px] text-fg tabular outline-none focus:border-accent"
  />
);

const Notifications: React.FC<SectionProps> = ({ s, set }) => (
  <>
    <Group>
      <SettingRow title="When all downloads finish">
        <Select
          value={s.on_complete_action}
          onValueChange={(v) => set({ on_complete_action: v as AppSettings['on_complete_action'] })}
          options={[
            { value: 'notify', label: 'Notify me' },
            { value: 'none', label: 'Do nothing' },
            { value: 'sleep', label: 'Sleep computer' },
            { value: 'shutdown', label: 'Shut down computer' },
          ]}
          className="w-48"
        />
      </SettingRow>
    </Group>
    <Group title="Notifications">
      <SettingRow title="Desktop notification for each download">
        <Switch checked={s.notify_on_complete} onChange={(v) => set({ notify_on_complete: v })} label="Desktop notifications" />
      </SettingRow>
      <SettingRow title="Play a sound">
        <Switch checked={s.sound_enabled} onChange={(v) => set({ sound_enabled: v })} label="Play a sound" />
      </SettingRow>
    </Group>
  </>
);

const Security: React.FC<SectionProps> = ({ s, set }) => (
  <Group>
    <SettingRow title="Scan finished downloads" description="Run Windows Defender (or your own scanner) on every completed file.">
      <Switch checked={s.scan_with_antivirus} onChange={(v) => set({ scan_with_antivirus: v })} label="Scan downloads" />
    </SettingRow>
    {s.scan_with_antivirus && (
      <SettingRow title="Scanner" description="Leave empty to use Windows Defender." stacked>
        <Input
          mono
          value={s.antivirus_path ?? ''}
          onChange={(e) => set({ antivirus_path: e.target.value || undefined })}
          placeholder="C:\Program Files\Windows Defender\MpCmdRun.exe"
        />
      </SettingRow>
    )}
  </Group>
);

const Torrents: React.FC<SectionProps> = ({ s, set }) => (
  <>
    <Group title="Sharing">
      <SettingRow title="Keep sharing after a torrent finishes" description="Seeding is how the torrent network stays healthy: others download from you while you share.">
        <Switch checked={s.torrent_seed_after_download} onChange={(v) => set({ torrent_seed_after_download: v })} label="Keep sharing after finishing" />
      </SettingRow>
      <SettingRow title="Stop sharing at ratio" description="Uploaded ÷ downloaded. A ratio of 1 means you gave back as much as you took. 0 never stops on its own.">
        <Select
          value={s.torrent_ratio_limit || 0}
          onValueChange={(v) => set({ torrent_ratio_limit: parseFloat(v) })}
          options={[0, 0.5, 1, 1.5, 2, 3, 5]
            .concat(s.torrent_ratio_limit && ![0.5, 1, 1.5, 2, 3, 5].includes(s.torrent_ratio_limit) ? [s.torrent_ratio_limit] : [])
            .sort((a, b) => a - b)
            .map((v) => ({ value: v, label: v === 0 ? 'Never stop' : `${v}×` }))}
          className="w-36"
        />
      </SettingRow>
      <SettingRow title="Upload speed limit" description="Caps how fast you share. Download speed follows the global limit in the status bar.">
        <NumberInput
          value={kb(s.torrent_upload_limit)}
          unit="KB/s"
          placeholder="Unlimited"
          onChange={(v) => set({ torrent_upload_limit: v * 1024 })}
          className="w-36"
        />
      </SettingRow>
    </Group>
    <Group title="Finding peers">
      <SettingRow title="Use DHT" description="Finds peers without a tracker. Needed for magnet links to work reliably.">
        <Switch checked={s.torrent_dht} onChange={(v) => set({ torrent_dht: v })} label="Use DHT" />
      </SettingRow>
      <SettingRow title="Open the port on my router (UPnP)" description="Lets more peers connect to you, which speeds up sharing. Takes effect after restarting Nexdrop.">
        <Switch checked={s.torrent_upnp} onChange={(v) => set({ torrent_upnp: v })} label="Use UPnP" />
      </SettingRow>
    </Group>
    <p className="text-[12.5px] leading-relaxed text-fg-3">
      Torrents connect you directly to other people, so your IP address is visible to them. If that matters, run Nexdrop behind a VPN. Windows may ask to allow
      Nexdrop through the firewall the first time a torrent starts: allow private networks so peers can connect.
    </p>
  </>
);

const Logins: React.FC = () => {
  const toast = useToast();
  const [creds, setCreds] = useState<SiteCredential[]>([]);
  const [adding, setAdding] = useState(false);
  const [form, setForm] = useState({ domain: '', username: '', password: '', token: '', notes: '' });

  const load = () =>
    api
      .getSiteCredentials()
      .then(setCreds)
      .catch(() => {});
  useEffect(() => {
    load();
  }, []);

  const save = async () => {
    if (!form.domain.trim()) return;
    try {
      await api.saveSiteCredential({
        id: `cred_${Date.now()}`,
        domain: form.domain.trim().toLowerCase(),
        username: form.username.trim() || undefined,
        password: form.password || undefined,
        token_or_cookie: form.token || undefined,
        notes: form.notes.trim() || undefined,
      });
      setForm({ domain: '', username: '', password: '', token: '', notes: '' });
      setAdding(false);
      load();
    } catch (err) {
      toast.show({ tone: 'error', title: "Couldn't save login", description: errText(err) });
    }
  };

  const remove = async (id: string) => {
    try {
      await api.deleteSiteCredential(id);
      load();
    } catch (err) {
      toast.show({ tone: 'error', title: "Couldn't remove login", description: errText(err) });
    }
  };

  return (
    <>
      <p className="-mt-2 mb-5 text-[13px] text-fg-2">
        Saved credentials are sent automatically when downloading from these sites — useful for premium file hosts and private servers.
      </p>
      <Group>
        {creds.length === 0 && !adding && <div className="px-4 py-6 text-center text-[12.5px] text-fg-3">No saved logins</div>}
        {creds.map((c) => (
          <div key={c.id} className="group flex items-center gap-3 px-4 py-3">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-surface-2 text-fg-2">
              <Key size={14} />
            </div>
            <div className="min-w-0 flex-1">
              <div className="truncate font-mono text-[12.5px] font-medium text-fg">{c.domain}</div>
              <div className="truncate text-[12px] text-fg-3">
                {c.username ? c.username : c.token_or_cookie ? 'Token / cookie' : 'No credentials'}
                {c.notes && ` · ${c.notes}`}
              </div>
            </div>
            <IconButton size="sm" icon={Trash} label="Remove login" tone="danger" onClick={() => remove(c.id)} />
          </div>
        ))}
        {adding && (
          <div className="space-y-3 bg-surface-2/50 px-4 py-4">
            <div className="grid grid-cols-2 gap-3">
              <div>
                <Label>Domain</Label>
                <Input data-autofocus mono value={form.domain} onChange={(e) => setForm({ ...form, domain: e.target.value })} placeholder="files.example.com" />
              </div>
              <div>
                <Label>Note</Label>
                <Input value={form.notes} onChange={(e) => setForm({ ...form, notes: e.target.value })} placeholder="Optional" />
              </div>
              <div>
                <Label>Username</Label>
                <Input value={form.username} onChange={(e) => setForm({ ...form, username: e.target.value })} autoComplete="off" />
              </div>
              <div>
                <Label>Password</Label>
                <Input type="password" value={form.password} onChange={(e) => setForm({ ...form, password: e.target.value })} autoComplete="new-password" />
              </div>
            </div>
            <div>
              <Label hint="Instead of username & password">Token or cookie</Label>
              <Input mono value={form.token} onChange={(e) => setForm({ ...form, token: e.target.value })} placeholder="sessionid=…" />
            </div>
            <div className="flex justify-end gap-2">
              <Button size="sm" variant="ghost" onClick={() => setAdding(false)}>
                Cancel
              </Button>
              <Button size="sm" variant="primary" onClick={save} disabled={!form.domain.trim()}>
                Save login
              </Button>
            </div>
          </div>
        )}
      </Group>
      {!adding && (
        <Button variant="secondary" icon={Plus} onClick={() => setAdding(true)} className="-mt-3">
          Add login
        </Button>
      )}
    </>
  );
};

const Integrations: React.FC = () => {
  const [copied, setCopied] = useState(false);
  return (
    <>
      <Group>
        <SettingRow title="Browser bridge" description={`Listening on 127.0.0.1:${GRABBER_PORT} for links sent from your browser.`}>
          <span className="flex items-center gap-1.5 rounded-full bg-success/12 px-2.5 py-1 text-[12px] font-medium text-success">
            <span className="nx-pulse h-1.5 w-1.5 rounded-full bg-success" />
            Running
          </span>
        </SettingRow>
      </Group>

      <Group title="Install the extension">
        <div className="space-y-4 px-4 py-4 text-[13px] text-fg-2">
          <Step n={1}>
            Open <code className="rounded bg-surface-2 px-1.5 py-0.5 font-mono text-[12px] text-fg">chrome://extensions</code> or{' '}
            <code className="rounded bg-surface-2 px-1.5 py-0.5 font-mono text-[12px] text-fg">edge://extensions</code>
          </Step>
          <Step n={2}>Turn on Developer mode</Step>
          <Step n={3}>
            Choose <span className="font-medium text-fg">Load unpacked</span> and select this folder:
            <div className="mt-2 flex items-center gap-2 rounded-lg bg-surface-2 py-1 pr-1 pl-3 shadow-[inset_0_0_0_1px_var(--line)]">
              <span className="selectable min-w-0 flex-1 truncate font-mono text-[12px] text-fg">{EXTENSION_DIR}</span>
              <Button
                size="sm"
                variant="ghost"
                icon={copied ? Check : Copy}
                onClick={() =>
                  navigator.clipboard.writeText(EXTENSION_DIR).then(() => {
                    setCopied(true);
                    setTimeout(() => setCopied(false), 1500);
                  })
                }
              >
                {copied ? 'Copied' : 'Copy'}
              </Button>
            </div>
          </Step>
        </div>
      </Group>

      <Group title="What it adds">
        <ul className="space-y-2 px-4 py-4 text-[13px] text-fg-2">
          <li>“Download with Nexdrop” on any link or media in the right-click menu</li>
          <li>“Grab all links on page” to bulk-add files</li>
          <li>A download button over HTML5 video players</li>
          <li>Automatic capture of archives, installers, videos and other large files</li>
        </ul>
      </Group>
    </>
  );
};

const Step: React.FC<{ n: number; children: React.ReactNode }> = ({ n, children }) => (
  <div className="flex gap-3">
    <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-accent-soft text-[11px] font-semibold text-accent">{n}</span>
    <div className="min-w-0 flex-1 pt-px">{children}</div>
  </div>
);
