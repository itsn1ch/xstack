use super::github::{resolve_checks, resolve_review_threads};
use super::types::*;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};

pub fn assess_github_merge(merge_state: &str, head: Option<&str>) -> Value {
    if merge_state == "BLOCKED" && matches!(head, Some("ERROR" | "FAILURE")) {
        json!({"kind":"refused","mergeStateStatus":merge_state,"headRollupState":head})
    } else {
        json!({"kind":"allowed","basis":if merge_state=="BLOCKED" {"rollup"} else {"merge-state"},"mergeStateStatus":merge_state,"headRollupState":head})
    }
}
pub fn read_snapshot(
    reader: &mut dyn Reader,
    c: &Context,
    include_pending_history: bool,
) -> QueryResult<Snapshot> {
    let facts = reader.pull_request(c)?;
    if facts.state == "MERGED" || facts.merged_at.is_some() {
        return Ok(Snapshot::Merged {
            context: c.clone(),
            facts,
        });
    }
    if facts.state == "CLOSED" {
        return Ok(Snapshot::Closed {
            context: c.clone(),
            facts,
        });
    }
    let threads = resolve_review_threads(reader, c)?;
    let (source, all) = resolve_checks(reader, c)?;
    let failed: Vec<_> = all
        .iter()
        .filter(|c| c.kind == CheckKind::Failed)
        .cloned()
        .collect();
    let pending: Vec<_> = all
        .iter()
        .filter(|c| c.kind == CheckKind::Pending)
        .cloned()
        .collect();
    let mut ci = Ci {
        kind: "ci-pending".into(),
        source,
        all,
        had_previous_passing_ci: false,
        failed,
        pending,
        github: None,
    };
    if !ci.failed.is_empty() || ci.pending.is_empty() || include_pending_history {
        let commits = reader.commit_rollups(c)?;
        let head = facts
            .head_ref_oid
            .as_ref()
            .and_then(|oid| commits.iter().find(|c| &c.oid == oid))
            .and_then(|c| c.state.as_deref());
        let github = assess_github_merge(&facts.merge_state_status, head);
        ci.had_previous_passing_ci = commits.iter().any(|c| {
            Some(&c.oid) != facts.head_ref_oid.as_ref() && c.state.as_deref() == Some("SUCCESS")
        });
        ci.kind = if !ci.failed.is_empty() {
            "ci-failing"
        } else if github["kind"] == "refused" {
            "ci-github-rejected"
        } else if !ci.pending.is_empty() {
            "ci-pending"
        } else {
            "ci-clean"
        }
        .into();
        if ci.kind != "ci-pending" {
            ci.github = Some(github);
        }
    }
    let review_automation_running = ci.all.iter().any(|c| {
        c.kind == CheckKind::Pending
            && [
                "bugbot",
                "security review",
                "pr review automation",
                "review automation",
            ]
            .iter()
            .any(|t| c.name.to_lowercase().contains(t))
    });
    Ok(Snapshot::Open {
        context: c.clone(),
        facts,
        threads,
        ci,
        review_automation_running,
    })
}
fn conflict_blocker(row: &Snapshot) -> Option<Value> {
    if let Snapshot::Open { context, facts, .. } = row {
        if facts.mergeable == "CONFLICTING"
            || ["DIRTY", "CONFLICTING"].contains(&facts.merge_state_status.as_str())
        {
            return Some(json!({"kind":"merge-conflicts","pr":context,"facts":facts}));
        }
    }
    None
}
fn thread_blocker(row: &Snapshot) -> Option<Value> {
    if let Snapshot::Open {
        context, threads, ..
    } = row
    {
        if !threads.is_empty() {
            return Some(json!({"kind":"review-threads","pr":context,"threads":threads}));
        }
    }
    None
}
fn ci_blocker(row: &Snapshot) -> Option<Value> {
    if let Snapshot::Open { context, ci, .. } = row {
        if ci.kind == "ci-failing" || ci.kind == "ci-github-rejected" {
            return Some(json!({"kind":"failing-checks","pr":context,"ci":ci}));
        }
    }
    None
}
fn gate_reason(row: &Snapshot, allow_draft: bool) -> Option<&'static str> {
    match row {
        Snapshot::Merged { .. } => None,
        Snapshot::Closed { .. } => Some("closed-without-merge"),
        Snapshot::Open { facts, .. } => {
            if facts.is_draft && !allow_draft {
                Some("draft-pr")
            } else if facts.review_decision.as_deref() == Some("CHANGES_REQUESTED") {
                Some("changes-requested")
            } else {
                None
            }
        }
    }
}
fn gate_blocker(row: &Snapshot, allow_draft: bool) -> Option<Value> {
    let reason = gate_reason(row, allow_draft)?;
    if reason == "draft-pr" && matches!(row,Snapshot::Open {ci,..} if ci.kind=="ci-pending") {
        return None;
    }
    Some(json!({"kind":"merge-gate","pr":row.context(),"reason":reason}))
}
fn ready_contribution(row: &Snapshot, allow_draft: bool) -> Option<Value> {
    match row {
        Snapshot::Merged { context, facts } => {
            Some(json!({"kind":"merged-pr","context":context,"mergedAt":facts.merged_at}))
        }
        Snapshot::Open {
            context,
            facts,
            threads,
            ci,
            ..
        } if ci.kind == "ci-clean"
            && threads.is_empty()
            && conflict_blocker(row).is_none()
            && gate_reason(row, allow_draft).is_none() =>
        {
            Some(
                json!({"kind":"ready-pr","context":context,"proof":{"mergeability":"clear","threads":[],"ci":ci,"gate":{"state":"OPEN","reviewDecision":facts.review_decision,"draft":if facts.is_draft {"draft-allowed"} else {"not-draft"}}}}),
            )
        }
        _ => None,
    }
}
#[derive(Debug)]
pub enum Decision {
    Blocker(Value),
    Waiting {
        frontier: Context,
        pending: Vec<Check>,
    },
    Clear(Vec<Value>),
}
pub fn select_stack_decision(rows: &[Snapshot], allow_draft: bool) -> Decision {
    for tier in [
        conflict_blocker as fn(&Snapshot) -> Option<Value>,
        thread_blocker,
        ci_blocker,
    ] {
        for row in rows {
            if let Some(blocker) = tier(row) {
                return Decision::Blocker(blocker);
            }
        }
    }
    for row in rows {
        if let Some(blocker) = gate_blocker(row, allow_draft) {
            return Decision::Blocker(blocker);
        }
    }
    for row in rows {
        if let Snapshot::Open { context, ci, .. } = row {
            if ci.kind == "ci-pending" {
                return Decision::Waiting {
                    frontier: context.clone(),
                    pending: ci.pending.clone(),
                };
            }
        }
    }
    let prs = rows
        .iter()
        .map(|row| {
            ready_contribution(row, allow_draft).expect("snapshot has no classified decision")
        })
        .collect();
    Decision::Clear(prs)
}
pub fn query_backoff_seconds(interval: f64, failures: u64) -> f64 {
    (interval.max(60.0) * 2.0_f64.powf(failures.saturating_sub(1) as f64)).min(300.0)
}
pub struct Stamp {
    sequence: u64,
    mode: Mode,
}
impl Stamp {
    pub fn new(mode: Mode) -> Self {
        Self { sequence: 0, mode }
    }
    pub fn stamp(&mut self, clock: &dyn Clock, mut payload: Value) -> Value {
        self.sequence += 1;
        let object = payload
            .as_object_mut()
            .expect("event payload must be an object");
        object.insert("schemaVersion".into(), json!(1));
        object.insert("sequence".into(), json!(self.sequence));
        object.insert("observedAt".into(), json!(clock.observed_at()));
        object.insert("mode".into(), json!(self.mode.as_str()));
        payload
    }
}
fn blocker_payload(blocker: Value) -> Value {
    let exit = match blocker["kind"].as_str().unwrap_or("") {
        "merge-conflicts" => 2,
        "review-threads" => 3,
        "failing-checks" => 4,
        "merge-gate" => 6,
        _ => unreachable!("invalid merge blocker"),
    };
    json!({"kind":"BLOCKER","terminal":true,"exitCode":exit,"blocker":blocker})
}
pub fn status_query_payload(failures: u64, failure: Failure) -> Value {
    json!({"kind":"BLOCKER","terminal":true,"exitCode":7,"blocker":{"kind":"status-query","failures":failures,"failure":failure}})
}
fn deadline_passed(start: f64, options: &PollingOptions, now: f64) -> bool {
    options.timeout > 0.0 && now - start >= options.timeout
}
fn sleep(clock: &mut dyn Clock, options: &PollingOptions, started: f64, seconds: f64) {
    let remaining = if options.timeout > 0.0 {
        (options.timeout - (clock.now() - started)).max(0.0)
    } else {
        seconds
    };
    clock.sleep(seconds.min(remaining));
}
fn retry(
    failure: Failure,
    failures: &mut u64,
    options: &PollingOptions,
    started: f64,
    stamp: &mut Stamp,
    clock: &mut dyn Clock,
    emit: &mut dyn FnMut(Value),
) -> Option<Value> {
    *failures += 1;
    if !failure.retryable || *failures >= options.max_query_errors {
        return Some(stamp.stamp(clock, status_query_payload(*failures, failure)));
    }
    let delay = query_backoff_seconds(options.interval, *failures);
    emit(stamp.stamp(clock,json!({"kind":"RETRY","terminal":false,"failure":failure,"consecutiveFailures":*failures,"retryInSeconds":delay})));
    if deadline_passed(started, options, clock.now()) {
        return Some(stamp.stamp(clock,json!({"kind":"TIMEOUT","terminal":true,"exitCode":5,"reason":{"kind":"status-unavailable","failure":failure}})));
    }
    sleep(clock, options, started, delay);
    None
}
pub fn run_simple(
    reader: &mut dyn Reader,
    clock: &mut dyn Clock,
    emit: &mut dyn FnMut(Value),
    contexts: &[Context],
    mode: Mode,
    status_only: bool,
    options: &PollingOptions,
) -> Value {
    let mut stamp = Stamp::new(mode);
    let started = clock.now();
    let mut failures = 0;
    loop {
        let rows = contexts
            .iter()
            .map(|c| read_snapshot(reader, c, true))
            .collect::<QueryResult<Vec<_>>>();
        let rows = match rows {
            Ok(rows) => {
                failures = 0;
                rows
            }
            Err(failure) => {
                if let Some(v) = retry(
                    failure,
                    &mut failures,
                    options,
                    started,
                    &mut stamp,
                    clock,
                    emit,
                ) {
                    return v;
                }
                continue;
            }
        };
        assert!(!rows.is_empty(), "watch context cannot be empty");
        if status_only {
            return stamp.stamp(clock,json!({"kind":"STATUS","terminal":true,"exitCode":0,"reason":"status-only","rows":rows}));
        }
        assert_ne!(
            mode,
            Mode::QueuedStack,
            "queued-stack requires status-only in simple runner"
        );
        if mode == Mode::Stack {
            emit(stamp.stamp(
                clock,
                json!({"kind":"STATUS","terminal":false,"reason":"poll","rows":rows}),
            ));
        }
        let selected = if mode == Mode::Single {
            &rows[..1]
        } else {
            &rows[..]
        };
        match select_stack_decision(selected,options.allow_draft) {
            Decision::Blocker(blocker)=>return stamp.stamp(clock,blocker_payload(blocker)),
            Decision::Clear(mut prs)=>return stamp.stamp(clock,json!({"kind":"READY","terminal":true,"exitCode":0,"scope":if mode==Mode::Single {json!({"kind":"single","pr":prs.remove(0)})} else {json!({"kind":"stack","prs":prs})}})),
            Decision::Waiting {frontier,pending}=>{
                emit(stamp.stamp(clock,json!({"kind":"WAITING","terminal":false,"frontier":frontier,"reason":{"kind":"pending-checks","pending":pending}})));
                if deadline_passed(started,options,clock.now()) {return stamp.stamp(clock,json!({"kind":"TIMEOUT","terminal":true,"exitCode":5,"reason":{"kind":"pending-checks","pending":pending}}));}
                sleep(clock,options,started,options.interval);
            }
        }
    }
}
#[derive(Clone, Debug)]
pub enum QueueWork {
    Sweep(VecDeque<Context>),
    Frontier(Context),
}
#[derive(Clone, Debug)]
pub struct QueueState {
    pub queue: Vec<Context>,
    pub snapshots: HashMap<u64, Snapshot>,
    pub work: Option<QueueWork>,
    pub next_sweep_at: f64,
    pub frontier: Option<Context>,
    pub last_wait_key: Option<String>,
    pub started_at: f64,
}
impl QueueState {
    pub fn new(queue: &[Context], now: f64) -> Self {
        Self {
            queue: queue.into(),
            snapshots: HashMap::new(),
            work: Some(QueueWork::Sweep(queue.iter().cloned().collect())),
            next_sweep_at: now,
            frontier: None,
            last_wait_key: None,
            started_at: now,
        }
    }
    fn ordered_rows(&self) -> Vec<&Snapshot> {
        self.queue
            .iter()
            .filter_map(|c| self.snapshots.get(&c.number))
            .collect()
    }
    pub fn plan(&mut self, now: f64) {
        if self.work.is_some() {
            return;
        }
        if self.snapshots.is_empty() || now >= self.next_sweep_at {
            let remaining: VecDeque<_> = self
                .queue
                .iter()
                .filter(|c| !self.snapshots.get(&c.number).is_some_and(Snapshot::merged))
                .cloned()
                .collect();
            if !remaining.is_empty() {
                self.work = Some(QueueWork::Sweep(remaining));
                return;
            }
        }
        if let Some(row) = self.ordered_rows().into_iter().find(|r| !r.merged()) {
            self.work = Some(QueueWork::Frontier(row.context().clone()));
        }
    }
    pub fn apply(
        &mut self,
        snapshot: Snapshot,
        now: f64,
        options: &PollingOptions,
    ) -> Option<Vec<Snapshot>> {
        let number = snapshot.context().number;
        let work = self.work.as_mut().expect("queue has no read in flight");
        self.snapshots.insert(number, snapshot);
        match work {
            QueueWork::Frontier(_) => {
                self.work = None;
                None
            }
            QueueWork::Sweep(remaining) => {
                assert_eq!(
                    remaining.front().map(|c| c.number),
                    Some(number),
                    "snapshot does not match sweep head"
                );
                remaining.pop_front();
                if !remaining.is_empty() {
                    return None;
                }
                self.work = None;
                self.next_sweep_at = now + options.sweep_interval;
                let rows: Vec<_> = self.ordered_rows().into_iter().cloned().collect();
                assert_eq!(
                    rows.len(),
                    self.queue.len(),
                    "sweep completed without every snapshot"
                );
                Some(rows)
            }
        }
    }
    pub fn evaluate(&mut self, now: f64, options: &PollingOptions) -> QueueEvaluation {
        let active: Vec<_> = self
            .ordered_rows()
            .into_iter()
            .filter(|r| !r.merged())
            .cloned()
            .collect();
        if active.is_empty() {
            let merged:Vec<_>=self.ordered_rows().into_iter().filter(|r|r.merged()).map(|r|json!({"kind":"merged-pr","context":r.context(),"mergedAt":r.facts().merged_at})).collect();
            assert!(!merged.is_empty(), "empty queue cannot complete");
            return QueueEvaluation::Complete(merged);
        }
        if let Decision::Blocker(blocker) = select_stack_decision(&active, options.allow_draft) {
            return QueueEvaluation::Blocker(blocker);
        }
        let frontier = active[0].context().clone();
        if let Some(old) = &self.frontier {
            if old.number != frontier.number {
                let merged = old.clone();
                self.frontier = Some(frontier.clone());
                self.last_wait_key = None;
                return QueueEvaluation::Advance {
                    merged,
                    frontier,
                    remaining: active.len(),
                };
            }
        }
        if deadline_passed(self.started_at, options, now) {
            self.frontier = Some(frontier.clone());
            return QueueEvaluation::Timeout {
                frontier,
                unmerged_count: active.len(),
            };
        }
        let pending = match &active[0] {
            Snapshot::Open { ci, .. } if ci.kind == "ci-pending" => Some(&ci.pending),
            _ => None,
        };
        let (reason, key) = if let Some(pending) = pending {
            (
                json!({"kind":"pending-checks","pending":pending}),
                format!("pending:{}:{}", frontier.number, pending.len()),
            )
        } else {
            (
                json!({"kind":"merge-queue","unmergedCount":active.len()}),
                format!("queue:{}:{}", frontier.number, active.len()),
            )
        };
        let emit = self.last_wait_key.as_ref() != Some(&key);
        self.frontier = Some(frontier.clone());
        self.last_wait_key = Some(key);
        QueueEvaluation::Waiting {
            frontier,
            reason,
            emit,
        }
    }
}
#[derive(Debug)]
pub enum QueueEvaluation {
    Complete(Vec<Value>),
    Blocker(Value),
    Advance {
        merged: Context,
        frontier: Context,
        remaining: usize,
    },
    Timeout {
        frontier: Context,
        unmerged_count: usize,
    },
    Waiting {
        frontier: Context,
        reason: Value,
        emit: bool,
    },
}
pub fn run_queued(
    reader: &mut dyn Reader,
    clock: &mut dyn Clock,
    emit: &mut dyn FnMut(Value),
    contexts: &[Context],
    options: &PollingOptions,
) -> Value {
    let mut state = QueueState::new(contexts, clock.now());
    let mut stamp = Stamp::new(Mode::QueuedStack);
    let started = clock.now();
    let mut failures = 0;
    emit(stamp.stamp(
        clock,
        json!({"kind":"QUEUE","terminal":false,"queue":contexts}),
    ));
    loop {
        state.plan(clock.now());
        let context = match state.work.as_ref() {
            Some(QueueWork::Sweep(remaining)) => remaining.front().expect("empty sweep").clone(),
            Some(QueueWork::Frontier(frontier)) => frontier.clone(),
            None => {
                let QueueEvaluation::Complete(merged) = state.evaluate(clock.now(), options) else {
                    panic!("queue has no work while active")
                };
                return stamp.stamp(clock,json!({"kind":"COMPLETE","terminal":true,"exitCode":0,"queue":state.queue,"merged":merged}));
            }
        };
        let snapshot = match read_snapshot(reader, &context, false) {
            Ok(row) => {
                failures = 0;
                row
            }
            Err(failure) => {
                if let Some(v) = retry(
                    failure,
                    &mut failures,
                    options,
                    started,
                    &mut stamp,
                    clock,
                    emit,
                ) {
                    return v;
                }
                continue;
            }
        };
        if let Some(rows) = state.apply(snapshot, clock.now(), options) {
            emit(stamp.stamp(
                clock,
                json!({"kind":"STATUS","terminal":false,"reason":"whole-stack-sweep","rows":rows}),
            ));
        }
        if state.work.is_some() {
            continue;
        }
        match state.evaluate(clock.now(),options) {
            QueueEvaluation::Complete(merged)=>return stamp.stamp(clock,json!({"kind":"COMPLETE","terminal":true,"exitCode":0,"queue":state.queue,"merged":merged})),
            QueueEvaluation::Blocker(blocker)=>return stamp.stamp(clock,blocker_payload(blocker)),
            QueueEvaluation::Advance {merged,frontier,remaining}=>{emit(stamp.stamp(clock,json!({"kind":"ADVANCE","terminal":false,"merged":merged,"frontier":frontier,"remaining":remaining})));},
            QueueEvaluation::Timeout {frontier,unmerged_count}=>return stamp.stamp(clock,json!({"kind":"TIMEOUT","terminal":true,"exitCode":5,"reason":{"kind":"queued-stack","frontier":frontier,"unmergedCount":unmerged_count}})),
            QueueEvaluation::Waiting {frontier,reason,emit:should_emit}=>{
                if should_emit {emit(stamp.stamp(clock,json!({"kind":"WAITING","terminal":false,"frontier":frontier,"reason":reason})));}
                sleep(clock,options,started,options.interval);
            }
        }
    }
}
