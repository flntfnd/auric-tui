use crate::db::Database;
use crate::scan::{DirectoryScanner, ScanError, ScanOptions, ScanSummary};
use notify::{recommended_watcher, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct WatchOptions {
    pub debounce_ms: u64,
    pub poll_timeout_ms: u64,
    pub watched_only: bool,
    pub prune_missing: bool,
    pub scan_batch_size: usize,
    pub follow_symlinks: bool,
    pub read_embedded_artwork: bool,
    pub max_embedded_artwork_bytes: usize,
    pub scan_on_start: bool,
    pub max_runtime: Option<Duration>,
}

impl Default for WatchOptions {
    fn default() -> Self {
        Self {
            debounce_ms: 750,
            poll_timeout_ms: 250,
            watched_only: true,
            prune_missing: false,
            scan_batch_size: 2_000,
            follow_symlinks: false,
            read_embedded_artwork: true,
            max_embedded_artwork_bytes: 8 * 1024 * 1024,
            scan_on_start: false,
            max_runtime: None,
        }
    }
}

impl WatchOptions {
    pub fn scan_options(&self) -> ScanOptions {
        ScanOptions {
            batch_size: self.scan_batch_size.max(1),
            prune_missing: self.prune_missing,
            follow_symlinks: self.follow_symlinks,
            read_embedded_artwork: self.read_embedded_artwork,
            max_embedded_artwork_bytes: self.max_embedded_artwork_bytes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchRescan {
    pub root_path: String,
    pub reason: String,
    pub event_count: usize,
    pub summary: ScanSummary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchSessionSummary {
    pub watched_root_count: usize,
    pub skipped_root_count: usize,
    pub observed_notify_events: usize,
    pub ignored_notify_events: usize,
    pub rescans: Vec<WatchRescan>,
    pub elapsed_ms: u128,
}

#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    #[error("notify error: {0}")]
    Notify(#[from] notify::Error),
    #[error("db error: {0}")]
    Db(#[from] crate::db::DbError),
    #[error("scan error: {0}")]
    Scan(#[from] ScanError),
}

#[derive(Debug, Clone)]
pub struct WatchedFolderService {
    options: WatchOptions,
}

impl WatchedFolderService {
    pub fn new(options: WatchOptions) -> Self {
        Self { options }
    }

    pub fn options(&self) -> &WatchOptions {
        &self.options
    }

    pub fn watch_saved_roots(&self, db: &mut Database) -> Result<WatchSessionSummary, WatchError> {
        let roots = db.list_library_roots()?;
        let watched = roots
            .into_iter()
            .filter(|r| !self.options.watched_only || r.watched)
            .map(|r| WatchedRoot {
                path_string: r.path.clone(),
                path: PathBuf::from(r.path),
            })
            .collect::<Vec<_>>();

        self.watch_roots(db, watched)
    }

    pub fn watch_roots(
        &self,
        db: &mut Database,
        roots: Vec<WatchedRoot>,
    ) -> Result<WatchSessionSummary, WatchError> {
        let started = Instant::now();
        if roots.is_empty() {
            return Ok(WatchSessionSummary {
                watched_root_count: 0,
                skipped_root_count: 0,
                observed_notify_events: 0,
                ignored_notify_events: 0,
                rescans: Vec::new(),
                elapsed_ms: 0,
            });
        }

        let mut skipped_root_count = 0usize;
        let mut rescans = Vec::new();
        let mut active_roots = roots
            .into_iter()
            .filter(|root| {
                let watchable = root.path.is_dir();
                if !watchable {
                    skipped_root_count = skipped_root_count.saturating_add(1);
                }
                watchable
            })
            .collect::<Vec<_>>();
        if active_roots.is_empty() {
            return Ok(WatchSessionSummary {
                watched_root_count: 0,
                skipped_root_count,
                observed_notify_events: 0,
                ignored_notify_events: 0,
                rescans,
                elapsed_ms: started.elapsed().as_millis(),
            });
        }

        let scanner = DirectoryScanner::new(self.options.scan_options());
        if self.options.scan_on_start {
            for root in &active_roots {
                let summary = scanner.scan_path(db, &root.path)?;
                rescans.push(WatchRescan {
                    root_path: root.path_string.clone(),
                    reason: "startup".to_string(),
                    event_count: 0,
                    summary,
                });
            }
        }

        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher = build_watcher(tx)?;
        let mut final_roots = Vec::with_capacity(active_roots.len());
        for root in active_roots.drain(..) {
            match watcher.watch(&root.path, RecursiveMode::Recursive) {
                Ok(()) => final_roots.push(root),
                Err(err) => {
                    eprintln!(
                        "warning: could not watch root '{}': {err}",
                        root.path.display()
                    );
                    skipped_root_count = skipped_root_count.saturating_add(1);
                }
            }
        }
        if final_roots.is_empty() {
            return Ok(WatchSessionSummary {
                watched_root_count: 0,
                skipped_root_count,
                observed_notify_events: 0,
                ignored_notify_events: 0,
                rescans,
                elapsed_ms: started.elapsed().as_millis(),
            });
        }

        let mut observed_notify_events = 0usize;
        let mut ignored_notify_events = 0usize;
        let mut pending = PendingRoots::new(self.options.debounce_ms);
        let poll_timeout = Duration::from_millis(self.options.poll_timeout_ms.max(10));

        loop {
            let elapsed = started.elapsed();
            if self
                .options
                .max_runtime
                .is_some_and(|limit| elapsed >= limit)
            {
                break;
            }

            let timeout =
                compute_poll_timeout(poll_timeout, &pending, started, self.options.max_runtime);
            match rx.recv_timeout(timeout) {
                Ok(Ok(event)) => {
                    observed_notify_events += 1;
                    let now_ms = started.elapsed().as_millis() as u64;
                    let changed_roots = roots_for_event_paths(&final_roots, &event);
                    if changed_roots.is_empty() {
                        ignored_notify_events += 1;
                    } else {
                        for root in changed_roots {
                            pending.mark(root, now_ms);
                        }
                    }
                }
                Ok(Err(_notify_err)) => {
                    // Ignore per-event errors and continue; the session summary surfaces ignored counts
                    // and runtime returns only on setup/fatal scan/database failures.
                    ignored_notify_events += 1;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }

            drain_ready_rescans(
                &scanner,
                db,
                &mut rescans,
                &mut pending,
                started.elapsed().as_millis() as u64,
            )?;
        }

        // Drain any remaining debounced roots before exit.
        let remaining = pending.drain_all();
        for (root_path, event_count) in remaining {
            let summary = scanner.scan_path(db, Path::new(&root_path))?;
            rescans.push(WatchRescan {
                root_path,
                reason: "shutdown-flush".to_string(),
                event_count,
                summary,
            });
        }

        Ok(WatchSessionSummary {
            watched_root_count: final_roots.len(),
            skipped_root_count,
            observed_notify_events,
            ignored_notify_events,
            rescans,
            elapsed_ms: started.elapsed().as_millis(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedRoot {
    pub path_string: String,
    pub path: PathBuf,
}

fn build_watcher(
    tx: mpsc::Sender<notify::Result<Event>>,
) -> Result<RecommendedWatcher, notify::Error> {
    recommended_watcher(move |event| {
        let _ = tx.send(event);
    })
}

fn compute_poll_timeout(
    base_timeout: Duration,
    pending: &PendingRoots,
    started: Instant,
    max_runtime: Option<Duration>,
) -> Duration {
    let mut timeout = base_timeout;
    if let Some(next_ready_ms) = pending.next_ready_at_ms() {
        let now_ms = started.elapsed().as_millis() as u64;
        if next_ready_ms <= now_ms {
            timeout = Duration::from_millis(0);
        } else {
            timeout = timeout.min(Duration::from_millis(next_ready_ms.saturating_sub(now_ms)));
        }
    }
    if let Some(limit) = max_runtime {
        let remaining = limit.saturating_sub(started.elapsed());
        timeout = timeout.min(remaining);
    }
    timeout
}

fn drain_ready_rescans(
    scanner: &DirectoryScanner,
    db: &mut Database,
    rescans: &mut Vec<WatchRescan>,
    pending: &mut PendingRoots,
    now_ms: u64,
) -> Result<(), WatchError> {
    for (root_path, event_count) in pending.drain_ready(now_ms) {
        let summary = scanner.scan_path(db, Path::new(&root_path))?;
        rescans.push(WatchRescan {
            root_path,
            reason: "filesystem-change".to_string(),
            event_count,
            summary,
        });
    }
    Ok(())
}

fn roots_for_event_paths<'a>(roots: &'a [WatchedRoot], event: &Event) -> Vec<&'a str> {
    let mut matched = Vec::new();
    for path in &event.paths {
        if let Some(root) = best_matching_root(roots, path) {
            if !matched.contains(&root.path_string.as_str()) {
                matched.push(root.path_string.as_str());
            }
        }
    }
    matched
}

fn best_matching_root<'a>(roots: &'a [WatchedRoot], path: &Path) -> Option<&'a WatchedRoot> {
    roots
        .iter()
        .filter(|root| path.starts_with(&root.path))
        .max_by_key(|root| root.path.as_os_str().len())
}

#[derive(Debug, Clone, Default)]
struct PendingRoots {
    debounce_ms: u64,
    roots: BTreeMap<String, PendingRoot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingRoot {
    last_event_ms: u64,
    event_count: usize,
}

impl PendingRoots {
    fn new(debounce_ms: u64) -> Self {
        Self {
            debounce_ms,
            roots: BTreeMap::new(),
        }
    }

    fn mark(&mut self, root_path: &str, now_ms: u64) {
        let entry = self
            .roots
            .entry(root_path.to_string())
            .or_insert(PendingRoot {
                last_event_ms: now_ms,
                event_count: 0,
            });
        entry.last_event_ms = now_ms;
        entry.event_count = entry.event_count.saturating_add(1);
    }

    fn next_ready_at_ms(&self) -> Option<u64> {
        self.roots
            .values()
            .map(|pending| pending.last_event_ms.saturating_add(self.debounce_ms))
            .min()
    }

    fn drain_ready(&mut self, now_ms: u64) -> Vec<(String, usize)> {
        let mut ready_keys = Vec::new();
        for (root, pending) in &self.roots {
            if now_ms >= pending.last_event_ms.saturating_add(self.debounce_ms) {
                ready_keys.push(root.clone());
            }
        }
        let mut ready = Vec::with_capacity(ready_keys.len());
        for key in ready_keys {
            if let Some(pending) = self.roots.remove(&key) {
                ready.push((key, pending.event_count));
            }
        }
        ready
    }

    fn drain_all(&mut self) -> Vec<(String, usize)> {
        std::mem::take(&mut self.roots)
            .into_iter()
            .map(|(root, pending)| (root, pending.event_count))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_roots_debounces_multiple_events() {
        let mut pending = PendingRoots::new(200);
        pending.mark("/music/a", 10);
        pending.mark("/music/a", 20);
        pending.mark("/music/b", 40);

        assert_eq!(pending.next_ready_at_ms(), Some(220));
        assert_eq!(pending.drain_ready(219), Vec::<(String, usize)>::new());

        let ready = pending.drain_ready(240);
        assert_eq!(
            ready,
            vec![("/music/a".to_string(), 2), ("/music/b".to_string(), 1)]
        );
        assert!(pending.drain_all().is_empty());
    }

    #[test]
    fn chooses_most_specific_matching_root() {
        let roots = vec![
            WatchedRoot {
                path_string: "/music".to_string(),
                path: PathBuf::from("/music"),
            },
            WatchedRoot {
                path_string: "/music/live".to_string(),
                path: PathBuf::from("/music/live"),
            },
        ];
        let path = Path::new("/music/live/show/01.flac");
        let root = best_matching_root(&roots, path).unwrap();
        assert_eq!(root.path_string, "/music/live");
    }

    #[test]
    fn roots_for_event_collects_unique_matches() {
        let roots = vec![WatchedRoot {
            path_string: "/music".to_string(),
            path: PathBuf::from("/music"),
        }];
        let event = Event {
            kind: notify::event::EventKind::Create(notify::event::CreateKind::File),
            paths: vec![
                PathBuf::from("/music/a.flac"),
                PathBuf::from("/music/sub/b.flac"),
            ],
            attrs: Default::default(),
        };

        let matched = roots_for_event_paths(&roots, &event);
        assert_eq!(matched, vec!["/music"]);
    }
}
