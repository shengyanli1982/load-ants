use dashmap::DashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::AbortHandle;
use tokio::time::interval;
use tracing::debug;

struct IpCounter {
    count: AtomicU64,
    window_start_secs: AtomicU64,
}

pub struct RateLimiter {
    counters: Arc<DashMap<IpAddr, IpCounter>>,
    max_per_second: u32,
    per_ip_max_per_second: u32,
    global_count: AtomicU64,
    global_window_start_secs: AtomicU64,
    cached_epoch_secs: Arc<AtomicU64>,
    cleanup_handle: AbortHandle,
}

fn current_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn reset_if_needed(count: &AtomicU64, window_start_secs: &AtomicU64, now_secs: u64) -> u64 {
    let window = window_start_secs.load(Ordering::Acquire);
    if now_secs > window
        && window_start_secs
            .compare_exchange(window, now_secs, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        count.store(1, Ordering::Release);
        return 0;
    }
    count.fetch_add(1, Ordering::AcqRel)
}

impl RateLimiter {
    pub fn new(max_per_second: u32) -> Arc<Self> {
        Self::new_with_per_ip(max_per_second, max_per_second)
    }

    pub fn new_with_per_ip(max_per_second: u32, per_ip_max_per_second: u32) -> Arc<Self> {
        let now_secs = current_epoch_secs();
        let counters = Arc::new(DashMap::new());
        let cached = Arc::new(AtomicU64::new(now_secs));

        let cleanup_handle = {
            let counters_clone = counters.clone();
            let cached_clone = cached.clone();
            let handle = tokio::spawn(async move {
                let mut tick = interval(Duration::from_secs(1));
                let mut cleanup_cycles = 0u32;
                loop {
                    tick.tick().await;
                    let fresh = current_epoch_secs();
                    cached_clone.store(fresh, Ordering::Relaxed);
                    cleanup_cycles += 1;
                    if cleanup_cycles >= 60 {
                        cleanup_cycles = 0;
                        counters_clone.retain(|_, v: &mut IpCounter| {
                            fresh - v.window_start_secs.load(Ordering::Relaxed) < 120
                        });
                        debug!("Rate limiter cleanup, active IPs: {}", counters_clone.len());
                    }
                }
            });
            handle.abort_handle()
        };

        Arc::new(Self {
            counters,
            max_per_second,
            per_ip_max_per_second,
            global_count: AtomicU64::new(0),
            global_window_start_secs: AtomicU64::new(now_secs),
            cached_epoch_secs: cached,
            cleanup_handle,
        })
    }

    pub fn check(&self, ip: IpAddr) -> bool {
        let now_secs = self.cached_epoch_secs.load(Ordering::Relaxed);

        let global = reset_if_needed(&self.global_count, &self.global_window_start_secs, now_secs);
        if global >= self.max_per_second as u64 {
            return false;
        }

        let entry = self.counters.entry(ip).or_insert_with(|| IpCounter {
            count: AtomicU64::new(0),
            window_start_secs: AtomicU64::new(now_secs),
        });

        let per_ip = reset_if_needed(&entry.count, &entry.window_start_secs, now_secs);
        per_ip < self.per_ip_max_per_second as u64
    }

    pub fn max_per_second(&self) -> u32 {
        self.max_per_second
    }
}

impl Drop for RateLimiter {
    fn drop(&mut self) {
        self.cleanup_handle.abort();
    }
}
