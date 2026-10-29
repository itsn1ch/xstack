use std::collections::BTreeSet;
use xstack_worker::state::{config_key, Config, Entry, State, Status};

fn config() -> Config {
    Config {
        checks: vec!["check".into()],
        copy: vec![],
        timeout_ms: 1,
        parallel: None,
        commits: 1,
        job_mem_mb: 1,
    }
}

// 0 means absent; the other 80 values cover status × tree × key × link.
fn entry(id: usize, code: usize, good_key: &str) -> Option<Entry> {
    if code == 0 {
        return None;
    }
    let mut n = code - 1;
    let status = [
        Status::Queued,
        Status::Running,
        Status::Skipped,
        Status::Pass,
        Status::Fail,
    ][n % 5];
    n /= 5;
    let tree = format!("t{}", n % 2);
    n /= 2;
    let key = if n % 2 == 0 {
        good_key
    } else {
        "old configuration"
    };
    n /= 2;
    let link = [None, Some("a"), Some("b"), Some("missing")][n % 4];
    Some(Entry {
        sha: ["a", "b"][id].into(),
        tree,
        status,
        checks: vec![],
        config_key: key.into(),
        reused_from: None,
        baseline: false,
        explicit: false,
        queued_at: 0,
        started_at: None,
        ended_at: None,
        pid: None,
        superseded_by: link.map(str::to_string),
    })
}

fn valid(state: &State, e: &Entry) -> bool {
    matches!(e.status, Status::Pass | Status::Fail)
        && state
            .config
            .as_ref()
            .is_some_and(|c| config_key(c) == e.config_key)
}

// Specify the allowed sources, not the implementation's selection order.
fn allowed(state: &State, sha: &str, tree: &str, own: bool) -> BTreeSet<String> {
    let mut sources = BTreeSet::new();
    for (id, e) in &state.entries {
        if valid(state, e) && (id == sha || e.tree == tree) {
            sources.insert(id.clone());
        }
    }
    // An own result takes precedence over all superseding results.
    if own || !sources.is_empty() {
        return sources;
    }
    let mut seen = BTreeSet::new();
    let mut cursor = sha;
    while seen.insert(cursor.to_owned()) {
        let Some(e) = state.entries.get(cursor) else {
            break;
        };
        if e.status != Status::Skipped {
            break;
        }
        let Some(next) = e.superseded_by.as_deref() else {
            break;
        };
        let Some(target) = state.entries.get(next) else {
            break;
        };
        if valid(state, target) {
            sources.insert(next.to_owned());
            break;
        }
        cursor = next;
    }
    sources
}

fn main() {
    let cfg = config();
    let key = config_key(&cfg);
    let mut cases = 0u64;
    for a in 0..=80 {
        for b in 0..=80 {
            for configured in [false, true] {
                let mut state = State::default();
                if configured {
                    state.config = Some(cfg.clone());
                }
                for (id, code) in [a, b].into_iter().enumerate() {
                    if let Some(e) = entry(id, code, &key) {
                        state.entries.insert(e.sha.clone(), e);
                    }
                }
                for (sha, tree) in [
                    (
                        "a",
                        state.entries.get("a").map_or("t0", |e| e.tree.as_str()),
                    ),
                    (
                        "b",
                        state.entries.get("b").map_or("t0", |e| e.tree.as_str()),
                    ),
                    ("missing", "t0"),
                    ("missing", "t1"),
                ] {
                    for own in [false, true] {
                        let expected = allowed(&state, sha, tree, own);
                        let actual = state.result_for(sha, tree, own).map(|e| e.sha.as_str());
                        assert!(match actual { Some(id) => expected.contains(id), None => expected.is_empty() },
                            "counterexample: a={a} b={b} configured={configured} query=({sha},{tree},own={own}) allowed={expected:?} actual={actual:?} state={state:?}");
                        cases += 1;
                    }
                }
            }
        }
    }
    println!("PASS: {cases} result-selection queries across 13,122 bounded states");
}
