//! Torrent engine tests over a real BitTorrent swarm on loopback: one librqbit
//! session seeds a torrent and our engine downloads it from that peer.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use librqbit::spawn_utils::BlockingSpawner;
use librqbit::{AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerOptions, Session, SessionOptions};
use tokio::sync::watch;

use nexdrop_lib::db::models::{DownloadStatus, DownloadTask, FileCategory, TorrentFile};
use nexdrop_lib::engine::torrent::{TorrentEngine, TorrentProbe, TorrentSettings, TorrentSource};

fn patterned(len: usize, seed: usize) -> Vec<u8> {
    (0..len).map(|i| ((i * 31 + seed * 7 + 5) % 251) as u8).collect()
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nexdrop_tor_{tag}_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

struct Swarm {
    _seeder: Arc<Session>,
    addr: SocketAddr,
    torrent_bytes: Vec<u8>,
}

/// Builds a torrent from `content_dir` and starts a seeder for it on 127.0.0.1.
async fn start_swarm(content_dir: &Path, name: &str) -> Swarm {
    let created = librqbit::create_torrent(
        content_dir,
        CreateTorrentOptions { name: Some(name), trackers: vec![], piece_length: Some(32 * 1024) },
        &BlockingSpawner::new(2),
    )
    .await
    .expect("create torrent");
    let torrent_bytes = created.as_bytes().expect("torrent bytes").to_vec();

    let seeder = Session::new_with_opts(
        content_dir.to_path_buf(),
        SessionOptions {
            dht: None,
            disable_trackers: true,
            disable_local_service_discovery: true,
            listen: Some(ListenerOptions { listen_addr: "127.0.0.1:0".parse().unwrap(), ..Default::default() }),
            ..Default::default()
        },
    )
    .await
    .expect("seeder session");

    let handle = seeder
        .add_torrent(
            AddTorrent::from_bytes(torrent_bytes.clone()),
            Some(AddTorrentOptions {
                output_folder: Some(content_dir.to_string_lossy().to_string()),
                overwrite: true,
                ..Default::default()
            }),
        )
        .await
        .expect("seed add")
        .into_handle()
        .expect("handle");
    tokio::time::timeout(Duration::from_secs(30), handle.wait_until_completed())
        .await
        .expect("seeder verification timed out")
        .expect("seeder verification failed");

    let port = seeder.listen_addr().expect("seeder listen addr").port();
    Swarm { _seeder: seeder, addr: format!("127.0.0.1:{port}").parse().unwrap(), torrent_bytes }
}

fn settings(download_limit: u64) -> TorrentSettings {
    TorrentSettings {
        upload_limit: 0,
        download_limit,
        dht: false,
        upnp: false,
        seed_after_download: true,
        default_dir: temp_dir("session"),
    }
}

fn task_for(probe: &TorrentProbe, dest: &Path, selected: &[usize]) -> DownloadTask {
    let files: Vec<TorrentFile> = probe
        .files
        .iter()
        .map(|f| TorrentFile { selected: selected.contains(&f.index), ..f.clone() })
        .collect();
    let total: u64 = files.iter().filter(|f| f.selected).map(|f| f.length).sum();
    DownloadTask {
        id: format!("tor-{}", &probe.info_hash[..8]),
        url: format!("torrent:{}", probe.info_hash),
        final_url: None,
        filename: probe.name.clone(),
        destination_path: dest.to_string_lossy().to_string(),
        temp_path: String::new(),
        total_bytes: total as i64,
        downloaded_bytes: 0,
        status: DownloadStatus::Downloading,
        resumable: true,
        etag: None,
        last_modified: None,
        mime_type: None,
        category: FileCategory::Other,
        connections: 1,
        speed_limit: 0,
        priority: 0,
        speed_bps: 0,
        eta_seconds: None,
        segments: vec![],
        created_at: 0,
        updated_at: 0,
        completed_at: None,
        error_message: None,
        scheduled_time: None,
        checksum_sha256: None,
        checksum_md5: None,
        user_agent: None,
        referer: None,
        mirrors: vec![],
        min_chunk_size: 0,
        proxy_mode: None,
        custom_proxy_url: None,
        antivirus_status: None,
        expected_checksum: None,
        custom_headers: Default::default(),
        kind: "torrent".into(),
        info_hash: Some(probe.info_hash.clone()),
        uploaded_bytes: 0,
        peers: 0,
        seeding: false,
        torrent_files: files,
        queue_id: "main".to_string(),
    }
}

async fn run(
    engine: &TorrentEngine,
    task: DownloadTask,
    bytes: &[u8],
    resume: bool,
    pause: watch::Receiver<bool>,
    s: &TorrentSettings,
) -> Result<DownloadTask, String> {
    engine.run(task, Some(bytes.to_vec()), resume, pause, s, |_| {}, |_| {}).await
}

fn index_of(probe: &TorrentProbe, name: &str) -> usize {
    probe.files.iter().find(|f| f.path.ends_with(name)).unwrap_or_else(|| panic!("{name} not in {:?}", probe.files)).index
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn probe_lists_files_and_downloads_only_the_selected_ones() {
    let content = temp_dir("album");
    let track1 = patterned(700_000, 1);
    let track2 = patterned(500_000, 2);
    let cover = patterned(100_000, 3);
    std::fs::write(content.join("track1.bin"), &track1).unwrap();
    std::fs::write(content.join("track2.bin"), &track2).unwrap();
    std::fs::write(content.join("cover.bin"), &cover).unwrap();
    let swarm = start_swarm(&content, "album").await;

    let engine = TorrentEngine::new().with_initial_peers(vec![swarm.addr]);
    let s = settings(0);
    let probe = engine.probe(TorrentSource::Bytes(swarm.torrent_bytes.clone()), &s).await.expect("probe");

    assert_eq!(probe.name, "album");
    assert_eq!(probe.files.len(), 3);
    assert_eq!(probe.total_bytes, 1_300_000);
    assert_eq!(probe.info_hash.len(), 40);
    assert!(engine.cached_torrent_bytes(&probe.info_hash).is_some(), "probe should be cached for the add step");

    // Take everything except track2.
    let wanted = [index_of(&probe, "track1.bin"), index_of(&probe, "cover.bin")];
    let dest = temp_dir("dest");
    let task = task_for(&probe, &dest, &wanted);
    assert_eq!(task.total_bytes, 800_000);

    let events = Arc::new(AtomicUsize::new(0));
    let saw_segments = Arc::new(Mutex::new(false));
    let (e2, s2) = (events.clone(), saw_segments.clone());
    let (_tx, rx) = watch::channel(false);
    let done = engine
        .run(
            task,
            Some(swarm.torrent_bytes.clone()),
            false,
            rx,
            &s,
            move |ev| {
                e2.fetch_add(1, Ordering::SeqCst);
                if ev.segments.len() == 2 {
                    *s2.lock().unwrap() = true;
                }
            },
            |_| {},
        )
        .await
        .expect("torrent download");

    assert_eq!(done.status, DownloadStatus::Completed);
    assert!(done.seeding, "finished torrents keep sharing by default");
    assert!(events.load(Ordering::SeqCst) >= 1);
    assert!(*saw_segments.lock().unwrap(), "byte map should show one segment per selected file");

    let folder = dest.join("album");
    assert_eq!(std::fs::read(folder.join("track1.bin")).unwrap(), track1, "track1 differs");
    assert_eq!(std::fs::read(folder.join("cover.bin")).unwrap(), cover, "cover differs");
    let unwanted = std::fs::read(folder.join("track2.bin")).ok();
    assert!(unwanted.as_deref() != Some(&track2[..]), "an unselected file must not be downloaded");

    // It is now being shared; stopping and removing work and leave the files alone.
    assert!(engine.live(&done).is_some());
    engine.stop(&probe.info_hash).await;
    engine.remove(&probe.info_hash, false).await;
    assert!(folder.join("track1.bin").exists(), "removing from the engine must keep the files");
    let _ = std::fs::remove_dir_all(&content);
    let _ = std::fs::remove_dir_all(&dest);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn single_file_torrent_pauses_and_resumes_byte_exact() {
    let content = temp_dir("single");
    let payload = patterned(1_500_000, 9);
    std::fs::write(content.join("movie.bin"), &payload).unwrap();
    let swarm = start_swarm(&content, "movie.bin").await;

    let engine = TorrentEngine::new().with_initial_peers(vec![swarm.addr]);
    let s = settings(300_000); // 300 KB/s: slow enough for the pause to land mid-way
    let probe = engine.probe(TorrentSource::Bytes(swarm.torrent_bytes.clone()), &s).await.expect("probe");
    assert_eq!(probe.files.len(), 1);

    let dest = temp_dir("dest1");
    let task = task_for(&probe, &dest, &[0]);

    let (pause_tx, pause_rx) = watch::channel(false);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1800)).await;
        let _ = pause_tx.send(true);
    });
    let paused = run(&engine, task, &swarm.torrent_bytes, false, pause_rx, &s).await.expect("first run");
    assert_eq!(paused.status, DownloadStatus::Paused, "finished before the pause landed");
    assert!(paused.downloaded_bytes > 0 && paused.downloaded_bytes < payload.len() as i64, "{}", paused.downloaded_bytes);

    let mut again = paused;
    again.status = DownloadStatus::Downloading;
    let (_tx, rx) = watch::channel(false);
    let done = run(&engine, again, &swarm.torrent_bytes, true, rx, &s).await.expect("resumed run");
    assert_eq!(done.status, DownloadStatus::Completed);
    // Single-file torrents land directly in the chosen folder.
    assert_eq!(std::fs::read(dest.join("movie.bin")).unwrap(), payload);
    engine.remove(&probe.info_hash, false).await;
    let _ = std::fs::remove_dir_all(&content);
    let _ = std::fs::remove_dir_all(&dest);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_new_torrent_never_overwrites_files_already_in_the_folder() {
    let content = temp_dir("clobber");
    let payload = patterned(400_000, 4);
    std::fs::write(content.join("data.bin"), &payload).unwrap();
    let swarm = start_swarm(&content, "data.bin").await;

    let engine = TorrentEngine::new().with_initial_peers(vec![swarm.addr]);
    let s = settings(0);
    let probe = engine.probe(TorrentSource::Bytes(swarm.torrent_bytes.clone()), &s).await.expect("probe");

    let dest = temp_dir("dest2");
    std::fs::write(dest.join("data.bin"), b"precious unrelated file").unwrap();
    let task = task_for(&probe, &dest, &[0]);
    let (_tx, rx) = watch::channel(false);
    let result = run(&engine, task, &swarm.torrent_bytes, false, rx, &s).await;

    assert!(result.is_err(), "a fresh add must not reuse or overwrite existing files");
    assert_eq!(std::fs::read(dest.join("data.bin")).unwrap(), b"precious unrelated file");
    let _ = std::fs::remove_dir_all(&content);
    let _ = std::fs::remove_dir_all(&dest);
}
