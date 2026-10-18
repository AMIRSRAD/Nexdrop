//! Development helper: creates a torrent from a folder and seeds it on loopback.
//!
//!   cargo run --example seed_torrent -- <content-dir> <tracker-url> <port> <out.torrent>
//!
//! Used by `scripts/torrent_swarm.py` to give the real app something to download.
//! Prints `MAGNET <link>` and `READY` once the seed is verified and listening.

use std::path::PathBuf;
use std::time::Duration;

use librqbit::spawn_utils::BlockingSpawner;
use librqbit::{AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerOptions, Session, SessionOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 {
        eprintln!("usage: seed_torrent <content-dir> <tracker-url> <port> <out.torrent>");
        std::process::exit(2);
    }
    let content = PathBuf::from(&args[1]);
    let tracker = args[2].clone();
    let port: u16 = args[3].parse()?;
    let out = PathBuf::from(&args[4]);
    let name = content.file_name().and_then(|n| n.to_str()).unwrap_or("torrent").to_string();

    let created = librqbit::create_torrent(
        &content,
        CreateTorrentOptions { name: Some(&name), trackers: vec![tracker.clone()], piece_length: Some(256 * 1024) },
        &BlockingSpawner::new(2),
    )
    .await?;
    let bytes = created.as_bytes()?;
    std::fs::write(&out, &bytes)?;

    let session = Session::new_with_opts(
        content.clone(),
        SessionOptions {
            dht: None,
            disable_trackers: true,
            disable_local_service_discovery: true,
            listen: Some(ListenerOptions { listen_addr: format!("127.0.0.1:{port}").parse()?, ..Default::default() }),
            ..Default::default()
        },
    )
    .await?;
    let handle = session
        .add_torrent(
            AddTorrent::from_bytes(bytes.to_vec()),
            Some(AddTorrentOptions {
                output_folder: Some(content.to_string_lossy().to_string()),
                overwrite: true,
                ..Default::default()
            }),
        )
        .await?
        .into_handle()
        .expect("handle");
    handle.wait_until_completed().await?;

    let hash = created.info_hash().as_string();
    println!("MAGNET magnet:?xt=urn:btih:{hash}&dn={}&tr={}", name.replace(' ', "+"), tracker.replace(':', "%3A").replace('/', "%2F"));
    println!("HASH {hash}");
    println!("READY");
    loop {
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}
