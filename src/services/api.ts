import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  BrowserIntegration,
  BrowserSetupOutcome,
  BrowserRequest,
  StreamProbe,
  DownloadQueue,
  QueueInfo,
  AddDownloadRequest,
  AddTorrentRequest,
  AppSettings,
  DownloadProgressEvent,
  DownloadTask,
  SiteCredential,
  UrlProbeResult,
  GrabbedLink,
  TorrentLive,
  TorrentProbeInfo,
} from '../types/download';

export const isTauri = (): boolean => {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
};

// Mock in-memory state for development / browser mode
let mockSiteCredentials: SiteCredential[] = [
  {
    id: 'mock-1',
    domain: 'github.com',
    username: 'octocat',
    token_or_cookie: 'ghp_exampleToken123',
    notes: 'GitHub personal access token',
  },
];

let mockQueues: QueueInfo[] = [
  {
    id: 'main',
    name: 'Main queue',
    schedule_enabled: false,
    days: ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'],
    start_time: '02:00',
    stop_time: null,
    max_active: 0,
    speed_limit: 0,
    after_done: 'none',
    retry_count: 0,
    retry_delay_secs: 60,
    position: 0,
    open: true,
    active: 0,
    queued: 0,
  },
];

let mockDownloads: DownloadTask[] = [
  {
    id: 'mock-t1',
    url: 'magnet:?xt=urn:btih:6f1c2a9e7d3b4a58c0e91f2b7a6d4c3e8f905a12&dn=Open+Source+Wallpapers',
    filename: 'Open Source Wallpapers',
    destination_path: 'C:/Downloads',
    temp_path: '',
    total_bytes: 1288490188,
    downloaded_bytes: 515396075,
    status: 'downloading',
    resumable: true,
    category: 'other',
    connections: 1,
    speed_limit: 0,
    priority: 0,
    speed_bps: 9400000,
    eta_seconds: 82,
    created_at: Date.now() - 120000,
    updated_at: Date.now(),
    segments: [],
    kind: 'torrent',
    info_hash: '6f1c2a9e7d3b4a58c0e91f2b7a6d4c3e8f905a12',
    uploaded_bytes: 41943040,
    peers: 14,
    upload_bps: 620000,
    seeding: false,
    torrent_files: [
      { index: 0, path: 'wallpapers/mountains.png', length: 524288000, selected: true },
      { index: 1, path: 'wallpapers/forest.png', length: 482344960, selected: true },
      { index: 2, path: 'wallpapers/ocean.png', length: 281857228, selected: true },
    ],
  },
  {
    id: 'mock-t2',
    url: 'magnet:?xt=urn:btih:a41b9c0d5e7f2318c6d4e0b9a8f7c6d5e4b3a291&dn=ubuntu-24.04-desktop-amd64.iso',
    filename: 'ubuntu-24.04-desktop-amd64.iso',
    destination_path: 'C:/Downloads',
    temp_path: '',
    total_bytes: 6144000000,
    downloaded_bytes: 6144000000,
    status: 'completed',
    resumable: true,
    category: 'compressed',
    connections: 1,
    speed_limit: 0,
    priority: 0,
    speed_bps: 0,
    created_at: Date.now() - 86400000,
    updated_at: Date.now() - 80000000,
    completed_at: Date.now() - 80000000,
    segments: [],
    kind: 'torrent',
    info_hash: 'a41b9c0d5e7f2318c6d4e0b9a8f7c6d5e4b3a291',
    uploaded_bytes: 3500000000,
    peers: 6,
    upload_bps: 1250000,
    seeding: true,
    torrent_files: [{ index: 0, path: 'ubuntu-24.04-desktop-amd64.iso', length: 6144000000, selected: true }],
  },
  {
    id: 'mock-1',
    url: 'https://releases.ubuntu.com/24.04/ubuntu-24.04-desktop-amd64.iso',
    filename: 'ubuntu-24.04-desktop-amd64.iso',
    destination_path: 'C:/Downloads',
    temp_path: 'C:/Downloads/ubuntu-24.04-desktop-amd64.iso.nexpart',
    total_bytes: 5865627648,
    downloaded_bytes: 3812657971,
    status: 'downloading',
    resumable: true,
    category: 'compressed',
    connections: 8,
    speed_limit: 0,
    priority: 2,
    speed_bps: 14820000,
    eta_seconds: 138,
    created_at: Date.now() - 360000,
    updated_at: Date.now(),
    segments: [
      { segment_index: 0, start_byte: 0, end_byte: 733203456, downloaded_bytes: 733203456, total_bytes: 733203456, status: 'completed' },
      { segment_index: 1, start_byte: 733203456, end_byte: 1466406912, downloaded_bytes: 733203456, total_bytes: 733203456, status: 'completed' },
      { segment_index: 2, start_byte: 1466406912, end_byte: 2199610368, downloaded_bytes: 733203456, total_bytes: 733203456, status: 'completed' },
      { segment_index: 3, start_byte: 2199610368, end_byte: 2932813824, downloaded_bytes: 733203456, total_bytes: 733203456, status: 'completed' },
      { segment_index: 4, start_byte: 2932813824, end_byte: 3666017280, downloaded_bytes: 620000000, total_bytes: 733203456, status: 'downloading' },
      { segment_index: 5, start_byte: 3666017280, end_byte: 4399220736, downloaded_bytes: 180000000, total_bytes: 733203456, status: 'downloading' },
      { segment_index: 6, start_byte: 4399220736, end_byte: 5132424192, downloaded_bytes: 80000000, total_bytes: 733203456, status: 'downloading' },
      { segment_index: 7, start_byte: 5132424192, end_byte: 5865627647, downloaded_bytes: 0, total_bytes: 733203456, status: 'pending' },
    ],
  },
  {
    id: 'mock-2',
    url: 'https://download.blender.org/release/Blender4.2/blender-4.2.0-windows-x64.msi',
    filename: 'blender-4.2.0-windows-x64.msi',
    destination_path: 'C:/Downloads',
    temp_path: 'C:/Downloads/blender-4.2.0-windows-x64.msi.nexpart',
    total_bytes: 341835776,
    downloaded_bytes: 341835776,
    status: 'completed',
    resumable: true,
    category: 'programs',
    connections: 4,
    speed_limit: 0,
    priority: 1,
    speed_bps: 0,
    created_at: Date.now() - 7200000,
    updated_at: Date.now() - 7000000,
    completed_at: Date.now() - 7000000,
    segments: [],
  },
  {
    id: 'mock-3',
    url: 'https://sample-videos.com/video321/mp4/720/big_buck_bunny_720p_30mb.mp4',
    filename: 'big_buck_bunny_720p_30mb.mp4',
    destination_path: 'C:/Downloads',
    temp_path: 'C:/Downloads/big_buck_bunny_720p_30mb.mp4.nexpart',
    total_bytes: 31457280,
    downloaded_bytes: 12582912,
    status: 'paused',
    resumable: true,
    category: 'videos',
    connections: 4,
    speed_limit: 0,
    priority: 0,
    speed_bps: 0,
    created_at: Date.now() - 14400000,
    updated_at: Date.now() - 14000000,
    segments: [],
  },
  {
    id: 'mock-4',
    url: 'https://dl.google.com/android/studio/install/2024.2/android-studio-2024.2-windows.exe',
    filename: 'android-studio-2024.2-windows.exe',
    destination_path: 'C:/Downloads',
    temp_path: 'C:/Downloads/android-studio-2024.2-windows.exe.nexpart',
    total_bytes: 1288490188,
    downloaded_bytes: 0,
    status: 'queued',
    resumable: true,
    category: 'programs',
    connections: 8,
    speed_limit: 0,
    priority: 0,
    speed_bps: 0,
    created_at: Date.now() - 60000,
    updated_at: Date.now(),
    segments: [],
  },
  {
    id: 'mock-5',
    url: 'https://cdn.example.org/podcasts/episode-142-final-mix.mp3',
    filename: 'episode-142-final-mix.mp3',
    destination_path: 'C:/Downloads',
    temp_path: 'C:/Downloads/episode-142-final-mix.mp3.nexpart',
    total_bytes: 94371840,
    downloaded_bytes: 41943040,
    status: 'failed',
    resumable: true,
    category: 'music',
    connections: 4,
    speed_limit: 0,
    priority: 0,
    speed_bps: 0,
    created_at: Date.now() - 86400000,
    updated_at: Date.now() - 86000000,
    error_message: 'Connection reset by peer (os error 10054)',
    segments: [],
  },
  {
    id: 'mock-6',
    url: 'https://arxiv.org/pdf/2401.00001v2.pdf',
    filename: 'attention-survey-2024.pdf',
    destination_path: 'C:/Downloads',
    temp_path: 'C:/Downloads/attention-survey-2024.pdf.nexpart',
    total_bytes: 4718592,
    downloaded_bytes: 0,
    status: 'scheduled',
    resumable: true,
    category: 'documents',
    connections: 2,
    speed_limit: 0,
    priority: 0,
    speed_bps: 0,
    created_at: Date.now() - 1800000,
    updated_at: Date.now(),
    scheduled_time: Math.floor((Date.now() + 9 * 3600000) / 1000),
    segments: [],
  },
];

let mockSettings: AppSettings = {
  default_destination: 'C:/Users/Downloads',
  max_active_downloads: 3,
  default_connections: 8,
  max_connections_per_server: 8,
  min_chunk_size: 1048576,
  global_speed_limit: 0,
  auto_resume_on_launch: true,
  notify_on_complete: true,
  theme: 'dark',
  clipboard_watch: false,
  auto_categorize: false,
  proxy_mode: 'system',
  custom_proxy_url: undefined,
  on_complete_action: 'notify',
  sound_enabled: true,
  scheduler_enabled: false,
  scheduler_start_time: '02:00',
  scheduler_stop_time: '07:00',
  scheduler_days: ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'],
  scheduler_speed_limit: 0,
  scheduler_action: 'none',
  scan_with_antivirus: true,
  antivirus_path: undefined,
  torrent_seed_after_download: true,
  torrent_ratio_limit: 1,
  torrent_upload_limit: 0,
  torrent_dht: true,
  torrent_upnp: false,
  browser_prompt: true,
  browser_setup_done: false,
};

export const api = {
  async probeUrl(url: string, from?: BrowserRequest): Promise<UrlProbeResult> {
    if (isTauri()) {
      return await invoke<UrlProbeResult>('probe_url_cmd', {
        url,
        referer: from?.referer || null,
        userAgent: from?.userAgent || null,
        headers: from?.headers && Object.keys(from.headers).length ? from.headers : null,
      });
    }
    // Browser mock probe
    const filename = url.split('/').filter(Boolean).pop()?.split('?')[0] || 'file.bin';
    return {
      url,
      filename,
      total_bytes: 104857600,
      resumable: true,
      category: 'compressed',
      default_destination: mockSettings.default_destination,
    };
  },

  async addDownload(req: AddDownloadRequest): Promise<DownloadTask> {
    if (isTauri()) {
      return await invoke<DownloadTask>('add_download_cmd', { req });
    }
    const task: DownloadTask = {
      id: `task-${Date.now()}`,
      url: req.url,
      filename: req.filename || req.url.split('/').filter(Boolean).pop() || 'download.bin',
      destination_path: req.destination_path || mockSettings.default_destination,
      temp_path: 'temp.part',
      total_bytes: 52428800,
      downloaded_bytes: 0,
      status: req.scheduled_time ? 'scheduled' : 'downloading',
      resumable: true,
      category: 'other',
      connections: req.connections || 8,
      speed_limit: req.speed_limit || 0,
      priority: req.priority || 0,
      speed_bps: 8500000,
      eta_seconds: 6,
      segments: [],
      created_at: Date.now(),
      updated_at: Date.now(),
      scheduled_time: req.scheduled_time,
    };
    mockDownloads.unshift(task);
    return task;
  },

  async batchAddDownloads(reqs: AddDownloadRequest[]): Promise<DownloadTask[]> {
    if (isTauri()) {
      return await invoke<DownloadTask[]>('batch_add_downloads_cmd', { reqs });
    }
    const tasks: DownloadTask[] = [];
    for (const r of reqs) {
      tasks.push(await this.addDownload(r));
    }
    return tasks;
  },

  async batchAdd(reqs: AddDownloadRequest[]): Promise<DownloadTask[]> {
    return this.batchAddDownloads(reqs);
  },

  async getAllDownloads(): Promise<DownloadTask[]> {
    if (isTauri()) {
      return await invoke<DownloadTask[]>('get_all_downloads_cmd');
    }
    return [...mockDownloads];
  },

  async pauseDownload(id: string): Promise<void> {
    if (isTauri()) {
      return await invoke('pause_download_cmd', { id });
    }
    mockDownloads = mockDownloads.map((t) => (t.id === id ? { ...t, status: 'paused', speed_bps: 0 } : t));
  },

  async resumeDownload(id: string): Promise<void> {
    if (isTauri()) {
      return await invoke('resume_download_cmd', { id });
    }
    mockDownloads = mockDownloads.map((t) => (t.id === id ? { ...t, status: 'downloading' } : t));
  },

  async retryDownload(id: string): Promise<void> {
    if (isTauri()) {
      return await invoke('retry_download_cmd', { id });
    }
    mockDownloads = mockDownloads.map((t) => (t.id === id ? { ...t, status: 'downloading', error_message: undefined } : t));
  },

  async cancelDownload(id: string, deleteFile = false): Promise<void> {
    if (isTauri()) {
      return await invoke('cancel_download_cmd', { id, deleteFile });
    }
    mockDownloads = mockDownloads.map((t) => (t.id === id ? { ...t, status: 'canceled' } : t));
  },

  async deleteDownload(id: string, deleteFile = false): Promise<void> {
    if (isTauri()) {
      return await invoke('delete_download_cmd', { id, deleteFile });
    }
    mockDownloads = mockDownloads.filter((t) => t.id !== id);
  },

  async openFile(id: string): Promise<void> {
    if (isTauri()) {
      return await invoke('open_file_cmd', { id });
    }
    console.info(`[mock] open file ${id}`);
  },

  async revealFile(id: string): Promise<void> {
    if (isTauri()) {
      return await invoke('reveal_file_cmd', { id });
    }
    console.info(`[mock] reveal file ${id}`);
  },

  async getSettings(): Promise<AppSettings> {
    if (isTauri()) {
      return await invoke<AppSettings>('get_settings_cmd');
    }
    return { ...mockSettings };
  },

  async updateSettings(settings: AppSettings): Promise<void> {
    if (isTauri()) {
      return await invoke('update_settings_cmd', { settings });
    }
    mockSettings = { ...settings };
  },

  async getDefaultDownloadDir(): Promise<string> {
    if (isTauri()) {
      return await invoke<string>('get_default_download_dir_cmd');
    }
    return 'C:/Users/Downloads';
  },

  async verifyChecksum(id: string, algorithm: string, expected?: string) {
    if (isTauri()) {
      return await invoke<{ algorithm: string; calculated_hash: string; expected_hash?: string; matches: boolean }>(
        'verify_checksum_cmd',
        { id, algorithm, expected: expected || null }
      );
    }
    return {
      algorithm,
      calculated_hash: algorithm === 'md5' ? 'd41d8cd98f00b204e9800998ecf8427e' : 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855',
      expected_hash: expected,
      matches: true,
    };
  },

  async calculateFileHash(filePath: string, algorithm: string) {
    if (isTauri()) {
      return await invoke<{ algorithm: string; calculated_hash: string; expected_hash?: string; matches: boolean }>(
        'calculate_file_hash_cmd',
        { filePath, algorithm }
      );
    }
    return {
      algorithm,
      calculated_hash: 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855',
      matches: true,
    };
  },

  async getCurlCommand(id: string): Promise<string> {
    if (isTauri()) {
      return await invoke<string>('get_curl_command_cmd', { id });
    }
    const task = mockDownloads.find((t) => t.id === id);
    return `curl -C - -L -o "${task?.filename || 'file.dat'}" "${task?.url || 'https://example.com'}"`;
  },

  async exportDownloads(): Promise<string> {
    if (isTauri()) {
      return await invoke<string>('export_downloads_cmd');
    }
    return JSON.stringify(mockDownloads, null, 2);
  },

  async getSiteCredentials(): Promise<SiteCredential[]> {
    if (isTauri()) {
      return await invoke<SiteCredential[]>('get_site_credentials_cmd');
    }
    return [...mockSiteCredentials];
  },

  async saveSiteCredential(cred: SiteCredential): Promise<void> {
    if (isTauri()) {
      await invoke('save_site_credential_cmd', { cred });
      return;
    }
    const idx = mockSiteCredentials.findIndex((c) => c.domain === cred.domain);
    if (idx >= 0) {
      mockSiteCredentials[idx] = cred;
    } else {
      mockSiteCredentials.push(cred);
    }
  },

  async deleteSiteCredential(id: string): Promise<void> {
    if (isTauri()) {
      await invoke('delete_site_credential_cmd', { id });
      return;
    }
    mockSiteCredentials = mockSiteCredentials.filter((c) => c.id !== id);
  },

  async exportChecksumFile(id: string, algorithm: string): Promise<string> {
    if (isTauri()) {
      return await invoke<string>('export_checksum_file_cmd', { id, algorithm });
    }
    return `C:/Users/Downloads/file.zip.${algorithm}`;
  },

  async parseCurlCommand(curl: string): Promise<AddDownloadRequest> {
    if (isTauri()) {
      return await invoke<AddDownloadRequest>('parse_curl_command_cmd', { curl });
    }
    // Fallback simple browser parser
    const match = curl.match(/https?:\/\/[^\s"']+/);
    return {
      url: match ? match[0] : 'https://example.com/download.zip',
    };
  },

  async grabPageLinks(url: string): Promise<GrabbedLink[]> {
    if (isTauri()) {
      return await invoke<GrabbedLink[]>('grab_page_links_cmd', { url });
    }
    return [
      {
        url: 'https://releases.ubuntu.com/24.04/ubuntu-24.04-desktop-amd64.iso',
        filename: 'ubuntu-24.04-desktop-amd64.iso',
        category: 'compressed',
        extension: 'iso',
      },
      {
        url: 'https://releases.ubuntu.com/24.04/ubuntu-24.04-live-server-amd64.iso',
        filename: 'ubuntu-24.04-live-server-amd64.iso',
        category: 'compressed',
        extension: 'iso',
      },
    ];
  },

  /** Looks inside an HLS / DASH link: qualities, length, and whether it can be downloaded. */
  async probeStream(url: string, referer?: string): Promise<StreamProbe> {
    if (isTauri()) {
      return await invoke<StreamProbe>('probe_stream_cmd', { url, referer: referer || null, userAgent: null });
    }
    await new Promise((r) => setTimeout(r, 400));
    return {
      url,
      kind: url.includes('.mpd') ? 'dash' : 'hls',
      title: 'Sample stream',
      filename: 'Sample stream.mp4',
      duration_secs: 634,
      variants: [
        { id: '8000000:1920x1080', label: '1080p · 8.0 Mbit/s', bandwidth: 8000000, width: 1920, height: 1080, separate_audio: true },
        { id: '4000000:1280x720', label: '720p · 4.0 Mbit/s', bandwidth: 4000000, width: 1280, height: 720, separate_audio: true },
        { id: '1000000:640x360', label: '360p · 1.0 Mbit/s', bandwidth: 1000000, width: 640, height: 360, separate_audio: true },
      ],
      unsupported: null,
    };
  },

  /** Browsers on this computer and whether Nexdrop's extension is working in each. */
  async getBrowsers(): Promise<BrowserIntegration> {
    if (isTauri()) {
      return await invoke<BrowserIntegration>('get_browsers_cmd');
    }
    return {
      browsers: [
        { id: 'edge', name: 'Microsoft Edge', family: 'chromium', installed: true, connected: true, ever_connected: true, last_seen_secs_ago: 40, version: '1.3.0' },
        { id: 'firefox', name: 'Mozilla Firefox', family: 'firefox', installed: true, connected: false, ever_connected: false },
      ],
      signed_firefox: false,
      extension_dir: 'C:\\Program Files\\Nexdrop\\extension',
    };
  },

  /** Opens what's needed in a browser to finish installing the extension. */
  async setupBrowser(id: string): Promise<BrowserSetupOutcome> {
    if (isTauri()) {
      return await invoke<BrowserSetupOutcome>('setup_browser_cmd', { id });
    }
    return id === 'firefox'
      ? { mode: 'temporary', folder: 'C:\\Program Files\\Nexdrop\\extension' }
      : { mode: 'unpacked', folder: 'C:\\Program Files\\Nexdrop\\extension' };
  },

  async getQueues(): Promise<QueueInfo[]> {
    if (isTauri()) {
      return await invoke<QueueInfo[]>('get_queues_cmd');
    }
    return mockQueues.map((q) => ({ ...q }));
  },

  async saveQueue(queue: DownloadQueue): Promise<DownloadQueue> {
    if (isTauri()) {
      return await invoke<DownloadQueue>('save_queue_cmd', { queue });
    }
    const saved: QueueInfo = {
      ...(mockQueues.find((q) => q.id === queue.id) ?? { open: true, active: 0, queued: 0 }),
      ...queue,
      id: queue.id || `q-${Date.now()}`,
    } as QueueInfo;
    mockQueues = mockQueues.some((q) => q.id === saved.id) ? mockQueues.map((q) => (q.id === saved.id ? saved : q)) : [...mockQueues, saved];
    return saved;
  },

  async deleteQueue(id: string): Promise<void> {
    if (isTauri()) {
      return await invoke('delete_queue_cmd', { id });
    }
    mockQueues = mockQueues.filter((q) => q.id !== id);
  },

  async moveToQueue(ids: string[], queueId: string): Promise<void> {
    if (isTauri()) {
      return await invoke('move_to_queue_cmd', { ids, queueId });
    }
    const set = new Set(ids);
    mockDownloads = mockDownloads.map((t) => (set.has(t.id) ? { ...t, queue_id: queueId } : t));
  },

  async startQueue(id: string): Promise<void> {
    if (isTauri()) {
      return await invoke('start_queue_cmd', { id });
    }
    mockQueues = mockQueues.map((q) => (q.id === id ? { ...q, open: true } : q));
  },

  async stopQueue(id: string): Promise<void> {
    if (isTauri()) {
      return await invoke('stop_queue_cmd', { id });
    }
  },

  onQueuesChanged(callback: () => void): Promise<UnlistenFn> {
    if (isTauri()) {
      return listen('queues-changed', () => callback());
    }
    return Promise.resolve(() => {});
  },

  /** Quits Nexdrop for real (closing the window only hides it to the tray). */
  async quitApp(): Promise<void> {
    if (isTauri()) {
      await invoke('quit_app_cmd');
    }
  },

  async getAutostart(): Promise<boolean> {
    if (isTauri()) {
      return await invoke<boolean>('get_autostart_cmd');
    }
    return false;
  },

  async setAutostart(enabled: boolean): Promise<boolean> {
    if (isTauri()) {
      return await invoke<boolean>('set_autostart_cmd', { enabled });
    }
    return enabled;
  },

  async getExtensionDir(): Promise<string> {
    if (isTauri()) {
      return await invoke<string>('get_extension_dir_cmd');
    }
    return 'C:\\Program Files\\Nexdrop\\extension';
  },

  async openExtensionDir(): Promise<void> {
    if (isTauri()) {
      await invoke('open_extension_dir_cmd');
    }
  },

  async getAppInfo(): Promise<{ name: string; version: string }> {
    if (isTauri()) {
      return await invoke<{ name: string; version: string }>('get_app_info_cmd');
    }
    return { name: 'Nexdrop', version: '1.2.0' };
  },

  /** Cancels a pending "sleep / shut down when finished" countdown. */
  async cancelCompletionAction(): Promise<boolean> {
    if (isTauri()) {
      return await invoke<boolean>('cancel_completion_action_cmd');
    }
    return false;
  },

  /** Looks inside a torrent (magnet link, link to a .torrent, or the file's bytes). */
  async probeTorrent(args: { url?: string; bytes?: number[] }): Promise<TorrentProbeInfo> {
    if (isTauri()) {
      return await invoke<TorrentProbeInfo>('probe_torrent_cmd', { url: args.url ?? null, bytes: args.bytes ?? null });
    }
    await new Promise((r) => setTimeout(r, 900));
    return {
      info_hash: '9b2f5e8c1d4a7036f8e1c2b5a9d0e3f47a6c8b15',
      name: 'Nexdrop Sample Collection',
      total_bytes: 2147483648,
      files: [
        { index: 0, path: 'Sample Collection/video/intro.mkv', length: 1288490188, selected: true },
        { index: 1, path: 'Sample Collection/video/outro.mkv', length: 524288000, selected: true },
        { index: 2, path: 'Sample Collection/audio/theme.flac', length: 262144000, selected: true },
        { index: 3, path: 'Sample Collection/README.txt', length: 2048, selected: true },
        { index: 4, path: 'Sample Collection/cover.jpg', length: 3145728, selected: true },
      ],
    };
  },

  async addTorrent(req: AddTorrentRequest): Promise<DownloadTask> {
    if (isTauri()) {
      return await invoke<DownloadTask>('add_torrent_cmd', { req });
    }
    const task: DownloadTask = {
      ...mockDownloads[0],
      id: `task-${Date.now()}`,
      filename: 'Nexdrop Sample Collection',
      status: 'downloading',
      downloaded_bytes: 0,
      info_hash: req.info_hash,
      created_at: Date.now(),
    };
    mockDownloads.unshift(task);
    return task;
  },

  async stopSeeding(id: string): Promise<void> {
    if (isTauri()) {
      return await invoke('stop_seeding_cmd', { id });
    }
    mockDownloads = mockDownloads.map((t) => (t.id === id ? { ...t, seeding: false } : t));
  },

  async startSeeding(id: string): Promise<void> {
    if (isTauri()) {
      return await invoke('start_seeding_cmd', { id });
    }
    mockDownloads = mockDownloads.map((t) => (t.id === id ? { ...t, seeding: true } : t));
  },

  onTorrentLive(callback: (live: TorrentLive) => void): Promise<UnlistenFn> {
    if (isTauri()) {
      return listen<TorrentLive>('torrent-live', (e) => callback(e.payload));
    }
    return Promise.resolve(() => {});
  },

  onProgress(callback: (event: DownloadProgressEvent) => void): Promise<UnlistenFn> {
    if (isTauri()) {
      return listen<DownloadProgressEvent>('download-progress', (e) => callback(e.payload));
    }
    // Browser preview: simulate the engine so the UI can be developed without Tauri
    const timer = window.setInterval(() => {
      mockDownloads = mockDownloads.map((t) => {
        if (t.status === 'queued') {
          const busy = mockDownloads.filter((d) => d.status === 'downloading').length;
          if (busy < mockSettings.max_active_downloads) return { ...t, status: 'downloading' };
          return t;
        }
        if (t.status !== 'downloading') return t;
        const speed = 6_000_000 + Math.random() * 12_000_000;
        const step = speed / 2;
        const downloaded = Math.min(t.total_bytes, t.downloaded_bytes + step);
        const segments = t.segments.map((seg) =>
          seg.status === 'completed'
            ? seg
            : {
                ...seg,
                downloaded_bytes: Math.min(seg.total_bytes, seg.downloaded_bytes + step / Math.max(1, t.segments.filter((x) => x.status !== 'completed').length)),
                status: seg.downloaded_bytes >= seg.total_bytes ? 'completed' : 'downloading',
              },
        );
        const done = downloaded >= t.total_bytes;
        const next: DownloadTask = {
          ...t,
          downloaded_bytes: downloaded,
          speed_bps: done ? 0 : speed,
          eta_seconds: done ? undefined : (t.total_bytes - downloaded) / speed,
          status: done ? 'completed' : 'downloading',
          completed_at: done ? Date.now() : undefined,
          segments,
        };
        callback({ id: t.id, downloaded_bytes: next.downloaded_bytes, total_bytes: next.total_bytes, speed_bps: next.speed_bps, eta_seconds: next.eta_seconds, status: next.status, segments });
        return next;
      });
    }, 500);
    return Promise.resolve(() => window.clearInterval(timer));
  },

  onCompleted(callback: (task: DownloadTask) => void): Promise<UnlistenFn> {
    if (isTauri()) {
      return listen<DownloadTask>('download-completed', (e) => callback(e.payload));
    }
    return Promise.resolve(() => {});
  },

  onStatusChanged(callback: (data: { id: string; status?: string }) => void): Promise<UnlistenFn> {
    if (isTauri()) {
      return listen<{ id: string; status?: string }>('download-status-changed', (e) => callback(e.payload));
    }
    return Promise.resolve(() => {});
  },
};
