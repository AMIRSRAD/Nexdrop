pub mod checksum;
pub mod client;
pub mod downloader;
pub mod manager;
pub mod probe;
pub mod scheduler;
pub mod segment;
pub mod speed;

pub use checksum::{compute_checksums, verify_file_checksum};
pub use client::{create_download_client_with_proxy, create_http_client, create_http_client_with_proxy};
pub use downloader::DownloadEngine;
pub use manager::DownloadManager;
pub use probe::probe_url;
pub use scheduler::DownloadScheduler;
pub use segment::{calculate_segments, calculate_segments_with_chunk_size};
