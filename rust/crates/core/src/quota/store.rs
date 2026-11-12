use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, SystemTime};

use super::client::{Clock, QuotaClient};
use super::model::{Provider, QuotaError, QuotaSnapshot};

pub const POLL_INTERVAL: Duration = Duration::from_secs(300);
const MIN_REFRESH_GAP: Duration = Duration::from_secs(300);

pub type ObserverId = u64;
pub type FetchFn = Arc<dyn Fn(Provider, bool) -> Result<QuotaSnapshot, QuotaError> + Send + Sync>;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct QuotaState {
    pub snapshot: Option<QuotaSnapshot>,
    pub error: Option<QuotaError>,
    pub last_updated: Option<SystemTime>,
    pub is_loading: bool,
}

impl QuotaState {
    pub fn retry_after(&self) -> Option<SystemTime> {
        match self.error {
            Some(QuotaError::RateLimited { until }) => Some(until),
            _ => None,
        }
    }
}

/// Completes when a refresh request has finished or been abandoned.
#[derive(Clone)]
pub struct Refresh {
    done: Arc<(Mutex<bool>, Condvar)>,
}

impl Refresh {
    fn new() -> Self {
        Self { done: Arc::new((Mutex::new(false), Condvar::new())) }
    }

    fn finish(&self) {
        *self.done.0.lock().unwrap() = true;
        self.done.1.notify_all();
    }

    pub fn wait(&self) {
        let mut done = self.done.0.lock().unwrap();
        while !*done {
            done = self.done.1.wait(done).unwrap();
        }
    }
}

struct Request {
    id: u64,
    refresh: Refresh,
}

#[derive(Default)]
struct Inner {
    states: HashMap<Provider, QuotaState>,
    observers: HashSet<ObserverId>,
    requests: HashMap<Provider, Request>,
    last_attempt: HashMap<Provider, SystemTime>,
    next_request: u64,
    polling_epoch: u64,
}

struct Shared {
    fetch: FetchFn,
    now: Clock,
    poll_interval: Duration,
    inner: Mutex<Inner>,
    wake: Condvar,
    subscribers: Mutex<Vec<Arc<dyn Fn() + Send + Sync>>>,
}

/// Cached quota state for every provider. Providers refresh in parallel on worker
/// threads while at least one observer is registered; results that arrive after the
/// last observer leaves are discarded.
#[derive(Clone)]
pub struct QuotaStore {
    shared: Arc<Shared>,
}

impl Default for QuotaStore {
    fn default() -> Self {
        let client = QuotaClient::default();
        Self::new(Arc::new(move |provider, interaction| client.fetch(provider, interaction)), Arc::new(SystemTime::now))
    }
}

impl QuotaStore {
    pub fn new(fetch: FetchFn, now: Clock) -> Self {
        Self::with_poll_interval(fetch, now, POLL_INTERVAL)
    }

    pub fn with_poll_interval(fetch: FetchFn, now: Clock, poll_interval: Duration) -> Self {
        Self {
            shared: Arc::new(Shared {
                fetch,
                now,
                poll_interval,
                inner: Mutex::new(Inner::default()),
                wake: Condvar::new(),
                subscribers: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Registers a callback run after any state change, on whichever thread changed it.
    pub fn subscribe(&self, callback: impl Fn() + Send + Sync + 'static) {
        self.shared.subscribers.lock().unwrap().push(Arc::new(callback));
    }

    pub fn state(&self, provider: Provider) -> QuotaState {
        self.lock().states.get(&provider).cloned().unwrap_or_default()
    }

    pub fn observe(&self, id: ObserverId) {
        {
            let mut inner = self.lock();
            if !inner.observers.insert(id) || inner.observers.len() != 1 {
                return;
            }
            inner.polling_epoch += 1;
            let epoch = inner.polling_epoch;
            drop(inner);
            self.spawn_polling(epoch);
        }
        for provider in Provider::ALL {
            self.refresh(provider, false, false);
        }
    }

    pub fn stop_observing(&self, id: ObserverId) {
        let mut inner = self.lock();
        if !inner.observers.remove(&id) || !inner.observers.is_empty() {
            return;
        }
        inner.polling_epoch += 1;
        // A request may still complete after this, including a blocked Keychain read.
        let abandoned: Vec<(Provider, Request)> = inner.requests.drain().collect();
        for (provider, _) in &abandoned {
            inner.states.entry(*provider).or_default().is_loading = false;
        }
        drop(inner);
        self.shared.wake.notify_all();
        for (_, request) in abandoned {
            request.refresh.finish();
        }
        self.notify();
    }

    /// Starts a refresh unless one is in flight, the provider is cooling down after a
    /// rate limit, or a recent attempt makes it unnecessary. Returns the refresh that
    /// is (or already was) running.
    pub fn refresh(&self, provider: Provider, allow_interaction: bool, force: bool) -> Option<Refresh> {
        let mut inner = self.lock();
        if inner.observers.is_empty() {
            return None;
        }
        if let Some(pending) = inner.requests.get(&provider) {
            return Some(pending.refresh.clone());
        }
        let state = inner.states.get(&provider).cloned().unwrap_or_default();
        let now = (self.shared.now)();
        if state.retry_after().is_some_and(|retry| retry > now) {
            return None;
        }
        if !force && (state.snapshot.is_some() || state.error.is_some()) {
            let recent = inner
                .last_attempt
                .get(&provider)
                .and_then(|attempt| now.duration_since(*attempt).ok())
                .is_some_and(|elapsed| elapsed < MIN_REFRESH_GAP);
            if recent {
                return None;
            }
        }

        inner.next_request += 1;
        let id = inner.next_request;
        let refresh = Refresh::new();
        inner.last_attempt.insert(provider, now);
        let slot = inner.states.entry(provider).or_default();
        slot.is_loading = true;
        slot.snapshot = None;
        slot.error = None;
        inner.requests.insert(provider, Request { id, refresh: refresh.clone() });
        drop(inner);
        self.notify();

        let store = self.clone();
        let fetch = self.shared.fetch.clone();
        let handle = refresh.clone();
        thread::spawn(move || {
            let result = fetch(provider, allow_interaction);
            store.complete(provider, id, result);
            handle.finish();
        });
        Some(refresh)
    }

    fn complete(&self, provider: Provider, id: u64, result: Result<QuotaSnapshot, QuotaError>) {
        let mut inner = self.lock();
        if inner.requests.get(&provider).map(|r| r.id) != Some(id) {
            return;
        }
        inner.requests.remove(&provider);
        let now = (self.shared.now)();
        let state = inner.states.entry(provider).or_default();
        state.is_loading = false;
        match result {
            Ok(snapshot) => {
                state.snapshot = Some(snapshot);
                state.last_updated = Some(now);
            }
            Err(error) => state.error = Some(error),
        }
        drop(inner);
        self.notify();
    }

    fn spawn_polling(&self, epoch: u64) {
        let store = self.clone();
        thread::spawn(move || loop {
            let mut inner = store.lock();
            loop {
                if inner.polling_epoch != epoch {
                    return;
                }
                let (guard, timeout) = store.shared.wake.wait_timeout(inner, store.shared.poll_interval).unwrap();
                inner = guard;
                if timeout.timed_out() {
                    break;
                }
            }
            let current = inner.polling_epoch == epoch;
            drop(inner);
            if !current {
                return;
            }
            for provider in Provider::ALL {
                store.refresh(provider, false, false);
            }
        });
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.shared.inner.lock().unwrap()
    }

    fn notify(&self) {
        let subscribers = self.shared.subscribers.lock().unwrap().clone();
        for callback in subscribers {
            callback();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quota::model::{QuotaWindow, WindowKind};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::UNIX_EPOCH;

    fn snapshot(percent: f64) -> QuotaSnapshot {
        QuotaSnapshot {
            five_hour: Some(QuotaWindow { kind: WindowKind::FiveHour, remaining_percent: percent, resets_at: None }),
            weekly: None,
        }
    }

    /// A fetch that blocks each provider until the test releases it.
    struct Probe {
        started: Mutex<HashMap<Provider, usize>>,
        started_cv: Condvar,
        released: Mutex<HashSet<Provider>>,
        released_cv: Condvar,
    }

    impl Probe {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                started: Mutex::new(HashMap::new()),
                started_cv: Condvar::new(),
                released: Mutex::new(HashSet::new()),
                released_cv: Condvar::new(),
            })
        }

        fn fetch(self: &Arc<Self>) -> FetchFn {
            let probe = self.clone();
            Arc::new(move |provider, _| {
                *probe.started.lock().unwrap().entry(provider).or_default() += 1;
                probe.started_cv.notify_all();
                let mut released = probe.released.lock().unwrap();
                while !released.remove(&provider) {
                    released = probe.released_cv.wait(released).unwrap();
                }
                Ok(snapshot(75.0))
            })
        }

        fn wait_for_request(&self, provider: Provider) {
            let mut started = self.started.lock().unwrap();
            while started.get(&provider).copied().unwrap_or(0) == 0 {
                started = self.started_cv.wait(started).unwrap();
            }
        }

        fn count(&self, provider: Provider) -> usize {
            self.started.lock().unwrap().get(&provider).copied().unwrap_or(0)
        }

        fn finish(&self, provider: Provider) {
            self.released.lock().unwrap().insert(provider);
            self.released_cv.notify_all();
        }
    }

    /// Waits for the refreshes started by `observe` to land.
    fn settle(store: &QuotaStore) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while Provider::ALL.iter().any(|p| store.state(*p).is_loading) {
            assert!(std::time::Instant::now() < deadline, "refresh never finished");
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn clock(secs: u64) -> Clock {
        Arc::new(move || UNIX_EPOCH + Duration::from_secs(secs))
    }

    #[test]
    fn observers_deduplicate_and_last_removal_rejects_late_results() {
        let probe = Probe::new();
        let store = QuotaStore::new(probe.fetch(), clock(1_800_000_000));
        store.observe(1);
        store.observe(2);
        let pending = store.refresh(Provider::Codex, false, true).unwrap();
        probe.wait_for_request(Provider::Codex);
        probe.wait_for_request(Provider::Claude);
        assert_eq!(probe.count(Provider::Codex), 1);
        store.stop_observing(1);
        assert!(store.state(Provider::Codex).is_loading);
        store.stop_observing(2);
        assert!(!store.state(Provider::Codex).is_loading);
        assert!(store.refresh(Provider::Codex, false, true).is_none());
        probe.finish(Provider::Codex);
        probe.finish(Provider::Claude);
        pending.wait();
        thread::sleep(Duration::from_millis(50));
        assert_eq!(store.state(Provider::Codex).snapshot, None);
    }

    #[test]
    fn slow_provider_does_not_block_other_providers() {
        let probe = Probe::new();
        let store = QuotaStore::new(probe.fetch(), clock(1_800_000_000));
        store.observe(1);
        let codex = store.refresh(Provider::Codex, false, false).unwrap();
        probe.wait_for_request(Provider::Codex);
        probe.wait_for_request(Provider::Claude);
        probe.finish(Provider::Codex);
        codex.wait();
        assert_eq!(store.state(Provider::Codex).snapshot.unwrap().ring_window().unwrap().remaining_percent, 75.0);
        assert!(store.state(Provider::Claude).is_loading);
        store.stop_observing(1);
        probe.finish(Provider::Claude);
    }

    #[test]
    fn manual_refresh_cannot_bypass_provider_cooldown() {
        let retry = UNIX_EPOCH + Duration::from_secs(1_800_000_600);
        let store = QuotaStore::new(
            Arc::new(move |provider, _| match provider {
                Provider::Codex => Err(QuotaError::RateLimited { until: retry }),
                Provider::Claude => Ok(snapshot(75.0)),
            }),
            clock(1_800_000_000),
        );
        store.observe(1);
        settle(&store);
        assert_eq!(store.state(Provider::Codex).error, Some(QuotaError::RateLimited { until: retry }));
        assert!(store.refresh(Provider::Codex, false, true).is_none());
        assert!(store.refresh(Provider::Codex, true, true).is_none());
        store.stop_observing(1);
        store.observe(1);
        assert!(store.refresh(Provider::Codex, false, true).is_none());
        store.stop_observing(1);
    }

    #[test]
    fn recent_results_are_cached_until_forced_or_stale() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let now = Arc::new(Mutex::new(1_800_000_000u64));
        let clock_now = now.clone();
        let store = QuotaStore::new(
            Arc::new(move |_, _| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(snapshot(50.0))
            }),
            Arc::new(move || UNIX_EPOCH + Duration::from_secs(*clock_now.lock().unwrap())),
        );
        store.observe(1);
        settle(&store);
        let baseline = calls.load(Ordering::SeqCst);
        assert_eq!(baseline, 2);

        assert!(store.refresh(Provider::Codex, false, false).is_none());
        *now.lock().unwrap() += 299;
        assert!(store.refresh(Provider::Codex, false, false).is_none());
        *now.lock().unwrap() += 1;
        store.refresh(Provider::Codex, false, false).unwrap().wait();
        assert_eq!(calls.load(Ordering::SeqCst), baseline + 1);
        store.refresh(Provider::Codex, false, true).unwrap().wait();
        assert_eq!(calls.load(Ordering::SeqCst), baseline + 2);
        store.stop_observing(1);
    }

    #[test]
    fn refresh_without_observers_does_nothing() {
        let store = QuotaStore::new(Arc::new(|_, _| panic!("must not fetch")), clock(0));
        assert!(store.refresh(Provider::Codex, false, true).is_none());
    }

    #[test]
    fn failures_are_stored_and_loading_state_resets() {
        let store = QuotaStore::new(Arc::new(|_, _| Err(QuotaError::Network)), clock(1_800_000_000));
        store.observe(1);
        settle(&store);
        let state = store.state(Provider::Codex);
        assert_eq!(state.error, Some(QuotaError::Network));
        assert!(!state.is_loading);
        assert!(state.snapshot.is_none() && state.last_updated.is_none());
        store.stop_observing(1);
    }

    #[test]
    fn successful_refresh_records_last_updated_and_notifies_subscribers() {
        let store = QuotaStore::new(Arc::new(|_, _| Ok(snapshot(10.0))), clock(1_800_000_000));
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        store.subscribe(move || {
            let _ = tx.lock().unwrap().send(());
        });
        store.observe(1);
        settle(&store);
        assert_eq!(store.state(Provider::Codex).last_updated, Some(UNIX_EPOCH + Duration::from_secs(1_800_000_000)));
        assert!(rx.try_recv().is_ok());
        store.stop_observing(1);
    }

    #[test]
    fn polling_refreshes_stale_providers_until_the_last_observer_leaves() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let now = Arc::new(Mutex::new(1_800_000_000u64));
        let clock_now = now.clone();
        let store = QuotaStore::with_poll_interval(
            Arc::new(move |_, _| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(snapshot(50.0))
            }),
            Arc::new(move || UNIX_EPOCH + Duration::from_secs(*clock_now.lock().unwrap())),
            Duration::from_millis(30),
        );
        store.observe(1);
        thread::sleep(Duration::from_millis(100));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        *now.lock().unwrap() += 301;
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while calls.load(Ordering::SeqCst) < 4 && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        store.stop_observing(1);
        thread::sleep(Duration::from_millis(100));
        *now.lock().unwrap() += 301;
        thread::sleep(Duration::from_millis(100));
        assert_eq!(calls.load(Ordering::SeqCst), 4);
    }
}
