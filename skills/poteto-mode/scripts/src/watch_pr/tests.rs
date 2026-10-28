use super::{cli, github::*, policy::*, render::*, types::*};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

fn context(number: u64) -> Context {
    Context {
        owner: "owner".into(),
        repo: "repo".into(),
        number,
    }
}
fn raw_facts() -> Value {
    json!({"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","reviewDecision":"APPROVED","headRefOid":"head","headRefName":"feature","baseRefName":"main","state":"OPEN","mergedAt":null,"isDraft":false})
}
fn check(kind: CheckKind, name: &str) -> Check {
    Check {
        name: name.into(),
        reported_state: match kind {
            CheckKind::Pending => "PENDING",
            CheckKind::Failed => "FAILURE",
            _ => "SUCCESS",
        }
        .into(),
        description: String::new(),
        link: String::new(),
        workflow: String::new(),
        kind,
    }
}
fn options() -> PollingOptions {
    PollingOptions {
        interval: 10.0,
        sweep_interval: 300.0,
        timeout: 0.0,
        max_query_errors: 5,
        allow_draft: false,
    }
}
fn error() -> Failure {
    let mut e = Failure::new("command-exit", "rate limited");
    e.code = Some(1);
    e
}
#[derive(Default)]
struct FakeClock {
    now: f64,
    sleeps: Vec<f64>,
    timeline: Option<Rc<RefCell<Vec<String>>>>,
}
impl Clock for FakeClock {
    fn now(&self) -> f64 {
        self.now
    }
    fn observed_at(&self) -> String {
        "2026-07-26T00:00:00.000Z".into()
    }
    fn sleep(&mut self, seconds: f64) {
        if let Some(t) = &self.timeline {
            t.borrow_mut().push("sleep".into());
        }
        self.sleeps.push(seconds);
        self.now += seconds;
    }
}
struct FakeReader {
    facts: Value,
    fast: FastChecks,
    rollup_pages: VecDeque<RollupPage>,
    thread_pages: VecDeque<ThreadPage>,
    rollups: Vec<CommitRollup>,
    open: Vec<OpenPr>,
    origin: Option<Repository>,
    calls: Vec<String>,
    pull_results: HashMap<u64, VecDeque<QueryResult<Value>>>,
    fast_results: VecDeque<FastChecks>,
    always_error: bool,
    timeline: Option<Rc<RefCell<Vec<String>>>>,
}
impl Default for FakeReader {
    fn default() -> Self {
        Self {
            facts: raw_facts(),
            fast: FastChecks::Checks(vec![check(CheckKind::Passed, "ci")]),
            rollup_pages: VecDeque::new(),
            thread_pages: VecDeque::new(),
            rollups: vec![CommitRollup {
                oid: "head".into(),
                state: Some("SUCCESS".into()),
            }],
            open: vec![],
            origin: Some(Repository {
                owner: "owner".into(),
                repo: "repo".into(),
            }),
            calls: vec![],
            pull_results: HashMap::new(),
            fast_results: VecDeque::new(),
            always_error: false,
            timeline: None,
        }
    }
}
impl FakeReader {
    fn record(&mut self, s: String) {
        self.calls.push(s);
    }
}
impl Reader for FakeReader {
    fn origin_repo(&mut self) -> QueryResult<Option<Repository>> {
        self.record("originRepo".into());
        Ok(self.origin.clone())
    }
    fn current_pr(&mut self, pr: Option<u64>) -> QueryResult<Context> {
        self.record("currentPr".into());
        Ok(context(pr.unwrap_or(1)))
    }
    fn pull_request(&mut self, c: &Context) -> QueryResult<Facts> {
        self.record(format!("pullRequest:{}", c.number));
        let result = if self.always_error {
            Err(error())
        } else {
            self.pull_results
                .get_mut(&c.number)
                .and_then(VecDeque::pop_front)
                .unwrap_or_else(|| Ok(self.facts.clone()))
        };
        if let Some(t) = &self.timeline {
            t.borrow_mut().push(format!(
                "{}:{}",
                if result.is_ok() { "read" } else { "fail" },
                c.number
            ));
        }
        parse_pull_request(&result?, c)
    }
    fn open_pull_requests(&mut self, _: &Repository) -> QueryResult<Vec<OpenPr>> {
        self.record("openPullRequests".into());
        Ok(self.open.clone())
    }
    fn checks_fast_path(&mut self, _: &Context) -> QueryResult<FastChecks> {
        self.record("checksFastPath".into());
        Ok(self
            .fast_results
            .pop_front()
            .unwrap_or_else(|| self.fast.clone()))
    }
    fn check_rollup_page(&mut self, _: &Context, after: Option<&str>) -> QueryResult<RollupPage> {
        self.record(format!("checkRollupPage:{}", after.unwrap_or("null")));
        Ok(self.rollup_pages.pop_front().unwrap_or(RollupPage {
            checks: vec![],
            end_cursor: None,
        }))
    }
    fn review_thread_page(&mut self, _: &Context, after: Option<&str>) -> QueryResult<ThreadPage> {
        self.record(format!("reviewThreadPage:{}", after.unwrap_or("null")));
        Ok(self.thread_pages.pop_front().unwrap_or(ThreadPage {
            threads: vec![],
            end_cursor: None,
        }))
    }
    fn commit_rollups(&mut self, _: &Context) -> QueryResult<Vec<CommitRollup>> {
        self.record("commitRollups".into());
        Ok(self.rollups.clone())
    }
}
fn snapshot(number: u64, patch: Value, kind: CheckKind) -> Snapshot {
    let mut reader = FakeReader::default();
    for (k, v) in patch.as_object().unwrap() {
        reader.facts[k] = v.clone();
    }
    reader.fast = FastChecks::Checks(vec![check(kind, "ci")]);
    read_snapshot(&mut reader, &context(number), false).unwrap()
}
#[test]
fn readiness_truth_table() {
    for (merge, head, expected) in [
        ("BLOCKED", Some("FAILURE"), "refused"),
        ("BLOCKED", Some("ERROR"), "refused"),
        ("BLOCKED", Some("PENDING"), "allowed"),
        ("UNSTABLE", Some("FAILURE"), "allowed"),
        ("UNKNOWN", Some("ERROR"), "allowed"),
        ("UNKNOWN", Some("EXPECTED"), "allowed"),
        ("UNKNOWN", Some("FAILURE"), "allowed"),
        ("UNKNOWN", Some("PENDING"), "allowed"),
        ("UNKNOWN", Some("SUCCESS"), "allowed"),
        ("UNKNOWN", None, "allowed"),
        ("CLEAN", Some("SUCCESS"), "allowed"),
    ] {
        assert_eq!(assess_github_merge(merge, head)["kind"], expected);
    }
}
#[test]
fn hidden_github_ci_refusal_is_explicit_blocker() {
    let mut reader = FakeReader::default();
    reader.facts["mergeStateStatus"] = json!("BLOCKED");
    reader.rollups[0].state = Some("FAILURE".into());
    let row = read_snapshot(&mut reader, &context(1), true).unwrap();
    assert_eq!(
        serde_json::to_value(&row).unwrap()["ci"]["kind"],
        "ci-github-rejected"
    );
    let Decision::Blocker(b) = select_stack_decision(&[row], false) else {
        panic!("expected blocker")
    };
    assert_eq!(b["kind"], "failing-checks");
}
#[test]
fn snapshot_query_planning_preserves_pending_history_and_terminal_shortcuts() {
    let mut reader = FakeReader::default();
    reader.fast = FastChecks::Checks(vec![check(CheckKind::Pending, "ci")]);
    let row = read_snapshot(&mut reader, &context(2), false).unwrap();
    assert_eq!(
        serde_json::to_value(row).unwrap()["ci"]["kind"],
        "ci-pending"
    );
    assert_eq!(
        reader.calls,
        ["pullRequest:2", "reviewThreadPage:null", "checksFastPath"]
    );
    reader.calls.clear();
    read_snapshot(&mut reader, &context(2), true).unwrap();
    assert!(reader.calls.contains(&"commitRollups".into()));
    for kind in [CheckKind::Passed, CheckKind::Failed] {
        reader.calls.clear();
        reader.fast = FastChecks::Checks(vec![check(kind, "ci")]);
        read_snapshot(&mut reader, &context(3), false).unwrap();
        assert!(reader.calls.contains(&"commitRollups".into()));
    }
    for state in ["MERGED", "CLOSED"] {
        reader.calls.clear();
        reader.facts["state"] = json!(state);
        let row = read_snapshot(&mut reader, &context(5), true).unwrap();
        assert_eq!(
            serde_json::to_value(row).unwrap()["kind"],
            state.to_lowercase()
        );
        assert_eq!(reader.calls, ["pullRequest:5"]);
    }
}
#[test]
fn stack_is_tier_major_and_attributes_waits_to_actual_pending_pr() {
    let bottom = snapshot(10, json!({}), CheckKind::Failed);
    let top = snapshot(11, json!({"mergeable":"CONFLICTING"}), CheckKind::Passed);
    let Decision::Blocker(b) = select_stack_decision(&[bottom, top], false) else {
        panic!("expected blocker")
    };
    assert_eq!(b["kind"], "merge-conflicts");
    assert_eq!(b["pr"]["number"], 11);
    let bottom = snapshot(20, json!({}), CheckKind::Passed);
    let top = snapshot(21, json!({}), CheckKind::Pending);
    let Decision::Waiting { frontier, pending } = select_stack_decision(&[bottom, top], false)
    else {
        panic!("expected wait")
    };
    assert_eq!(frontier.number, 21);
    assert_eq!(pending.len(), 1);
}
#[test]
fn all_blocker_tiers_and_merge_gates() {
    for (patch, kind, expected) in [
        (
            json!({"mergeStateStatus":"DIRTY"}),
            CheckKind::Passed,
            "merge-conflicts",
        ),
        (json!({}), CheckKind::Failed, "failing-checks"),
        (json!({"state":"CLOSED"}), CheckKind::Passed, "merge-gate"),
        (
            json!({"reviewDecision":"CHANGES_REQUESTED"}),
            CheckKind::Passed,
            "merge-gate",
        ),
    ] {
        let Decision::Blocker(b) = select_stack_decision(&[snapshot(1, patch, kind)], false) else {
            panic!("expected blocker")
        };
        assert_eq!(b["kind"], expected);
    }
    let pending = snapshot(12, json!({"isDraft":true}), CheckKind::Pending);
    assert!(matches!(
        select_stack_decision(&[pending], false),
        Decision::Waiting { .. }
    ));
    let settled = snapshot(12, json!({"isDraft":true}), CheckKind::Passed);
    let Decision::Blocker(b) = select_stack_decision(&[settled.clone()], false) else {
        panic!("expected draft gate")
    };
    assert_eq!(b["reason"], "draft-pr");
    let Decision::Clear(prs) = select_stack_decision(&[settled], true) else {
        panic!("expected allowed draft")
    };
    assert_eq!(prs[0]["proof"]["gate"]["draft"], "draft-allowed");
}
#[test]
fn checks_fallback_chain_and_pagination() {
    let mut reader = FakeReader::default();
    let (source, checks) = resolve_checks(&mut reader, &context(42)).unwrap();
    assert_eq!(source, "gh-pr-checks");
    assert_eq!(checks[0].name, "ci");
    assert_eq!(reader.calls, ["checksFastPath"]);
    for fast in [
        FastChecks::Unusable {
            exit_code: 8,
            stderr: String::new(),
        },
        FastChecks::Checks(vec![]),
    ] {
        let mut reader = FakeReader {
            fast,
            ..FakeReader::default()
        };
        reader.rollup_pages = VecDeque::from([
            RollupPage {
                checks: vec![check(CheckKind::Passed, "first")],
                end_cursor: Some("next".into()),
            },
            RollupPage {
                checks: vec![check(CheckKind::Failed, "second")],
                end_cursor: None,
            },
        ]);
        let (source, checks) = resolve_checks(&mut reader, &context(42)).unwrap();
        assert_eq!(source, "graphql-rollup");
        assert_eq!(
            checks.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            ["first", "second"]
        );
        assert_eq!(
            reader.calls,
            [
                "checksFastPath",
                "checkRollupPage:null",
                "checkRollupPage:next"
            ]
        );
    }
    let mut reader = FakeReader {
        fast: FastChecks::Unusable {
            exit_code: 8,
            stderr: "credential cannot read checks".into(),
        },
        ..FakeReader::default()
    };
    let e = resolve_checks(&mut reader, &context(42)).unwrap_err();
    assert_eq!(e.kind, "checks-unavailable");
    assert!(e.detail.contains("credential cannot read checks"));
}
#[test]
fn rollup_and_fast_path_classification_fail_closed() {
    for (status, conclusion, kind, state) in [
        ("IN_PROGRESS", None, CheckKind::Pending, "PENDING"),
        ("COMPLETED", Some("SUCCESS"), CheckKind::Passed, "SUCCESS"),
        ("COMPLETED", Some("NEUTRAL"), CheckKind::Skipped, "NEUTRAL"),
        ("COMPLETED", Some("SKIPPED"), CheckKind::Skipped, "SKIPPED"),
        (
            "COMPLETED",
            Some("ACTION_REQUIRED"),
            CheckKind::Failed,
            "ACTION_REQUIRED",
        ),
        ("COMPLETED", Some("TIMED_OUT"), CheckKind::Failed, "FAILURE"),
        (
            "COMPLETED",
            Some("FUTURE_VALUE"),
            CheckKind::Failed,
            "FAILURE",
        ),
    ] {
        let c = map_rollup_node(
            &json!({"__typename":"CheckRun","name":"ci","status":status,"conclusion":conclusion}),
        )
        .unwrap()
        .unwrap();
        assert_eq!(c.kind, kind);
        assert_eq!(c.reported_state, state);
    }
    for (state, kind, reported) in [
        ("EXPECTED", CheckKind::Pending, "PENDING"),
        ("PENDING", CheckKind::Pending, "PENDING"),
        ("SUCCESS", CheckKind::Passed, "SUCCESS"),
        ("FUTURE_VALUE", CheckKind::Failed, "FUTURE_VALUE"),
    ] {
        let c =
            map_rollup_node(&json!({"__typename":"StatusContext","context":"ci","state":state}))
                .unwrap()
                .unwrap();
        assert_eq!(c.kind, kind);
        assert_eq!(c.reported_state, reported);
    }
    assert!(map_rollup_node(&json!({"__typename":"FutureNode"}))
        .unwrap()
        .is_none());
    for (bucket, state, kind) in [
        ("pass", "SUCCESS", CheckKind::Passed),
        ("skipping", "SKIPPED", CheckKind::Skipped),
        ("pending", "PENDING", CheckKind::Pending),
        ("unknown", "FUTURE", CheckKind::Failed),
        ("pass", "ERROR", CheckKind::Failed),
        ("fail", "ACTION_REQUIRED", CheckKind::Failed),
    ] {
        assert_eq!(
            parse_fast_check(&json!({"name":"ci","bucket":bucket,"state":state}))
                .unwrap()
                .kind,
            kind
        );
    }
}
#[test]
fn owner_approval_gate_is_neither_pending_nor_failed_but_real_failure_is_failed() {
    for v in [
        json!({"__typename":"CheckRun","name":"Code Review Gate","status":"IN_PROGRESS","conclusion":null}),
        json!({"__typename":"StatusContext","context":"Code Review Gate","state":"PENDING"}),
        json!({"__typename":"CheckRun","name":"Code Review Gate","status":"COMPLETED","conclusion":"ACTION_REQUIRED"}),
    ] {
        assert_eq!(
            map_rollup_node(&v).unwrap().unwrap().kind,
            CheckKind::CodeReviewGate
        );
    }
    for (bucket, state, kind) in [
        ("pending", "PENDING", CheckKind::CodeReviewGate),
        ("fail", "ACTION_REQUIRED", CheckKind::CodeReviewGate),
        ("fail", "FAILURE", CheckKind::Failed),
    ] {
        assert_eq!(
            parse_fast_check(&json!({"name":"Code Review Gate","state":state,"bucket":bucket}))
                .unwrap()
                .kind,
            kind
        );
    }
}
#[test]
fn closed_enum_parsing_and_nullable_review_decision() {
    let mut v = raw_facts();
    v["mergeStateStatus"] = json!("CONFLICTING");
    assert_eq!(
        parse_pull_request(&v, &context(42))
            .unwrap()
            .merge_state_status,
        "CONFLICTING"
    );
    v["reviewDecision"] = json!("");
    assert!(parse_pull_request(&v, &context(42))
        .unwrap()
        .review_decision
        .is_none());
    v["reviewDecision"] = json!("MAYBE");
    assert_eq!(
        parse_pull_request(&v, &context(42)).unwrap_err().kind,
        "missing-key"
    );
    v["reviewDecision"] = Value::Null;
    v["mergeStateStatus"] = json!("FUTURE_STATE");
    let e = parse_pull_request(&v, &context(42)).unwrap_err();
    assert!(e.retryable);
    assert_eq!(e.raw_value.as_deref(), Some("\"FUTURE_STATE\""));
    v.as_object_mut().unwrap().remove("mergeStateStatus");
    let e = parse_pull_request(&v, &context(42)).unwrap_err();
    assert!(e.raw_value.is_none());
}
fn raw_thread(id: &str, author: &str, body: &str, resolved: bool) -> RawThread {
    RawThread {
        id: id.into(),
        first_comment: Some(Comment {
            author_login: Some(author.into()),
            body: body.into(),
            path: Some("a.ts".into()),
            line: Some(1),
            created_at: "now".into(),
        }),
        resolved,
    }
}
#[test]
fn review_pagination_counts_all_passes_and_filters_resolved_threads() {
    let mut reader = FakeReader::default();
    reader.thread_pages = VecDeque::from([
        ThreadPage {
            threads: vec![raw_thread("one", "bugbot", "RUN_ID: run-1", false)],
            end_cursor: Some("next".into()),
        },
        ThreadPage {
            threads: vec![raw_thread(
                "two",
                "cursor",
                "CURSOR_AUTOMATION_ID: run-2 severity high",
                false,
            )],
            end_cursor: Some("last".into()),
        },
        ThreadPage {
            threads: vec![raw_thread("resolved", "bugbot", "RUN_ID: run-3", true)],
            end_cursor: None,
        },
    ]);
    let threads = resolve_review_threads(&mut reader, &context(42)).unwrap();
    assert_eq!(
        threads.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        ["one", "two"]
    );
    assert!(threads
        .iter()
        .all(|t| t.is_bugbot && t.bugbot_review_passes == 3));
    assert_eq!(
        reader.calls,
        [
            "reviewThreadPage:null",
            "reviewThreadPage:next",
            "reviewThreadPage:last"
        ]
    );
    let threads = build_review_threads(vec![
        raw_thread("keyless", "bugbot", "issue", false),
        raw_thread("human", "someone", "issue", false),
    ]);
    assert_eq!(threads[0].bugbot_review_passes, 1);
    assert!(!threads[1].is_bugbot);
    let mut reader = FakeReader::default();
    reader.thread_pages = VecDeque::from([
        ThreadPage {
            threads: vec![raw_thread("early", "someone", "fix", true)],
            end_cursor: Some("next".into()),
        },
        ThreadPage {
            threads: vec![raw_thread("late", "someone", "fix", false)],
            end_cursor: None,
        },
    ]);
    assert_eq!(
        resolve_review_threads(&mut reader, &context(42)).unwrap()[0].id,
        "late"
    );
}
#[test]
fn review_page_requires_page_info_and_parses_nullable_first_comment() {
    let value = json!({"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"one","isResolved":false,"comments":{"nodes":[{"author":null,"body":"fix","path":null,"line":null,"createdAt":"now"}]}},{"id":"empty","isResolved":true,"comments":{"nodes":[]}}],"pageInfo":{"hasNextPage":true,"endCursor":"next"}}}}}});
    let page = parse_review_thread_page(&value).unwrap();
    assert_eq!(page.end_cursor.as_deref(), Some("next"));
    assert!(page.threads[0]
        .first_comment
        .as_ref()
        .unwrap()
        .author_login
        .is_none());
    assert!(page.threads[1].first_comment.is_none());
    let mut value = value;
    value["data"]["repository"]["pullRequest"]["reviewThreads"]
        .as_object_mut()
        .unwrap()
        .remove("pageInfo");
    assert_eq!(
        parse_review_thread_page(&value).unwrap_err().kind,
        "missing-key"
    );
}
fn open(number: u64, head: &str, base: &str, fork: bool) -> OpenPr {
    OpenPr {
        number,
        head_ref_name: head.into(),
        base_ref_name: base.into(),
        is_cross_repository: fork,
    }
}
#[test]
fn context_resolution_prefers_explicit_then_origin_then_current() {
    let mut reader = FakeReader::default();
    assert_eq!(
        resolve_context(&mut reader, Some("explicit"), Some("repo"), Some(42))
            .unwrap()
            .owner,
        "explicit"
    );
    assert!(reader.calls.is_empty());
    reader.origin = Some(Repository {
        owner: "local".into(),
        repo: "checkout".into(),
    });
    let c = resolve_context(&mut reader, None, None, Some(42)).unwrap();
    assert_eq!(c.owner, "local");
    assert_eq!(c.repo, "checkout");
    assert_eq!(reader.calls, ["originRepo"]);
    reader.calls.clear();
    reader.origin = None;
    let c = resolve_context(&mut reader, Some("override"), None, Some(42)).unwrap();
    assert_eq!(c.owner, "override");
    assert_eq!(reader.calls, ["originRepo", "currentPr"]);
}
#[test]
fn stack_order_and_fork_identity() {
    let numbers = |prs: Vec<OpenPr>| {
        order_stack(&context(42), &prs)
            .iter()
            .map(|p| p.number)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        numbers(vec![
            open(41, "base-feature", "main", false),
            open(42, "feature", "base-feature", false),
            open(44, "second-child", "feature", false),
            open(43, "upstack", "feature", false)
        ]),
        [41, 42, 43, 44]
    );
    assert_eq!(
        numbers(vec![
            open(41, "base-feature", "main", true),
            open(42, "feature", "base-feature", false)
        ]),
        [42]
    );
    assert_eq!(
        numbers(vec![
            open(42, "feature", "main", true),
            open(43, "upstack", "feature", false)
        ]),
        [42]
    );
    assert_eq!(
        numbers(vec![
            open(41, "main", "main", true),
            open(42, "feature", "main", false)
        ]),
        [42]
    );
    assert_eq!(numbers(vec![]), [42]);
}
#[test]
fn canonical_remote_and_pr_urls_reject_wrong_origin_or_credentials() {
    for remote in [
        "https://github.com/owner/repo.git",
        "git@github.com:owner/repo.git",
        "ssh://git@github.com/owner/repo.git",
    ] {
        assert_eq!(
            parse_remote(remote).unwrap(),
            Repository {
                owner: "owner".into(),
                repo: "repo".into()
            }
        );
    }
    for remote in [
        "https://evil.test/owner/repo",
        "http://github.com/owner/repo",
        "https://u@github.com/owner/repo",
        "https://github.com/owner/repo?x=1",
        "https://github.com/owner/repo#x",
        "https://github.com/owner/repo/extra",
    ] {
        assert!(parse_remote(remote).is_none(), "{remote}");
    }
    assert_eq!(
        parse_pr_url("https://github.com/owner/repo/pull/42").unwrap(),
        context(42)
    );
    for url in [
        "https://github.com/owner/repo/issues/42",
        "https://evil.test/owner/repo/pull/42",
        "https://github.com/owner/repo/pull/0",
    ] {
        let e = parse_pr_url(url).unwrap_err();
        assert!(!e.retryable);
        assert_eq!(e.raw_value.as_deref(), Some(url));
    }
}
#[test]
fn queue_sweep_retains_failed_head_and_only_emits_completed_sweep() {
    let queue = [context(20), context(21), context(22)];
    let mut state = QueueState::new(&queue, 0.0);
    assert!(state
        .apply(snapshot(20, json!({}), CheckKind::Passed), 0.0, &options())
        .is_none());
    let Some(QueueWork::Sweep(remaining)) = &state.work else {
        panic!("expected sweep")
    };
    assert_eq!(
        remaining.iter().map(|c| c.number).collect::<Vec<_>>(),
        [21, 22]
    );
    state.plan(60.0);
    let Some(QueueWork::Sweep(remaining)) = &state.work else {
        panic!("expected sweep")
    };
    assert_eq!(remaining[0].number, 21);
    assert!(state
        .apply(snapshot(21, json!({}), CheckKind::Passed), 60.0, &options())
        .is_none());
    let rows = state
        .apply(snapshot(22, json!({}), CheckKind::Passed), 65.0, &options())
        .unwrap();
    assert_eq!(
        rows.iter().map(|r| r.context().number).collect::<Vec<_>>(),
        [20, 21, 22]
    );
    assert_eq!(state.next_sweep_at, 365.0);
}
#[test]
fn queue_deduplicates_waits_and_sweeps_on_schedule() {
    let mut state = QueueState::new(&[context(50)], 0.0);
    state.apply(snapshot(50, json!({}), CheckKind::Passed), 0.0, &options());
    assert!(matches!(
        state.evaluate(0.0, &options()),
        QueueEvaluation::Waiting { emit: true, .. }
    ));
    assert!(matches!(
        state.evaluate(10.0, &options()),
        QueueEvaluation::Waiting { emit: false, .. }
    ));
    state.plan(300.0);
    assert!(matches!(state.work, Some(QueueWork::Sweep(_))));
}
#[test]
fn queued_mode_ignores_upstack_pending_for_frontier_wait() {
    let mut state = QueueState::new(&[context(20), context(21)], 0.0);
    state.apply(snapshot(20, json!({}), CheckKind::Passed), 0.0, &options());
    state.apply(snapshot(21, json!({}), CheckKind::Pending), 0.0, &options());
    let QueueEvaluation::Waiting {
        frontier, reason, ..
    } = state.evaluate(0.0, &options())
    else {
        panic!("expected wait")
    };
    assert_eq!(frontier.number, 20);
    assert_eq!(reason["kind"], "merge-queue");
}
#[test]
fn queued_runner_retries_failed_sweep_head_before_reading_next() {
    let timeline = Rc::new(RefCell::new(Vec::new()));
    let mut reader = FakeReader {
        timeline: Some(timeline.clone()),
        ..FakeReader::default()
    };
    reader
        .pull_results
        .insert(21, VecDeque::from([Err(error())]));
    let mut clock = FakeClock {
        timeline: Some(timeline.clone()),
        ..FakeClock::default()
    };
    let mut opts = options();
    opts.timeout = 65.0;
    let result = run_queued(
        &mut reader,
        &mut clock,
        &mut |v| {
            timeline
                .borrow_mut()
                .push(format!("emit:{}", v["kind"].as_str().unwrap()))
        },
        &[context(20), context(21), context(22)],
        &opts,
    );
    assert_eq!(result["kind"], "TIMEOUT");
    assert_eq!(
        &timeline.borrow()[..11],
        [
            "emit:QUEUE",
            "read:20",
            "fail:21",
            "emit:RETRY",
            "sleep",
            "read:21",
            "read:22",
            "emit:STATUS",
            "emit:WAITING",
            "sleep",
            "read:20"
        ]
    );
}
#[test]
fn advance_reads_new_frontier_without_sleeping() {
    let timeline = Rc::new(RefCell::new(Vec::new()));
    let mut reader = FakeReader {
        timeline: Some(timeline.clone()),
        ..FakeReader::default()
    };
    let mut merged = raw_facts();
    merged["state"] = json!("MERGED");
    merged["mergedAt"] = json!("now");
    reader
        .pull_results
        .insert(40, VecDeque::from([Ok(raw_facts()), Ok(merged)]));
    let mut clock = FakeClock {
        timeline: Some(timeline.clone()),
        ..FakeClock::default()
    };
    let mut opts = options();
    opts.timeout = 20.0;
    let result = run_queued(
        &mut reader,
        &mut clock,
        &mut |v| {
            timeline
                .borrow_mut()
                .push(format!("emit:{}", v["kind"].as_str().unwrap()))
        },
        &[context(40), context(41)],
        &opts,
    );
    assert_eq!(result["kind"], "TIMEOUT");
    let t = timeline.borrow();
    let index = t.iter().position(|s| s == "sleep").unwrap();
    assert_eq!(
        &t[index..index + 5],
        [
            "sleep",
            "read:40",
            "emit:ADVANCE",
            "read:41",
            "emit:WAITING"
        ]
    );
}
#[test]
fn complete_queue_only_when_every_pr_is_merged() {
    let mut reader = FakeReader::default();
    reader.facts["state"] = json!("MERGED");
    reader.facts["mergedAt"] = json!("now");
    let mut emitted = vec![];
    let result = run_queued(
        &mut reader,
        &mut FakeClock::default(),
        &mut |v| emitted.push(v),
        &[context(1), context(2)],
        &options(),
    );
    assert_eq!(result["kind"], "COMPLETE");
    assert_eq!(result["merged"].as_array().unwrap().len(), 2);
    assert_eq!(
        emitted
            .iter()
            .map(|v| v["kind"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["QUEUE", "STATUS"]
    );
}
#[test]
fn retry_floor_cap_budget_reset_and_non_retryable_failure() {
    assert_eq!(query_backoff_seconds(1.0, 1), 60.0);
    assert_eq!(query_backoff_seconds(1.0, 2), 120.0);
    assert_eq!(query_backoff_seconds(60.0, 4), 300.0);
    let mut reader = FakeReader {
        always_error: true,
        ..FakeReader::default()
    };
    let mut clock = FakeClock::default();
    let mut emitted = vec![];
    let result = run_simple(
        &mut reader,
        &mut clock,
        &mut |v| emitted.push(v),
        &[context(1)],
        Mode::Single,
        false,
        &options(),
    );
    assert_eq!(result["exitCode"], 7);
    assert_eq!(result["blocker"]["failures"], 5);
    assert_eq!(clock.sleeps, [60.0, 120.0, 240.0, 300.0]);
    assert_eq!(emitted.len(), 4);
    let mut reader = FakeReader::default();
    let mut failure = error();
    failure.retryable = false;
    reader
        .pull_results
        .insert(1, VecDeque::from([Err(failure)]));
    let mut clock = FakeClock::default();
    let result = run_simple(
        &mut reader,
        &mut clock,
        &mut |_| {},
        &[context(1)],
        Mode::Single,
        false,
        &options(),
    );
    assert_eq!(result["exitCode"], 7);
    assert!(clock.sleeps.is_empty());
}
#[test]
fn all_polling_sleep_paths_are_capped_at_deadline() {
    for mode in ["single", "queued-stack", "query-retry"] {
        let mut reader = FakeReader {
            fast: FastChecks::Checks(vec![check(CheckKind::Pending, "ci")]),
            always_error: mode == "query-retry",
            ..FakeReader::default()
        };
        let mut clock = FakeClock::default();
        let mut opts = options();
        opts.interval = 60.0;
        opts.timeout = 5.0;
        let result = if mode == "queued-stack" {
            run_queued(&mut reader, &mut clock, &mut |_| {}, &[context(1)], &opts)
        } else {
            run_simple(
                &mut reader,
                &mut clock,
                &mut |_| {},
                &[context(1)],
                Mode::Single,
                false,
                &opts,
            )
        };
        assert_eq!(result["kind"], "TIMEOUT", "{mode}");
        assert_eq!(clock.sleeps, [5.0]);
        assert_eq!(clock.now, 5.0);
    }
}
#[test]
fn deadline_disabled_keeps_interval_and_success_after_pending() {
    let mut reader = FakeReader::default();
    reader
        .fast_results
        .push_back(FastChecks::Checks(vec![check(CheckKind::Pending, "ci")]));
    let mut opts = options();
    opts.interval = 60.0;
    let mut clock = FakeClock::default();
    let mut events = vec![];
    let result = run_simple(
        &mut reader,
        &mut clock,
        &mut |v| events.push(v),
        &[context(1)],
        Mode::Single,
        false,
        &opts,
    );
    assert_eq!(result["kind"], "READY");
    assert_eq!(clock.sleeps, [60.0]);
    assert_eq!(events[0]["sequence"], 1);
    assert_eq!(result["sequence"], 2);
}
fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| (*s).into()).collect()
}
#[test]
fn cli_defaults_frozen_queue_and_numeric_validation() {
    let o = cli::parse_args(&[]).unwrap().unwrap();
    assert_eq!(o.mode, Mode::Single);
    assert!(o.owner.is_none() && o.repo.is_none() && o.pr.is_none());
    assert!(o.stack_prs.is_empty());
    assert!(!o.pretty && !o.status_only);
    assert_eq!(o.polling.interval, 60.0);
    assert_eq!(o.polling.sweep_interval, 300.0);
    assert_eq!(o.polling.timeout, 0.0);
    assert_eq!(o.polling.max_query_errors, 5);
    let o = cli::parse_args(&strings(&[
        "--queued-stack",
        "--stack-prs",
        "#10, 11,#12",
        "--interval",
        "2.5",
        "--sweep-interval",
        "30",
        "--timeout",
        "0",
        "--max-query-errors",
        "3",
        "--allow-draft",
        "--pretty",
    ]))
    .unwrap()
    .unwrap();
    assert_eq!(o.stack_prs, [10, 11, 12]);
    assert_eq!(o.mode, Mode::QueuedStack);
    assert_eq!(o.polling.interval, 2.5);
    assert_eq!(o.polling.sweep_interval, 30.0);
    assert!(o.polling.allow_draft && o.pretty);
    for argv in [
        vec!["--unknown"],
        vec!["--interval", "0"],
        vec!["--sweep-interval", "-1"],
        vec!["--timeout", "-1"],
        vec!["--max-query-errors", "1.5"],
        vec!["--stack", "--queued-stack"],
        vec!["--stack-prs", "1,2"],
        vec!["--queued-stack", "--stack-prs", "1,1"],
    ] {
        let mut stdout = vec![];
        let mut stderr = vec![];
        assert_eq!(
            cli::main_with(
                &strings(&argv),
                &mut FakeReader::default(),
                &mut FakeClock::default(),
                &mut |s| stdout.push(s),
                &mut |s| stderr.push(s)
            ),
            64
        );
        assert!(stdout.is_empty());
        assert!(stderr.join("").contains("error:"));
    }
}
#[test]
fn cli_help_usage_status_only_hidden_refusal_and_exit_codes() {
    let mut reader = FakeReader::default();
    let mut stdout = vec![];
    let mut stderr = vec![];
    assert_eq!(
        cli::main_with(
            &strings(&["--help"]),
            &mut reader,
            &mut FakeClock::default(),
            &mut |s| stdout.push(s),
            &mut |s| stderr.push(s)
        ),
        0
    );
    assert!(stdout.join("").contains("JSON (NDJSON while polling)"));
    assert!(reader.calls.is_empty());
    stdout.clear();
    assert_eq!(
        cli::main_with(
            &strings(&["--interval", "0"]),
            &mut reader,
            &mut FakeClock::default(),
            &mut |s| stdout.push(s),
            &mut |s| stderr.push(s)
        ),
        64
    );
    assert!(stdout.is_empty());
    assert!(stderr
        .join("")
        .contains("option '--interval <seconds>' argument '0' is invalid"));
    let argv = strings(&[
        "--owner",
        "owner",
        "--repo",
        "repo",
        "--queued-stack",
        "--stack-prs",
        "1",
        "--status-only",
    ]);
    assert_eq!(
        cli::main_with(
            &argv,
            &mut reader,
            &mut FakeClock::default(),
            &mut |s| stdout.push(s),
            &mut |_| {}
        ),
        0
    );
    assert_eq!(stdout.len(), 1);
    let v: Value = serde_json::from_str(&stdout[0]).unwrap();
    assert_eq!(v["kind"], "STATUS");
    assert_eq!(v["mode"], "queued-stack");
    for (patch, kind, exit) in [
        (json!({"mergeable":"CONFLICTING"}), CheckKind::Passed, 2),
        (json!({}), CheckKind::Failed, 4),
        (json!({"isDraft":true}), CheckKind::Passed, 6),
    ] {
        let mut reader = FakeReader::default();
        for (k, v) in patch.as_object().unwrap() {
            reader.facts[k] = v.clone();
        }
        reader.fast = FastChecks::Checks(vec![check(kind, "ci")]);
        assert_eq!(
            cli::main_with(
                &strings(&["--owner", "owner", "--repo", "repo", "--pr", "1"]),
                &mut reader,
                &mut FakeClock::default(),
                &mut |_| {},
                &mut |_| {}
            ),
            exit
        );
    }
    let mut reader = FakeReader::default();
    reader.facts["mergeStateStatus"] = json!("BLOCKED");
    reader.rollups[0].state = Some("FAILURE".into());
    let mut stdout = vec![];
    assert_eq!(
        cli::main_with(
            &strings(&["--owner", "owner", "--repo", "repo", "--pr", "1"]),
            &mut reader,
            &mut FakeClock::default(),
            &mut |s| stdout.push(s),
            &mut |_| {}
        ),
        4
    );
    let v: Value = serde_json::from_str(&stdout[0]).unwrap();
    assert_eq!(v["blocker"]["ci"]["kind"], "ci-github-rejected");
}
#[test]
fn render_json_and_markdown_table_from_same_event() {
    let row = snapshot(
        1,
        json!({"state":"MERGED","mergedAt":"now"}),
        CheckKind::Passed,
    );
    let event = Stamp::new(Mode::Single).stamp(
        &FakeClock::default(),
        json!({"kind":"STATUS","terminal":true,"exitCode":0,"reason":"status-only","rows":[row]}),
    );
    let rendered = render_json(&event);
    assert!(rendered.ends_with('\n'));
    assert_eq!(serde_json::from_str::<Value>(&rendered).unwrap(), event);
    assert!(render_pretty(&event)
        .contains("| [#1](https://github.com/owner/repo/pull/1) | — | — | ✅ merged |"));
    let row = snapshot(2, json!({"isDraft":true}), CheckKind::Passed);
    let Decision::Clear(prs) = select_stack_decision(&[row], true) else {
        panic!()
    };
    let event = json!({"kind":"READY","scope":{"kind":"single","pr":prs[0]}});
    assert!(render_pretty(&event).contains("leave draft — do not mark ready"));
}
struct MockRunner {
    responses: VecDeque<CommandResult>,
    calls: Vec<Vec<String>>,
}
impl MockRunner {
    fn new(responses: Vec<CommandResult>) -> Self {
        Self {
            responses: responses.into(),
            calls: vec![],
        }
    }
}
impl Runner for MockRunner {
    fn run(&mut self, argv: &[String]) -> QueryResult<CommandResult> {
        self.calls.push(argv.into());
        Ok(self.responses.pop_front().expect("unexpected command"))
    }
}
fn output(code: i32, value: Value) -> CommandResult {
    CommandResult {
        code,
        stdout: value.to_string(),
        stderr: String::new(),
    }
}
fn graphql_commits(nodes: Value) -> Value {
    json!({"data":{"repository":{"pullRequest":{"commits":{"nodes":nodes}}}}})
}
#[test]
fn gh_transport_requests_and_parses_fork_identity() {
    let mut reader = GhReader {
        runner: MockRunner::new(vec![output(
            0,
            json!([{"number":42,"headRefName":"feature","baseRefName":"main","isCrossRepository":true}]),
        )]),
    };
    let prs = reader
        .open_pull_requests(&context(42).repository())
        .unwrap();
    assert!(prs[0].is_cross_repository);
    assert_eq!(
        reader.runner.calls[0],
        strings(&[
            "gh",
            "pr",
            "list",
            "--repo",
            "owner/repo",
            "--state",
            "open",
            "--limit",
            "300",
            "--json",
            "number,headRefName,baseRefName,isCrossRepository"
        ])
    );
}
#[test]
fn gh_transport_fast_path_exit_codes_and_malformed_output_fallback() {
    for code in [0, 1, 8] {
        let mut reader = GhReader {
            runner: MockRunner::new(vec![output(
                code,
                json!([{"name":"ci","state":"SUCCESS","bucket":"pass"}]),
            )]),
        };
        assert!(matches!(
            reader.checks_fast_path(&context(42)).unwrap(),
            FastChecks::Checks(_)
        ));
    }
    for response in [
        CommandResult {
            code: 0,
            stdout: "bad json".into(),
            stderr: "oops".into(),
        },
        output(0, json!([{"name":"ci"}])),
        output(2, json!([])),
    ] {
        let mut reader = GhReader {
            runner: MockRunner::new(vec![response]),
        };
        assert!(matches!(
            reader.checks_fast_path(&context(42)).unwrap(),
            FastChecks::Unusable { .. }
        ));
    }
}
#[test]
fn gh_transport_graphql_queries_and_rollup_validation() {
    let rollup = graphql_commits(
        json!([{"commit":{"statusCheckRollup":{"contexts":{"nodes":[{"__typename":"CheckRun","name":"ci","status":"COMPLETED","conclusion":"SUCCESS"}],"pageInfo":{"hasNextPage":true,"endCursor":"next"}}}}}]),
    );
    let threads = json!({"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}});
    let commits = graphql_commits(
        json!([{"commit":{"oid":"head","statusCheckRollup":{"state":"SUCCESS"}}},{"commit":{"oid":"old","statusCheckRollup":null}}]),
    );
    let mut reader = GhReader {
        runner: MockRunner::new(vec![
            output(0, rollup),
            output(0, threads),
            output(0, commits),
        ]),
    };
    let page = reader
        .check_rollup_page(&context(42), Some("cursor"))
        .unwrap();
    assert_eq!(page.end_cursor.as_deref(), Some("next"));
    assert_eq!(page.checks[0].kind, CheckKind::Passed);
    reader.review_thread_page(&context(42), None).unwrap();
    let commits = reader.commit_rollups(&context(42)).unwrap();
    assert_eq!(commits[0].state.as_deref(), Some("SUCCESS"));
    assert!(commits[1].state.is_none());
    for (call, query) in reader.runner.calls.iter().zip([
        PR_CHECK_ROLLUP_QUERY,
        REVIEW_THREADS_QUERY,
        PR_COMMIT_STATUS_QUERY,
    ]) {
        assert_eq!(call[4], format!("query={query}"));
        assert!(call.contains(&"owner=owner".into()));
        assert!(call.contains(&"repo=repo".into()));
        assert!(call.contains(&"pr=42".into()));
    }
    assert_eq!(
        &reader.runner.calls[0][11..],
        strings(&["-f", "after=cursor"])
    );
    let mut reader = GhReader {
        runner: MockRunner::new(vec![output(
            0,
            graphql_commits(json!([{"commit":{"oid":"head","statusCheckRollup":{"state":"NEW"}}}])),
        )]),
    };
    assert_eq!(
        reader
            .commit_rollups(&context(42))
            .unwrap_err()
            .raw_value
            .as_deref(),
        Some("\"NEW\"")
    );
}
#[test]
fn gh_transport_command_and_json_errors_are_structured() {
    let mut reader = GhReader {
        runner: MockRunner::new(vec![CommandResult {
            code: 1,
            stdout: String::new(),
            stderr: " denied\nmore detail".into(),
        }]),
    };
    let e = reader.pull_request(&context(42)).unwrap_err();
    assert_eq!(e.kind, "command-exit");
    assert_eq!(e.code, Some(1));
    assert_eq!(e.detail, "denied");
    let mut reader = GhReader {
        runner: MockRunner::new(vec![CommandResult {
            code: 0,
            stdout: "broken".into(),
            stderr: String::new(),
        }]),
    };
    assert_eq!(
        reader.pull_request(&context(42)).unwrap_err().kind,
        "json-parse"
    );
}

#[test]
fn review_threads_exit_three_and_outrank_failing_ci() {
    let mut reader = FakeReader {
        fast: FastChecks::Checks(vec![check(CheckKind::Failed, "ci")]),
        ..FakeReader::default()
    };
    reader.thread_pages.push_back(ThreadPage {
        threads: vec![raw_thread("thread", "someone", "fix this", false)],
        end_cursor: None,
    });
    let mut stdout = vec![];
    let code = cli::main_with(
        &strings(&["--owner", "owner", "--repo", "repo", "--pr", "1"]),
        &mut reader,
        &mut FakeClock::default(),
        &mut |s| stdout.push(s),
        &mut |_| {},
    );
    assert_eq!(code, 3);
    let event: Value = serde_json::from_str(&stdout[0]).unwrap();
    assert_eq!(event["blocker"]["kind"], "review-threads");
    assert!(render_pretty(&event)
        .contains("thread a.ts 1 someone isBugBot=false bugbotReviewPasses=0 fix this"));
}

#[test]
fn retry_budget_resets_after_successful_pending_poll() {
    let mut reader = FakeReader::default();
    reader.pull_results.insert(
        1,
        VecDeque::from([Err(error()), Ok(raw_facts()), Err(error()), Ok(raw_facts())]),
    );
    reader
        .fast_results
        .push_back(FastChecks::Checks(vec![check(CheckKind::Pending, "ci")]));
    let mut opts = options();
    opts.max_query_errors = 2;
    let mut events = vec![];
    let result = run_simple(
        &mut reader,
        &mut FakeClock::default(),
        &mut |v| events.push(v),
        &[context(1)],
        Mode::Single,
        false,
        &opts,
    );
    assert_eq!(result["kind"], "READY");
    let retries: Vec<_> = events.iter().filter(|v| v["kind"] == "RETRY").collect();
    assert_eq!(retries.len(), 2);
    assert!(retries.iter().all(|v| v["consecutiveFailures"] == 1));
}

#[test]
fn pending_history_and_review_automation_are_reflected_in_status() {
    let mut reader = FakeReader::default();
    reader.fast = FastChecks::Checks(vec![check(
        CheckKind::Pending,
        "Security Review automation",
    )]);
    reader.rollups.push(CommitRollup {
        oid: "old".into(),
        state: Some("SUCCESS".into()),
    });
    let row = read_snapshot(&mut reader, &context(1), true).unwrap();
    let v = serde_json::to_value(&row).unwrap();
    assert_eq!(v["reviewAutomationRunning"], true);
    assert_eq!(v["ci"]["hadPreviousPassingCi"], true);
    let table = render_status_table(&json!([row]));
    assert!(table.contains("⏳ 1 pending, was ✅"));
    assert!(table.contains("🤖 running"));
}

#[test]
fn transport_spawn_failures_keep_stderr_only_exit_one() {
    let mut reader = FakeReader::default();
    let mut failure = Failure::new("command-spawn", "gh: executable not found");
    failure.retryable = false;
    reader
        .pull_results
        .insert(1, VecDeque::from([Err(failure)]));
    let mut stdout = vec![];
    let mut stderr = vec![];
    let mut clock = FakeClock::default();
    let code = cli::main_with(
        &strings(&["--owner", "owner", "--repo", "repo", "--pr", "1"]),
        &mut reader,
        &mut clock,
        &mut |s| stdout.push(s),
        &mut |s| stderr.push(s),
    );
    assert_eq!(code, 1);
    assert!(stdout.is_empty());
    assert!(stderr.join("").contains("gh: executable not found"));
    assert!(clock.sleeps.is_empty());
}

#[test]
fn gh_transport_origin_current_pr_and_pull_request_commands() {
    let mut reader = GhReader {
        runner: MockRunner::new(vec![
            CommandResult {
                code: 0,
                stdout: "git@github.com:owner/repo.git\n".into(),
                stderr: String::new(),
            },
            output(
                0,
                json!({"number":42,"url":"https://github.com/owner/repo/pull/42"}),
            ),
            output(0, raw_facts()),
        ]),
    };
    assert_eq!(
        reader.origin_repo().unwrap(),
        Some(context(42).repository())
    );
    assert_eq!(reader.current_pr(Some(42)).unwrap(), context(42));
    assert_eq!(reader.pull_request(&context(42)).unwrap().state, "OPEN");
    assert_eq!(
        reader.runner.calls[0],
        strings(&["git", "remote", "get-url", "origin"])
    );
    assert_eq!(
        reader.runner.calls[1],
        strings(&["gh", "pr", "view", "42", "--json", "number,url"])
    );
    assert_eq!(reader.runner.calls[2],strings(&["gh","pr","view","42","--repo","owner/repo","--json","mergeable,mergeStateStatus,reviewDecision,headRefOid,headRefName,baseRefName,state,mergedAt,isDraft"]));
}

#[test]
fn pretty_progress_and_terminal_messages_match_contract() {
    for (v, expected) in [
        (
            json!({"kind":"QUEUE","queue":[context(1),context(2)]}),
            "QUEUE: captured 2 PRs bottom-to-top: #1,#2\n",
        ),
        (
            json!({"kind":"WAITING","frontier":context(1),"reason":{"kind":"pending-checks","pending":[check(CheckKind::Pending,"ci")]}}),
            "WAITING: frontier=#1; 1 check pending\n",
        ),
        (
            json!({"kind":"WAITING","frontier":context(1),"reason":{"kind":"merge-queue","unmergedCount":2}}),
            "WAITING: frontier=#1 is blocker-free; waiting for merge queue (2 PRs unmerged)\n",
        ),
        (
            json!({"kind":"ADVANCE","merged":context(1),"frontier":context(2),"remaining":1}),
            "ADVANCE: merged #1; next=#2; remaining=1\n",
        ),
        (
            json!({"kind":"RETRY","retryInSeconds":60.0,"failure":{"detail":"limited"}}),
            "RETRY: GitHub status query failed; retrying in 60s\ndetail=limited\n",
        ),
        (
            json!({"kind":"COMPLETE","queue":[context(1)]}),
            "COMPLETE: queued stack merged (1 PR)\n",
        ),
        (
            json!({"kind":"TIMEOUT","reason":{"kind":"pending-checks"}}),
            "TIMEOUT: checks still pending\n",
        ),
        (
            json!({"kind":"TIMEOUT","reason":{"kind":"status-unavailable"}}),
            "TIMEOUT: GitHub status remained unavailable\n",
        ),
        (
            json!({"kind":"TIMEOUT","reason":{"kind":"queued-stack","frontier":context(2),"unmergedCount":1}}),
            "TIMEOUT: queued stack still has 1 PR unmerged; frontier=#2\n",
        ),
    ] {
        assert_eq!(render_pretty(&v), expected);
    }
}
