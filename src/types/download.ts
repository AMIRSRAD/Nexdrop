export type DownloadStatus = 
  | 'queued' 
  | 'downloading' 
  | 'paused' 
  | 'completed' 
  | 'failed' 
  | 'scheduled' 
  | 'canceled';

export type FileCategory = 
  | 'all' 
  | 'compressed' 
  | 'documents' 
  | 'music' 
  | 'videos' 
  | 'programs' 
  | 'other';

export interface DownloadSegment {
  segment_index: number;
  start_byte: number;
  end_byte: number;
  downloaded_bytes: number;
  total_bytes: number;
  status: string;
}

export interface DownloadTask {
  id: string;
  url: string;
  final_url?: string;
  filename: string;
  destination_path: string;
  temp_path: string;
  total_bytes: number;
  downloaded_bytes: number;
  status: DownloadStatus;
  resumable: boolean;
  etag?: string;
  last_modified?: string;
  mime_type?: string;
  category: FileCategory;
  connections: number;
  speed_limit: number;
  priority: number;
  speed_bps: number;
  eta_seconds?: number;
  segments: DownloadSegment[];
  created_at: number;
  updated_at: number;
  completed_at?: number;
  error_message?: string;
  scheduled_time?: number;
  checksum_sha256?: string;
  checksum_md5?: string;
  user_agent?: string;
  referer?: string;
  mirrors?: string[];
  min_chunk_size?: number;
  proxy_mode?: 'system' | 'direct' | 'custom';
  custom_proxy_url?: string;
  antivirus_status?: 'clean' | 'threat_detected' | 'scanning' | 'skipped';
  /** Hash the user expects; checked automatically when the download finishes. */
  expected_checksum?: string;
  /** 'http' for a normal download, 'torrent' for BitTorrent. */
  kind?: 'http' | 'torrent';
  /** The queue this download belongs to. */
  queue_id?: string;
  info_hash?: string;
  uploaded_bytes?: number;
  peers?: number;
  /** A finished torrent that is still being shared. */
  seeding?: boolean;
  torrent_files?: TorrentFile[];
  /** Live upload speed (bytes/s); torrents only, never stored. */
  upload_bps?: number;
}

export interface TorrentFile {
  index: number;
  path: string;
  length: number;
  selected: boolean;
}

export interface TorrentProbeInfo {
  info_hash: string;
  name: string;
  total_bytes: number;
  files: TorrentFile[];
}

export interface AddTorrentRequest {
  source?: string;
  info_hash?: string;
  destination_path?: string;
  /** Indices of the files to download; omit for all. */
  selected_files?: number[];
  queue_id?: string;
}

export interface TorrentLive {
  id: string;
  uploaded_bytes: number;
  upload_bps: number;
  peers: number;
  seeding: boolean;
  ratio: number;
}

export interface SiteCredential {
  id: string;
  domain: string;
  username?: string;
  password?: string;
  token_or_cookie?: string;
  notes?: string;
}

export interface AppSettings {
  default_destination: string;
  max_active_downloads: number;
  default_connections: number;
  max_connections_per_server: number;
  min_chunk_size: number;
  global_speed_limit: number;
  auto_resume_on_launch: boolean;
  notify_on_complete: boolean;
  theme: string;
  clipboard_watch: boolean;
  auto_categorize: boolean;
  proxy_mode: 'system' | 'direct' | 'custom';
  custom_proxy_url?: string;
  on_complete_action: 'none' | 'notify' | 'sleep' | 'shutdown';
  sound_enabled: boolean;
  scheduler_enabled: boolean;
  scheduler_start_time?: string;
  scheduler_stop_time?: string;
  scheduler_days: string[];
  scheduler_speed_limit: number;
  scheduler_action: 'none' | 'sleep' | 'shutdown' | 'exit';
  scan_with_antivirus: boolean;
  antivirus_path?: string;
  torrent_seed_after_download: boolean;
  /** Stop sharing at this upload/download ratio (0 = never stop on ratio). */
  torrent_ratio_limit: number;
  /** Bytes per second, 0 = unlimited. */
  torrent_upload_limit: number;
  torrent_dht: boolean;
  torrent_upnp: boolean;
}

export interface UrlProbeResult {
  url: string;
  filename: string;
  total_bytes: number;
  resumable: boolean;
  mime_type?: string;
  category: FileCategory;
  default_destination: string;
}

export interface AddDownloadRequest {
  url: string;
  filename?: string;
  destination_path?: string;
  connections?: number;
  speed_limit?: number;
  priority?: number;
  scheduled_time?: number;
  user_agent?: string;
  referer?: string;
  expected_checksum?: string;
  mirrors?: string[];
  min_chunk_size?: number;
  proxy_mode?: 'system' | 'direct' | 'custom';
  custom_proxy_url?: string;
  /** Extra request headers (e.g. Cookie / Authorization imported from cURL). */
  custom_headers?: Record<string, string>;
  queue_id?: string;
}

export interface DownloadProgressEvent {
  id: string;
  downloaded_bytes: number;
  total_bytes: number;
  speed_bps: number;
  eta_seconds?: number;
  status: DownloadStatus;
  segments: DownloadSegment[];
}

export interface ChecksumResult {
  algorithm: string;
  calculated_hash: string;
  expected_hash?: string;
  matches: boolean;
}

export interface GrabbedLink {
  url: string;
  filename: string;
  category: string;
  extension: string;
}

export type QueueAction = 'none' | 'notify' | 'exit' | 'sleep' | 'hibernate' | 'shutdown' | 'lock' | 'program';

/** A named list of downloads with its own schedule, limits and "when finished" action. */
export interface DownloadQueue {
  id: string;
  name: string;
  schedule_enabled: boolean;
  days: string[];
  start_time: string;
  /** Without a stop time the queue starts at the start time and runs until it is empty. */
  stop_time?: string | null;
  /** Simultaneous downloads (0 = the global limit). */
  max_active: number;
  /** Bytes per second shared by the whole queue (0 = unlimited). */
  speed_limit: number;
  after_done: QueueAction;
  after_done_program?: string | null;
  retry_count: number;
  retry_delay_secs: number;
  position: number;
}

export interface QueueInfo extends DownloadQueue {
  /** Allowed to start downloads right now. */
  open: boolean;
  active: number;
  queued: number;
  /** Unix seconds when the schedule next opens (while closed). */
  next_start?: number | null;
}
