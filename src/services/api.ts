import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  AddDownloadRequest,
  AppSettings,
  DownloadProgressEvent,
  DownloadTask,
  SiteCredential,
  UrlProbeResult,
  GrabbedLink,
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

let mockDownloads: DownloadTask[] = [
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
};

export const api = {
  async probeUrl(url: string): Promise<UrlProbeResult> {
    if (isTauri()) {
      return await invoke<UrlProbeResult>('probe_url_cmd', { url });
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
    alert(`Open file: ${id}`);
  },

  async revealFile(id: string): Promise<void> {
    if (isTauri()) {
      return await invoke('reveal_file_cmd', { id });
    }
    alert(`Reveal file: ${id}`);
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

  onProgress(callback: (event: DownloadProgressEvent) => void): Promise<UnlistenFn> {
    if (isTauri()) {
      return listen<DownloadProgressEvent>('download-progress', (e) => callback(e.payload));
    }
    return Promise.resolve(() => {});
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
