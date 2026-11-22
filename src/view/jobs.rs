use crate::document::Document;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Priority {
    Urgent = 0,
    High = 1,
    Low = 2,
    None = 3,
}

type JobFn = Box<dyn FnOnce(&mut dyn Document) + Send>;

struct Job {
    cancelled: Arc<AtomicBool>,
    f: JobFn,
}

struct Shared {
    queues: Mutex<[VecDeque<Job>; 4]>,
    cv: Condvar,
    quit: AtomicBool,
    doc: Mutex<Box<dyn Document>>,
    generation: AtomicU64,
}

/// Handle to a queued job; dropping it does not cancel the job.
#[derive(Clone, Debug)]
pub struct JobHandle {
    cancelled: Arc<AtomicBool>,
}

impl JobHandle {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// Runs document operations on a background thread, mirroring EvJobScheduler.
/// The document lives behind a mutex so the UI thread can also do quick synchronous queries.
pub struct Jobs {
    shared: Arc<Shared>,
}

impl Drop for Jobs {
    fn drop(&mut self) {
        self.shared.quit.store(true, Ordering::SeqCst);
        if let Ok(mut q) = self.shared.queues.lock() {
            for level in q.iter_mut() {
                level.clear();
            }
        }
        self.shared.cv.notify_all();
    }
}

impl Jobs {
    pub fn new(doc: Box<dyn Document>) -> Jobs {
        let shared = Arc::new(Shared {
            queues: Mutex::new(Default::default()),
            cv: Condvar::new(),
            quit: AtomicBool::new(false),
            doc: Mutex::new(doc),
            generation: AtomicU64::new(0),
        });
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("evince-rs-jobs".into())
            .spawn(move || worker_loop(worker))
            .expect("spawn job thread");
        Jobs { shared }
    }

    /// Queues `f` to run on the worker; `done` runs on the main loop with its result
    /// unless the job was cancelled.
    pub fn run<T, F, C>(&self, prio: Priority, f: F, done: C) -> JobHandle
    where
        T: Send + 'static,
        F: FnOnce(&mut dyn Document) -> T + Send + 'static,
        C: FnOnce(T) + 'static,
    {
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, rx) = async_channel::bounded::<T>(1);
        let job = Job {
            cancelled: cancelled.clone(),
            f: Box::new(move |d| {
                let r = f(d);
                let _ = tx.send_blocking(r);
            }),
        };
        {
            let mut q = self.shared.queues.lock().unwrap();
            q[prio as usize].push_back(job);
        }
        self.shared.cv.notify_one();
        let c2 = cancelled.clone();
        glib::spawn_future_local(async move {
            if let Ok(v) = rx.recv().await {
                if !c2.load(Ordering::SeqCst) {
                    done(v);
                }
            }
        });
        JobHandle { cancelled }
    }

    /// Runs `f` synchronously on the calling thread with the document locked.
    pub fn with<T>(&self, f: impl FnOnce(&mut dyn Document) -> T) -> T {
        let mut d = self.shared.doc.lock().unwrap_or_else(|e| e.into_inner());
        f(d.as_mut())
    }

    pub fn bump_generation(&self) -> u64 {
        self.shared.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn generation(&self) -> u64 {
        self.shared.generation.load(Ordering::SeqCst)
    }
}

fn worker_loop(shared: Arc<Shared>) {
    loop {
        let job = {
            let mut q = shared.queues.lock().unwrap();
            loop {
                if shared.quit.load(Ordering::SeqCst) {
                    return;
                }
                let mut found = None;
                for level in q.iter_mut() {
                    while let Some(j) = level.pop_front() {
                        if !j.cancelled.load(Ordering::SeqCst) {
                            found = Some(j);
                            break;
                        }
                    }
                    if found.is_some() {
                        break;
                    }
                }
                match found {
                    Some(j) => break j,
                    None => q = shared.cv.wait(q).unwrap(),
                }
            }
        };
        let mut doc = shared.doc.lock().unwrap_or_else(|e| e.into_inner());
        let f = job.f;
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(doc.as_mut())));
        if r.is_err() {
            eprintln!("evince-rs: a document job panicked");
        }
    }
}
