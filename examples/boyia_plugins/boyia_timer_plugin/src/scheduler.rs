//! TimerScheduler: a dedicated thread driving a min-heap of pending timers with
//! `mio::Poll::poll(timeout)` (epoll / IOCP / kqueue).
//!
//! `schedule` only enqueues a job and returns a timer id — it never blocks the caller
//! (the VM thread). The scheduler thread wakes at the earliest deadline and runs the
//! expired job. Jobs must be cheap and must not touch the VM: they only hop back to the
//! runtime thread via `BuiltinCtx::post_runtime_task` (the VM is single-threaded).
//!
//! Lifecycle: the scheduler thread starts lazily on the first `schedule` and runs for the
//! lifetime of the process.

use mio::{Events, Poll, Token, Waker};
use std::cmp::Ordering as CmpOrdering;
use std::collections::BinaryHeap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const WAKER_TOKEN: Token = Token(0);

pub type TimerId = usize;

/// Why a [`TimerJob`] is executed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerOutcome {
    /// The deadline was reached.
    Fired,
    /// `cancel` removed the timer before its deadline.
    Cancelled,
}

/// Work executed on the scheduler thread when a timer fires or is cancelled.
pub type TimerJob = Box<dyn FnOnce(TimerOutcome) + Send + 'static>;

pub struct TimerScheduler {
    inner: OnceLock<Arc<SchedulerInner>>,
}

struct SchedulerInner {
    state: Mutex<Wheel>,
    waker: Waker,
}

struct Wheel {
    heap: BinaryHeap<TimerEntry>,
    next_id: TimerId,
}

struct TimerEntry {
    id: TimerId,
    when: Instant,
    job: TimerJob,
}

// BinaryHeap is a max-heap; earliest deadline first, then lowest id for stability.
impl Ord for TimerEntry {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        other
            .when
            .cmp(&self.when)
            .then_with(|| other.id.cmp(&self.id))
    }
}
impl PartialOrd for TimerEntry {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for TimerEntry {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for TimerEntry {}

impl TimerScheduler {
    pub fn global() -> &'static Self {
        static SCHEDULER: TimerScheduler = TimerScheduler {
            inner: OnceLock::new(),
        };
        &SCHEDULER
    }

    /// Register `job` for `ms` milliseconds from now and return the timer id.
    /// Never blocks the caller and never touches the VM.
    pub fn schedule(&self, ms: u64, job: TimerJob) -> TimerId {
        let inner = self.get_or_create();
        let id = {
            let mut w = inner.state.lock().unwrap_or_else(|e| e.into_inner());
            w.next_id = w.next_id.wrapping_add(1).max(1);
            let id = w.next_id;
            w.heap.push(TimerEntry {
                id,
                when: Instant::now() + Duration::from_millis(ms),
                job,
            });
            id
        };
        let _ = inner.waker.wake();
        id
    }

    /// Cancel a pending timer. Its job runs with [`TimerOutcome::Cancelled`] so captured
    /// script callbacks can be released instead of invoked. Returns false when the id is
    /// unknown (already fired or never scheduled).
    pub fn cancel(&self, id: TimerId) -> bool {
        let Some(inner) = self.inner.get() else {
            return false;
        };
        let mut cancelled = None;
        {
            let mut w = inner.state.lock().unwrap_or_else(|e| e.into_inner());
            if !w.heap.iter().any(|e| e.id == id) {
                return false;
            }
            let mut kept = BinaryHeap::with_capacity(w.heap.len());
            while let Some(entry) = w.heap.pop() {
                if entry.id == id {
                    cancelled = Some(entry.job);
                } else {
                    kept.push(entry);
                }
            }
            w.heap = kept;
        }
        // Run the job after releasing the lock so it may safely re-enter the scheduler.
        if let Some(job) = cancelled {
            job(TimerOutcome::Cancelled);
        }
        let _ = inner.waker.wake();
        true
    }

    fn get_or_create(&self) -> Arc<SchedulerInner> {
        self.inner
            .get_or_init(|| {
                let poll = Poll::new().expect("create mio Poll");
                let waker = Waker::new(poll.registry(), WAKER_TOKEN).expect("create mio Waker");
                let inner = Arc::new(SchedulerInner {
                    state: Mutex::new(Wheel {
                        heap: BinaryHeap::new(),
                        next_id: 0,
                    }),
                    waker,
                });
                let inner_for_thread = inner.clone();
                let _ = std::thread::Builder::new()
                    .name("boyia-timer-wheel".to_owned())
                    .spawn(move || Self::run(poll, inner_for_thread))
                    .expect("spawn boyia-timer-wheel thread");
                inner
            })
            .clone()
    }

    fn run(mut poll: Poll, inner: Arc<SchedulerInner>) {
        let mut events = Events::with_capacity(16);
        loop {
            // Compute the timeout from the earliest pending deadline.
            let timeout = {
                let w = inner.state.lock().unwrap_or_else(|e| e.into_inner());
                w.heap
                    .peek()
                    .map(|e| e.when.saturating_duration_since(Instant::now()))
            };

            // Block on epoll/iocp/kqueue until the timeout or a Waker wake.
            match poll.poll(&mut events, timeout) {
                Ok(_) => {}
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    eprintln!("[timer] poll error: {e}");
                    continue;
                }
            }

            // Pop every expired timer and run its job outside the lock.
            loop {
                let entry = {
                    let mut w = inner.state.lock().unwrap_or_else(|e| e.into_inner());
                    let now = Instant::now();
                    match w.heap.peek() {
                        Some(top) if top.when <= now => w.heap.pop(),
                        _ => None,
                    }
                };
                match entry {
                    Some(e) => (e.job)(TimerOutcome::Fired),
                    None => break,
                }
            }
        }
    }
}
