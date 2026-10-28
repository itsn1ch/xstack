use crate::run::{self, RunAll, RunOptions, Spec};
use crate::state::CheckResult;
use std::cmp::Reverse;
use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::Mutex;
use std::thread;
use std::time::Instant;

const BATCH_ITEMS: usize = 32;

pub struct SplitOptions {
    pub workers: usize,
    pub each: bool,
    pub fail_fast: bool,
    pub timeout_ms: u64,
}

struct Batch {
    id: usize,
    items: Vec<usize>,
}

struct PendingWork {
    queues: Vec<VecDeque<usize>>,
    next_batch: usize,
    first_failed: Option<usize>,
}

impl PendingWork {
    fn take(&mut self, worker: usize) -> Option<Batch> {
        if self.queues[worker].is_empty() {
            let donor = (0..self.queues.len())
                .max_by_key(|&index| (self.queues[index].len(), Reverse(index)))?;
            let remaining = self.queues[donor].len();
            if remaining == 0 {
                return None;
            }
            self.queues[worker] = self.queues[donor].split_off(remaining / 2);
        }
        let count = self.queues[worker].len().min(BATCH_ITEMS);
        let items = self.queues[worker].drain(..count).collect();
        let id = self.next_batch;
        self.next_batch += 1;
        Some(Batch { id, items })
    }
}

pub fn run(cwd: &Path, items: &[String], command: &[String], options: SplitOptions) -> RunAll {
    if options.each {
        let jobs = run::item_jobs(items, command);
        return run::run_all(
            cwd,
            &jobs,
            options.workers,
            options.fail_fast,
            options.timeout_ms,
        );
    }

    let started = Instant::now();
    let worker_count = options.workers.max(1).min(items.len());
    if worker_count == 0 {
        return RunAll {
            results: Vec::new(),
            first_failed: None,
            ms: 0,
        };
    }
    let queues = run::partition_indices_by_size(items, worker_count, run::file_size)
        .into_iter()
        .map(VecDeque::from)
        .collect();
    let mut pending = PendingWork {
        queues,
        next_batch: 0,
        first_failed: None,
    };
    let initial: Vec<_> = (0..worker_count)
        .map(|worker| pending.take(worker))
        .collect();
    let pending = Mutex::new(pending);
    let cancel = AtomicBool::new(false);

    let mut completed: Vec<(usize, CheckResult)> = thread::scope(|scope| {
        let handles: Vec<_> = initial
            .into_iter()
            .enumerate()
            .map(|(worker, initial)| {
                let (pending, cancel, options) = (&pending, &cancel, &options);
                scope.spawn(move || {
                    let mut batch = initial;
                    let mut completed = Vec::new();
                    while let Some(current) = batch {
                        if cancel.load(SeqCst) {
                            break;
                        }
                        let argv = command
                            .iter()
                            .cloned()
                            .chain(current.items.iter().map(|&index| items[index].clone()))
                            .collect();
                        let spec = Spec::Argv(argv);
                        let run_options = RunOptions {
                            cancel: Some(cancel),
                            ..RunOptions::new(options.timeout_ms)
                        };
                        let result = run::run_command(cwd, &spec, &run_options);
                        let failed = result.exit != 0;
                        completed.push((current.id, result));
                        let mut work = pending.lock().expect("pending split work");
                        if failed {
                            work.first_failed.get_or_insert(current.id);
                            if options.fail_fast {
                                cancel.store(true, SeqCst);
                            }
                        }
                        batch = if cancel.load(SeqCst) {
                            None
                        } else {
                            work.take(worker)
                        };
                    }
                    completed
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("split worker"))
            .collect()
    });

    completed.sort_unstable_by_key(|(id, _)| *id);
    let failure = pending
        .into_inner()
        .expect("pending split work")
        .first_failed;
    let first_failed = completed.iter().position(|(id, _)| Some(*id) == failure);
    let results = completed
        .into_iter()
        .map(|(_, result)| Some(result))
        .collect();
    RunAll {
        results,
        first_failed,
        ms: started.elapsed().as_millis() as u64,
    }
}
