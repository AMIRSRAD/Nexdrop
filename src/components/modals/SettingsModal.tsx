import React, { useState, useEffect } from 'react';
import { motion, AnimatePresence } from 'motion/react';
import {
  X,
  Settings,
  Folder,
  Network,
  Gauge,
  Bell,
  Save,
  Check,
  Shield,
  Volume2,
  Power,
  FolderTree,
  Clock,
  Calendar,
  Key,
  Plus,
  Trash2,
  Lock,
  ShieldCheck,
  Compass,
  Globe,
} from 'lucide-react';
import { AppSettings, SiteCredential } from '../../types/download';
import { api } from '../../services/api';

interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
  settings: AppSettings;
  onSave: (settings: AppSettings) => Promise<void>;
}

export const SettingsModal: React.FC<SettingsModalProps> = ({
  isOpen,
  onClose,
  settings: initialSettings,
  onSave,
}) => {
  const [settings, setSettings] = useState<AppSettings>(initialSettings);
  const [saved, setSaved] = useState(false);

  // Site Credentials state
  const [credentials, setCredentials] = useState<SiteCredential[]>([]);
  const [isAddingCred, setIsAddingCred] = useState(false);
  const [newDomain, setNewDomain] = useState('');
  const [newUsername, setNewUsername] = useState('');
  const [newPassword, setNewPassword] = useState('');
  const [newToken, setNewToken] = useState('');
  const [newNotes, setNewNotes] = useState('');

  useEffect(() => {
    if (isOpen) {
      setSettings(initialSettings);
      api.getSiteCredentials().then(setCredentials).catch(() => {});
    }
  }, [isOpen, initialSettings]);

  const handleAddCredential = async () => {
    if (!newDomain.trim()) return;
    const cred: SiteCredential = {
      id: `cred_${Date.now()}`,
      domain: newDomain.trim().toLowerCase(),
      username: newUsername.trim() || undefined,
      password: newPassword || undefined,
      token_or_cookie: newToken || undefined,
      notes: newNotes.trim() || undefined,
    };
    await api.saveSiteCredential(cred);
    setNewDomain('');
    setNewUsername('');
    setNewPassword('');
    setNewToken('');
    setNewNotes('');
    setIsAddingCred(false);
    const updated = await api.getSiteCredentials();
    setCredentials(updated);
  };

  const handleDeleteCredential = async (id: string) => {
    await api.deleteSiteCredential(id);
    const updated = await api.getSiteCredentials();
    setCredentials(updated);
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    await onSave(settings);
    setSaved(true);
    setTimeout(() => {
      setSaved(false);
      onClose();
    }, 400);
  };

  if (!isOpen) return null;

  return (
    <AnimatePresence>
      <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/75 backdrop-blur-md">
        <motion.div
          initial={{ opacity: 0, scale: 0.95, y: 10 }}
          animate={{ opacity: 1, scale: 1, y: 0 }}
          exit={{ opacity: 0, scale: 0.95, y: 10 }}
          className="w-full max-w-xl rounded-3xl bg-slate-900 border border-white/10 shadow-2xl overflow-hidden"
        >
          {/* Header */}
          <div className="flex items-center justify-between px-6 py-4 border-b border-white/5">
            <div className="flex items-center gap-2">
              <div className="flex h-7 w-7 items-center justify-center rounded-lg bg-indigo-500/20 text-indigo-400">
                <Settings size={16} />
              </div>
              <h2 className="text-sm font-semibold text-white">Nexdrop Preferences</h2>
            </div>
            <button
              onClick={onClose}
              className="p-1 rounded-lg text-slate-400 hover:text-white hover:bg-slate-800 transition-colors"
            >
              <X size={16} />
            </button>
          </div>

          <form onSubmit={handleSubmit} className="p-6 space-y-5 max-h-[80vh] overflow-y-auto">
            {/* Download Location & Categorization */}
            <div className="space-y-3">
              <div>
                <label className="flex items-center gap-2 text-xs font-medium text-slate-300 mb-1.5">
                  <Folder size={14} className="text-indigo-400" />
                  <span>Default Download Directory</span>
                </label>
                <input
                  type="text"
                  value={settings.default_destination}
                  onChange={(e) =>
                    setSettings({ ...settings, default_destination: e.target.value })
                  }
                  className="w-full bg-slate-950/80 border border-white/10 rounded-xl px-3.5 py-2 text-xs text-slate-100 placeholder:text-slate-500 focus:outline-none focus:border-indigo-500"
                />
              </div>

              <label className="flex items-center gap-3 cursor-pointer text-xs text-slate-300 p-2.5 rounded-xl bg-slate-950/40 border border-white/5">
                <FolderTree size={16} className="text-indigo-400 shrink-0" />
                <div className="flex-1">
                  <div className="font-medium text-slate-200">Smart Category Subfolders</div>
                  <div className="text-[11px] text-slate-400">
                    Automatically sort downloads into Compressed, Videos, Music, Documents, Programs folders
                  </div>
                </div>
                <input
                  type="checkbox"
                  checked={settings.auto_categorize}
                  onChange={(e) =>
                    setSettings({ ...settings, auto_categorize: e.target.checked })
                  }
                  className="rounded accent-indigo-500 w-4 h-4"
                />
              </label>
            </div>

            {/* Network & Concurrency */}
            <div className="p-4 rounded-2xl bg-slate-950/50 border border-white/5 space-y-4">
              <div className="flex items-center gap-2 text-xs font-semibold text-slate-200">
                <Network size={14} className="text-indigo-400" />
                <span>Network & Acceleration</span>
              </div>

              <div className="grid grid-cols-2 gap-4">
                <div>
                  <label className="block text-[11px] text-slate-400 mb-1">
                    Max Simultaneous Downloads
                  </label>
                  <input
                    type="number"
                    min={1}
                    max={10}
                    value={settings.max_active_downloads}
                    onChange={(e) =>
                      setSettings({
                        ...settings,
                        max_active_downloads: parseInt(e.target.value, 10) || 3,
                      })
                    }
                    className="w-full bg-slate-900 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 focus:outline-none focus:border-indigo-500"
                  />
                </div>

                <div>
                  <label className="block text-[11px] text-slate-400 mb-1">
                    Default Streams Per File
                  </label>
                  <input
                    type="number"
                    min={1}
                    max={32}
                    value={settings.default_connections}
                    onChange={(e) =>
                      setSettings({
                        ...settings,
                        default_connections: parseInt(e.target.value, 10) || 8,
                      })
                    }
                    className="w-full bg-slate-900 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 focus:outline-none focus:border-indigo-500"
                  />
                </div>

                <div>
                  <label className="block text-[11px] text-slate-400 mb-1">
                    Max Streams Per Server
                  </label>
                  <input
                    type="number"
                    min={1}
                    max={32}
                    value={settings.max_connections_per_server || 8}
                    onChange={(e) =>
                      setSettings({
                        ...settings,
                        max_connections_per_server: parseInt(e.target.value, 10) || 8,
                      })
                    }
                    className="w-full bg-slate-900 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 focus:outline-none focus:border-indigo-500"
                  />
                </div>

                <div>
                  <label className="block text-[11px] text-slate-400 mb-1">
                    Min Segment Threshold
                  </label>
                  <select
                    value={settings.min_chunk_size || 1048576}
                    onChange={(e) =>
                      setSettings({
                        ...settings,
                        min_chunk_size: parseInt(e.target.value, 10),
                      })
                    }
                    className="w-full bg-slate-900 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 focus:outline-none focus:border-indigo-500"
                  >
                    <option value={262144}>256 KB</option>
                    <option value={524288}>512 KB</option>
                    <option value={1048576}>1 MB (Recommended)</option>
                    <option value={2097152}>2 MB</option>
                    <option value={5242880}>5 MB</option>
                    <option value={10485760}>10 MB</option>
                  </select>
                </div>
              </div>

              <div>
                <label className="flex items-center gap-2 text-[11px] text-slate-400 mb-1">
                  <Gauge size={13} className="text-indigo-400" />
                  <span>Global Speed Limit (KB/s, 0 = Unlimited)</span>
                </label>
                <input
                  type="number"
                  min={0}
                  value={settings.global_speed_limit ? settings.global_speed_limit / 1024 : 0}
                  onChange={(e) =>
                    setSettings({
                      ...settings,
                      global_speed_limit: (parseInt(e.target.value, 10) || 0) * 1024,
                    })
                  }
                  className="w-full bg-slate-900 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 focus:outline-none focus:border-indigo-500"
                />
              </div>
            </div>

            {/* Proxy & VPN Configuration */}
            <div className="p-4 rounded-2xl bg-slate-950/50 border border-white/5 space-y-3">
              <div className="flex items-center gap-2 text-xs font-semibold text-slate-200">
                <Shield size={14} className="text-indigo-400" />
                <span>VPN & Proxy Routing</span>
              </div>

              <div className="space-y-2 text-xs">
                <label className="flex items-center gap-2.5 text-slate-300 cursor-pointer">
                  <input
                    type="radio"
                    name="proxy_mode"
                    value="system"
                    checked={settings.proxy_mode === 'system'}
                    onChange={() => setSettings({ ...settings, proxy_mode: 'system' })}
                    className="accent-indigo-500"
                  />
                  <span>Auto-detect Windows System / VPN Proxy (Recommended)</span>
                </label>

                <label className="flex items-center gap-2.5 text-slate-300 cursor-pointer">
                  <input
                    type="radio"
                    name="proxy_mode"
                    value="direct"
                    checked={settings.proxy_mode === 'direct'}
                    onChange={() => setSettings({ ...settings, proxy_mode: 'direct' })}
                    className="accent-indigo-500"
                  />
                  <span>Direct Connection (Bypass all proxies)</span>
                </label>

                <label className="flex items-center gap-2.5 text-slate-300 cursor-pointer">
                  <input
                    type="radio"
                    name="proxy_mode"
                    value="custom"
                    checked={settings.proxy_mode === 'custom'}
                    onChange={() => setSettings({ ...settings, proxy_mode: 'custom' })}
                    className="accent-indigo-500"
                  />
                  <span>Manual Custom Proxy</span>
                </label>
              </div>

              {settings.proxy_mode === 'custom' && (
                <input
                  type="text"
                  value={settings.custom_proxy_url || ''}
                  onChange={(e) => setSettings({ ...settings, custom_proxy_url: e.target.value })}
                  placeholder="http://127.0.0.1:10808 or socks5://127.0.0.1:1080"
                  className="w-full bg-slate-900 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 font-mono focus:outline-none focus:border-indigo-500"
                />
              )}
            </div>

            {/* Antivirus & Threat Scanning */}
            <div className="p-4 rounded-2xl bg-slate-950/50 border border-white/5 space-y-3">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2 text-xs font-semibold text-slate-200">
                  <ShieldCheck size={14} className="text-emerald-400" />
                  <span>Antivirus & Threat Scanning</span>
                </div>
                <label className="relative inline-flex items-center cursor-pointer">
                  <input
                    type="checkbox"
                    checked={settings.scan_with_antivirus || false}
                    onChange={(e) =>
                      setSettings({ ...settings, scan_with_antivirus: e.target.checked })
                    }
                    className="sr-only peer"
                  />
                  <div className="w-8 h-4 bg-slate-800 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-slate-300 after:border after:rounded-full after:h-3 after:w-3 after:transition-all peer-checked:bg-emerald-600"></div>
                </label>
              </div>

              <p className="text-[11px] text-slate-400">
                Automatically verify completed downloads using Windows Defender or a custom antivirus scanner.
              </p>

              {settings.scan_with_antivirus && (
                <div className="pt-2 border-t border-white/5 space-y-2">
                  <label className="block text-[11px] text-slate-400">
                    Custom Antivirus Executable Path (Leave blank for default Windows Defender)
                  </label>
                  <input
                    type="text"
                    value={settings.antivirus_path || ''}
                    onChange={(e) =>
                      setSettings({ ...settings, antivirus_path: e.target.value })
                    }
                    placeholder="C:\Program Files\Windows Defender\MpCmdRun.exe"
                    className="w-full bg-slate-900 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 font-mono placeholder:text-slate-600 focus:outline-none focus:border-indigo-500"
                  />
                </div>
              )}
            </div>

            {/* IDM-Grade Scheduler */}
            <div className="p-4 rounded-2xl bg-slate-950/50 border border-white/5 space-y-4">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2 text-xs font-semibold text-slate-200">
                  <Clock size={14} className="text-indigo-400" />
                  <span>Automated Download Scheduler</span>
                </div>
                <label className="relative inline-flex items-center cursor-pointer">
                  <input
                    type="checkbox"
                    checked={settings.scheduler_enabled || false}
                    onChange={(e) =>
                      setSettings({ ...settings, scheduler_enabled: e.target.checked })
                    }
                    className="sr-only peer"
                  />
                  <div className="w-8 h-4 bg-slate-800 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-slate-300 after:border after:rounded-full after:h-3 after:w-3 after:transition-all peer-checked:bg-indigo-600"></div>
                </label>
              </div>

              {settings.scheduler_enabled && (
                <div className="space-y-3 pt-1 border-t border-white/5">
                  {/* Active Days */}
                  <div>
                    <label className="block text-[11px] text-slate-400 mb-1.5">
                      Scheduled Active Days
                    </label>
                    <div className="flex items-center gap-1.5">
                      {[
                        { id: 'mon', label: 'M' },
                        { id: 'tue', label: 'T' },
                        { id: 'wed', label: 'W' },
                        { id: 'thu', label: 'T' },
                        { id: 'fri', label: 'F' },
                        { id: 'sat', label: 'S' },
                        { id: 'sun', label: 'S' },
                      ].map((day) => {
                        const isSelected = (settings.scheduler_days || []).includes(day.id);
                        return (
                          <button
                            key={day.id}
                            type="button"
                            onClick={() => {
                              const current = settings.scheduler_days || [];
                              const updated = isSelected
                                ? current.filter((d) => d !== day.id)
                                : [...current, day.id];
                              setSettings({ ...settings, scheduler_days: updated });
                            }}
                            className={`flex-1 h-7 rounded-lg text-xs font-medium transition-all ${
                              isSelected
                                ? 'bg-indigo-600 text-white shadow-sm shadow-indigo-600/30'
                                : 'bg-slate-900 text-slate-400 hover:text-slate-200 hover:bg-slate-800'
                            }`}
                          >
                            {day.label}
                          </button>
                        );
                      })}
                    </div>
                  </div>

                  {/* Start & Stop Times */}
                  <div className="grid grid-cols-2 gap-3">
                    <div>
                      <label className="block text-[11px] text-slate-400 mb-1">
                        Start Queue At
                      </label>
                      <input
                        type="time"
                        value={settings.scheduler_start_time || '02:00'}
                        onChange={(e) =>
                          setSettings({ ...settings, scheduler_start_time: e.target.value })
                        }
                        className="w-full bg-slate-900 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 focus:outline-none focus:border-indigo-500"
                      />
                    </div>
                    <div>
                      <label className="block text-[11px] text-slate-400 mb-1">
                        Stop Queue At
                      </label>
                      <input
                        type="time"
                        value={settings.scheduler_stop_time || '07:00'}
                        onChange={(e) =>
                          setSettings({ ...settings, scheduler_stop_time: e.target.value })
                        }
                        className="w-full bg-slate-900 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 focus:outline-none focus:border-indigo-500"
                      />
                    </div>
                  </div>

                  {/* Scheduler Action & Speed Cap */}
                  <div className="grid grid-cols-2 gap-3">
                    <div>
                      <label className="block text-[11px] text-slate-400 mb-1">
                        Scheduled Speed Cap (KB/s)
                      </label>
                      <input
                        type="number"
                        min={0}
                        value={settings.scheduler_speed_limit ? settings.scheduler_speed_limit / 1024 : 0}
                        onChange={(e) =>
                          setSettings({
                            ...settings,
                            scheduler_speed_limit: (parseInt(e.target.value, 10) || 0) * 1024,
                          })
                        }
                        placeholder="0 = Unlimited"
                        className="w-full bg-slate-900 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 focus:outline-none focus:border-indigo-500"
                      />
                    </div>

                    <div>
                      <label className="block text-[11px] text-slate-400 mb-1">
                        When Schedule Closes:
                      </label>
                      <select
                        value={settings.scheduler_action || 'none'}
                        onChange={(e) =>
                          setSettings({
                            ...settings,
                            scheduler_action: e.target.value as any,
                          })
                        }
                        className="w-full bg-slate-900 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 focus:outline-none focus:border-indigo-500"
                      >
                        <option value="none">Pause / Hold Queue Only</option>
                        <option value="sleep">Put Computer to Sleep</option>
                        <option value="shutdown">Shut Down Computer</option>
                        <option value="exit">Exit Nexdrop</option>
                      </select>
                    </div>
                  </div>
                </div>
              )}
            </div>

            {/* Automation & Completion Actions */}
            <div className="p-4 rounded-2xl bg-slate-950/50 border border-white/5 space-y-3">
              <div className="flex items-center gap-2 text-xs font-semibold text-slate-200">
                <Power size={14} className="text-indigo-400" />
                <span>Completion Actions</span>
              </div>

              <div>
                <label className="block text-[11px] text-slate-400 mb-1">
                  When All Downloads Complete:
                </label>
                <select
                  value={settings.on_complete_action}
                  onChange={(e) =>
                    setSettings({
                      ...settings,
                      on_complete_action: e.target.value as any,
                    })
                  }
                  className="w-full bg-slate-900 border border-white/10 rounded-xl px-3 py-1.5 text-xs text-slate-200 focus:outline-none"
                >
                  <option value="notify">Show Desktop Notification & Sound</option>
                  <option value="sleep">Put Computer to Sleep</option>
                  <option value="shutdown">Shut Down Computer</option>
                  <option value="none">Do Nothing</option>
                </select>
              </div>

              <div className="space-y-2 pt-1 text-xs">
                <label className="flex items-center gap-3 cursor-pointer text-slate-300">
                  <input
                    type="checkbox"
                    checked={settings.sound_enabled}
                    onChange={(e) =>
                      setSettings({ ...settings, sound_enabled: e.target.checked })
                    }
                    className="rounded accent-indigo-500"
                  />
                  <div className="flex items-center gap-1.5">
                    <Volume2 size={13} className="text-slate-400" />
                    <span>Play chime on download complete</span>
                  </div>
                </label>

                <label className="flex items-center gap-3 cursor-pointer text-slate-300">
                  <input
                    type="checkbox"
                    checked={settings.auto_resume_on_launch}
                    onChange={(e) =>
                      setSettings({ ...settings, auto_resume_on_launch: e.target.checked })
                    }
                    className="rounded accent-indigo-500"
                  />
                  <span>Automatically resume unfinished downloads on startup</span>
                </label>

                <label className="flex items-center gap-3 cursor-pointer text-slate-300">
                  <input
                    type="checkbox"
                    checked={settings.clipboard_watch}
                    onChange={(e) =>
                      setSettings({ ...settings, clipboard_watch: e.target.checked })
                    }
                    className="rounded accent-indigo-500"
                  />
                  <span>Monitor clipboard and notify when download links are copied</span>
                </label>
              </div>
            </div>

            {/* Site Logins & Credentials Vault */}
            <div className="p-4 rounded-2xl bg-slate-950/50 border border-white/5 space-y-3">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2 text-xs font-semibold text-slate-200">
                  <Key size={14} className="text-amber-400" />
                  <span>Site Logins &amp; Credentials Vault</span>
                </div>
                <button
                  type="button"
                  onClick={() => setIsAddingCred(!isAddingCred)}
                  className="flex items-center gap-1 px-2.5 py-1 text-[11px] font-medium text-indigo-400 hover:text-white bg-indigo-500/10 hover:bg-indigo-600/30 rounded-lg transition-colors"
                >
                  <Plus size={12} />
                  <span>{isAddingCred ? 'Cancel' : 'Add Login'}</span>
                </button>
              </div>

              <p className="text-[11px] text-slate-400">
                Store domain credentials (username/password or bearer cookies) to automatically authenticate downloads from restricted hosts.
              </p>

              {/* Add Credential Form */}
              {isAddingCred && (
                <div className="p-3 rounded-xl bg-slate-900 border border-indigo-500/30 space-y-2.5">
                  <div className="text-xs font-medium text-indigo-300">Register New Site Credential</div>
                  <div className="grid grid-cols-2 gap-2">
                    <div>
                      <label className="block text-[10px] text-slate-400 mb-0.5">Domain / Host</label>
                      <input
                        type="text"
                        value={newDomain}
                        onChange={(e) => setNewDomain(e.target.value)}
                        placeholder="e.g. rapidgator.net"
                        className="w-full bg-slate-950 border border-white/10 rounded-lg px-2.5 py-1 text-xs text-slate-200 font-mono focus:outline-none focus:border-indigo-500"
                      />
                    </div>
                    <div>
                      <label className="block text-[10px] text-slate-400 mb-0.5">Username / Login</label>
                      <input
                        type="text"
                        value={newUsername}
                        onChange={(e) => setNewUsername(e.target.value)}
                        placeholder="Optional username"
                        className="w-full bg-slate-950 border border-white/10 rounded-lg px-2.5 py-1 text-xs text-slate-200 focus:outline-none focus:border-indigo-500"
                      />
                    </div>
                  </div>

                  <div className="grid grid-cols-2 gap-2">
                    <div>
                      <label className="block text-[10px] text-slate-400 mb-0.5">Password</label>
                      <input
                        type="password"
                        value={newPassword}
                        onChange={(e) => setNewPassword(e.target.value)}
                        placeholder="Optional password"
                        className="w-full bg-slate-950 border border-white/10 rounded-lg px-2.5 py-1 text-xs text-slate-200 focus:outline-none focus:border-indigo-500"
                      />
                    </div>
                    <div>
                      <label className="block text-[10px] text-slate-400 mb-0.5">Bearer Token / Cookie</label>
                      <input
                        type="text"
                        value={newToken}
                        onChange={(e) => setNewToken(e.target.value)}
                        placeholder="sessionid=... or token"
                        className="w-full bg-slate-950 border border-white/10 rounded-lg px-2.5 py-1 text-xs text-slate-200 font-mono focus:outline-none focus:border-indigo-500"
                      />
                    </div>
                  </div>

                  <div>
                    <label className="block text-[10px] text-slate-400 mb-0.5">Notes / Description</label>
                    <input
                      type="text"
                      value={newNotes}
                      onChange={(e) => setNewNotes(e.target.value)}
                      placeholder="e.g. Premium account"
                      className="w-full bg-slate-950 border border-white/10 rounded-lg px-2.5 py-1 text-xs text-slate-200 focus:outline-none focus:border-indigo-500"
                    />
                  </div>

                  <div className="flex justify-end pt-1">
                    <button
                      type="button"
                      onClick={handleAddCredential}
                      disabled={!newDomain.trim()}
                      className="px-3 py-1 bg-indigo-600 hover:bg-indigo-500 disabled:opacity-40 text-white rounded-lg text-xs font-medium transition-colors"
                    >
                      Save to Vault
                    </button>
                  </div>
                </div>
              )}

              {/* Credentials List */}
              {credentials.length === 0 ? (
                <div className="py-2 text-center text-xs text-slate-500">
                  No saved site credentials yet.
                </div>
              ) : (
                <div className="space-y-1.5 max-h-40 overflow-y-auto pr-1">
                  {credentials.map((cred) => (
                    <div
                      key={cred.id}
                      className="flex items-center justify-between p-2 rounded-xl bg-slate-900 border border-white/5 text-xs"
                    >
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2">
                          <Lock size={12} className="text-amber-400 shrink-0" />
                          <span className="font-mono font-medium text-slate-200 truncate">
                            {cred.domain}
                          </span>
                          {cred.notes && (
                            <span className="text-[10px] px-1.5 py-0.5 rounded bg-slate-800 text-slate-400">
                              {cred.notes}
                            </span>
                          )}
                        </div>
                        <div className="text-[11px] text-slate-400 mt-0.5 pl-5">
                          {cred.username ? `User: ${cred.username}` : 'Token-based auth'}
                        </div>
                      </div>

                      <button
                        type="button"
                        onClick={() => handleDeleteCredential(cred.id)}
                        className="p-1.5 text-slate-400 hover:text-rose-400 hover:bg-rose-500/10 rounded-lg transition-colors ml-2"
                        title="Delete credential"
                      >
                        <Trash2 size={13} />
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </div>

            {/* Browser Extension & Link Grabber Server */}
            <div className="p-4 rounded-2xl bg-slate-950/50 border border-white/5 space-y-3">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2 text-xs font-semibold text-slate-200">
                  <Compass size={14} className="text-indigo-400" />
                  <span>Browser Extension &amp; Native Link Grabber</span>
                </div>
                <span className="flex items-center gap-1 text-[10px] font-medium text-emerald-400 bg-emerald-500/10 px-2 py-0.5 rounded-full border border-emerald-500/20">
                  <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse" />
                  Active on 127.0.0.1:18888
                </span>
              </div>

              <p className="text-[11px] text-slate-400 leading-relaxed">
                Connect Chrome, Edge, Brave, Opera, or Firefox to capture downloads and grab HTML5 video streams directly into Nexdrop.
              </p>

              <div className="p-2.5 rounded-xl bg-slate-900 border border-white/5 space-y-1.5 text-[11px]">
                <div className="font-medium text-slate-300">Extension Directory (Manifest V3):</div>
                <div className="font-mono text-[10px] text-indigo-300 bg-slate-950 p-2 rounded-lg border border-white/5 select-all">
                  d:\Projects\Nexdrop\extensions\nexdrop-browser-extension
                </div>
                <p className="text-slate-500 text-[10px]">
                  Go to <code className="text-slate-400">chrome://extensions</code> &rarr; Enable Developer mode &rarr; Click "Load unpacked" &rarr; Select folder above.
                </p>
              </div>
            </div>

            {/* Footer Buttons */}
            <div className="flex items-center justify-end gap-3 pt-4 border-t border-white/5">
              <button
                type="button"
                onClick={onClose}
                className="px-4 py-2 rounded-xl text-xs font-medium text-slate-400 hover:text-slate-200 hover:bg-slate-800 transition-colors"
              >
                Close
              </button>
              <button
                type="submit"
                className="flex items-center gap-2 px-5 py-2 rounded-xl text-xs font-semibold bg-indigo-600 hover:bg-indigo-500 text-white shadow-md shadow-indigo-600/30 transition-all active:scale-95"
              >
                {saved ? <Check size={14} /> : <Save size={14} />}
                <span>{saved ? 'Saved!' : 'Save Preferences'}</span>
              </button>
            </div>
          </form>
        </motion.div>
      </div>
    </AnimatePresence>
  );
};
