use super::{cli, store::*};
use serde_json::{json, Value};
use std::fs;
use tempfile::TempDir;

fn initialized() -> (TempDir, Store) {
    let directory = TempDir::new().unwrap();
    let mut store = Store::open(directory.path(), false).unwrap();
    store.init().unwrap();
    (directory, store)
}
fn run(directory: Option<&TempDir>, args: &[&str]) -> cli::Output {
    cli::run(
        &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        directory.map(|d| d.path().to_string_lossy().into_owned()),
        None,
    )
}
fn json_output(output: cli::Output) -> Value {
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).unwrap()
}

#[test]
fn initialization_is_idempotent_and_releases_lock() {
    let (directory, mut store) = initialized();
    let first = fs::read(directory.path().join("units.tsv")).unwrap();
    assert_eq!(
        store.init().unwrap(),
        json!({"store": directory.path().to_string_lossy()})
    );
    assert_eq!(fs::read(directory.path().join("units.tsv")).unwrap(), first);
    assert_eq!(
        fs::read_to_string(directory.path().join("ledger.tsv")).unwrap(),
        "pr\tsha\tverdict\tevidence\tverifier\tts\n"
    );
    let mut files: Vec<_> = fs::read_dir(directory.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    assert_eq!(
        files,
        [
            ".orch.lock",
            "frontier.json",
            "gates.md",
            "inbox",
            "ledger.tsv",
            "preferences.md",
            "units.tsv"
        ]
    );
    store.close().unwrap();
    store.close().unwrap();
    assert!(!directory.path().join(".orch.lock").exists());
}
#[test]
fn units_compose_and_preserve_canonical_tsv() {
    let (directory, mut store) = initialized();
    assert_eq!(
        store
            .unit_add("u1", "build", Some("briefs/u1.md"))
            .unwrap()
            .state,
        "pending"
    );
    let escaped = store.unit_add("=SUM(A1)", "+build", None).unwrap();
    assert_eq!(escaped.id, "'=SUM(A1)");
    assert_eq!(escaped.track, "'+build");
    let updated = store
        .unit_set(
            "u1",
            "done",
            Some("poteto/u1"),
            Some(184530),
            Some("abc123"),
        )
        .unwrap();
    assert_eq!(
        serde_json::to_value(&updated).unwrap(),
        json!({"id":"u1","track":"build","state":"done","branch":"poteto/u1","pr":"184530","sha":"abc123","brief":"briefs/u1.md"})
    );
    assert_eq!(store.unit_get("u1").unwrap(), updated);
    assert_eq!(
        store.unit_list(Some("done"), Some("build")).unwrap(),
        [updated]
    );
    assert_eq!(
        serde_json::to_value(store.unit_counts().unwrap()).unwrap(),
        json!({"done":1,"pending":1})
    );
    assert!(store
        .unit_add("u1", "build", None)
        .unwrap_err()
        .message
        .contains("already exists"));
    assert_eq!(
        store
            .unit_set("missing", "done", None, None, None)
            .unwrap_err()
            .code,
        2
    );
    let tsv = fs::read_to_string(directory.path().join("units.tsv")).unwrap();
    assert!(tsv.contains("u1\tbuild\tdone\tpoteto/u1\t184530\tabc123\tbriefs/u1.md\n"));
    assert!(tsv.contains("'=SUM(A1)\t'+build\tpending\t\t\t\t\n"));
}
#[test]
fn cell_and_line_cleaning_is_preserved() {
    let (_directory, mut store) = initialized();
    assert_eq!(
        store
            .unit_add("tab\tline\nreturn\r", "track", None)
            .unwrap()
            .id,
        "tab line return "
    );
    for value in ["=1", "+1", "-1", "@name"] {
        assert_eq!(clean_cell(value), format!("'{value}"));
    }
    assert!(store
        .unit_add("\n\t", "build", None)
        .unwrap_err()
        .message
        .contains("unit id must not be empty"));
    assert!(store
        .unit_add("ok", "build", Some(" "))
        .unwrap_err()
        .message
        .contains("brief must not be empty"));
    assert_eq!(
        store.standing_add("  No\nforce\rpush.  ").unwrap().line,
        "No force push."
    );
}
#[test]
fn ledger_records_replaces_checks_and_summarizes() {
    let (_directory, mut store) = initialized();
    let missing = store.ledger_check(184530, "abc123").unwrap_err();
    assert_eq!(missing.code, 2);
    assert_eq!(missing.message, "NOT-VERIFIED");
    assert_eq!(
        missing.output,
        Some(json!({"pr":"184530","sha":"abc123","verdict":"NOT-VERIFIED"}))
    );
    assert!(parse_verdict("looks-good")
        .unwrap_err()
        .message
        .starts_with("verdict must be"));
    for verdict in [
        "live-ui-verified",
        "unit-test-verified",
        "type-check-only",
        "verifier-blocked",
        "verifier-failed",
    ] {
        assert_eq!(parse_verdict(verdict).unwrap(), verdict);
    }
    let entry = store
        .ledger_record(
            184530,
            "abc123",
            "unit-test-verified",
            "reports/verify.md",
            Some("opus"),
        )
        .unwrap();
    assert_eq!(store.ledger_check(184530, "abc123").unwrap(), entry);
    assert!(entry.ts.ends_with('Z'));
    assert_eq!(entry.verifier, "opus");
    assert_eq!(
        store.ledger_summary().unwrap().get("unit-test-verified"),
        Some(&1)
    );
    store
        .ledger_record(
            184530,
            "abc123",
            "live-ui-verified",
            "reports/live.md",
            None,
        )
        .unwrap();
    assert_eq!(store.ledger().unwrap().len(), 1);
    assert_eq!(
        store.ledger_summary().unwrap().get("live-ui-verified"),
        Some(&1)
    );
    assert!(store
        .ledger_record(0, "sha", "type-check-only", "report", None)
        .unwrap_err()
        .message
        .contains("positive integer"));
}
#[test]
fn inbox_push_peek_and_atomic_drain() {
    let (directory, mut store) = initialized();
    let (first, filename) = store
        .inbox_push("worker-1", "u1", "done", Some("reports/u1.md"))
        .unwrap();
    assert_eq!(first.unit, "u1");
    assert!(filename.ends_with(".tsv"));
    let persisted = fs::read_to_string(directory.path().join("inbox").join(filename)).unwrap();
    assert_eq!(persisted, format!("{}\n", first.cells().join("\t")));
    store.inbox_push("worker-2", "u2", "failed", None).unwrap();
    assert_eq!(store.inbox_peek().unwrap().len(), 2);
    assert_eq!(store.inbox_peek().unwrap().len(), 2);
    assert_eq!(store.inbox_drain().unwrap().len(), 2);
    assert!(store.inbox_peek().unwrap().is_empty());
    assert!(!fs::read_dir(directory.path()).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".inbox-drain-")));
}
#[test]
fn read_only_operations_do_not_take_lock() {
    let (directory, mut store) = initialized();
    store.close().unwrap();
    fs::write(directory.path().join(".orch.lock"), "unknown\n").unwrap();
    let reader = Store::open(directory.path(), false).unwrap();
    assert!(reader.units().unwrap().is_empty());
    assert!(reader.inbox_peek().unwrap().is_empty());
    assert_eq!(reader.frontier_show().unwrap(), Frontier::default());
    assert_eq!(
        fs::read_to_string(directory.path().join(".orch.lock")).unwrap(),
        "unknown\n"
    );
}
#[test]
fn live_lock_blocks_and_force_steals_with_warning() {
    let (directory, mut store) = initialized();
    store.close().unwrap();
    fs::write(
        directory.path().join(".orch.lock"),
        format!("{}\n", std::process::id()),
    )
    .unwrap();
    let mut blocked = Store::open(directory.path(), false).unwrap();
    assert_eq!(
        blocked.unit_add("u1", "build", None).unwrap_err().message,
        format!("store lock held by pid {}", std::process::id())
    );
    let mut forced = Store::open(directory.path(), true).unwrap();
    forced.unit_add("u1", "build", None).unwrap();
    assert_eq!(
        forced.warnings,
        [format!(
            "stealing store lock held by pid {}\n",
            std::process::id()
        )]
    );
    forced.close().unwrap();
    assert!(!directory.path().join(".orch.lock").exists());
}
#[test]
#[cfg(unix)]
fn stale_lock_is_replaced_without_force() {
    let (directory, mut store) = initialized();
    store.close().unwrap();
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    fs::write(directory.path().join(".orch.lock"), format!("{pid}\n")).unwrap();
    let mut recovered = Store::open(directory.path(), false).unwrap();
    recovered.unit_add("u1", "build", None).unwrap();
    assert_eq!(
        recovered.warnings,
        [format!("replacing stale store lock (pid {pid} is dead)\n")]
    );
    recovered.close().unwrap();
    assert!(!directory.path().join(".orch.lock").exists());
}
#[test]
fn closing_does_not_release_someone_elses_lock() {
    let (directory, mut store) = initialized();
    fs::write(directory.path().join(".orch.lock"), "another-holder\n").unwrap();
    store.close().unwrap();
    assert_eq!(
        fs::read_to_string(directory.path().join(".orch.lock")).unwrap(),
        "another-holder\n"
    );
}
#[test]
fn gates_standing_orders_and_status_remain_plain_files() {
    let (directory, mut store) = initialized();
    store.unit_add("u1", "build", None).unwrap();
    assert_eq!(
        store
            .gate_park("release", "Ship now?", "ship,wait", "wait")
            .unwrap()
            .kind,
        "open"
    );
    assert_eq!(
        store.standing_add("Never force push.").unwrap(),
        StandingLine {
            number: 1,
            line: "Never force push.".into()
        }
    );
    let first = store.status_render().unwrap();
    assert_eq!(first.changed, "first render");
    assert_eq!(first.summary.open_gate_ids, ["release"]);
    assert!(fs::read_to_string(directory.path().join("status.md"))
        .unwrap()
        .contains("| release | open | Ship now? |"));
    assert_eq!(store.status_render().unwrap().changed, "no derived changes");
    assert_eq!(
        store
            .gate_resolve("release", "ship")
            .unwrap()
            .answer
            .as_deref(),
        Some("ship")
    );
    assert_eq!(store.status_render().unwrap().changed, "open gates 1->0");
    assert!(store.gate_list().unwrap().is_empty());
    assert_eq!(
        store.standing_show().unwrap(),
        [StandingLine {
            number: 1,
            line: "Never force push.".into()
        }]
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("preferences.md")).unwrap(),
        "1. Never force push.\n"
    );
    store
        .gate_park("release", "Again?", "yes,no", "no")
        .unwrap();
    assert_eq!(store.gates().unwrap().len(), 1);
    assert_eq!(store.gate_list().unwrap().len(), 1);
    assert_eq!(store.gate_resolve("missing", "x").unwrap_err().code, 2);
}
#[test]
fn status_reports_all_derived_deltas_and_escapes_tables() {
    let (directory, mut store) = initialized();
    store.unit_add("a|b\\c", "build", None).unwrap();
    store.status_render().unwrap();
    store.unit_set("a|b\\c", "done", None, None, None).unwrap();
    store
        .ledger_record(1, "sha", "type-check-only", "report", None)
        .unwrap();
    store.gate_park("g", "Q?", "a,b", "a").unwrap();
    fs::write(
        directory.path().join("frontier.json"),
        "{\"generation\":2,\"prs\":[],\"lowestUnmerged\":null}\n",
    )
    .unwrap();
    let report = store.status_render().unwrap();
    assert_eq!(report.changed, "units done 0->1; units pending 1->0; ledger type-check-only 0->1; frontier generation 0->2; open gates 0->1");
    let markdown = fs::read_to_string(directory.path().join("status.md")).unwrap();
    assert!(markdown.contains("a\\|b\\\\c"));
    assert!(markdown.contains("<!-- orch-summary "));
}
#[test]
fn malformed_persisted_data_is_rejected_loudly() {
    let (directory, store) = initialized();
    fs::write(directory.path().join("units.tsv"), "wrong\n").unwrap();
    assert_eq!(
        store.units().unwrap_err().message,
        "units.tsv has an invalid header"
    );
    fs::write(
        directory.path().join("units.tsv"),
        "id\ttrack\tstate\tbranch\tpr\tsha\tbrief\nshort\trow\n",
    )
    .unwrap();
    assert_eq!(
        store.units().unwrap_err().message,
        "units.tsv has a malformed row"
    );
    fs::write(
        directory.path().join("ledger.tsv"),
        "pr\tsha\tverdict\tevidence\tverifier\tts\n1\tsha\tinvalid\treport\tme\tnow\n",
    )
    .unwrap();
    assert_eq!(
        store.ledger_summary().unwrap_err().message,
        "ledger.tsv has invalid verdict invalid"
    );
    for (raw, expected) in [
        ("bad", "frontier.json is not valid JSON"),
        ("[]", "frontier.json must contain an object"),
        (
            "{\"generation\":\"1\"}",
            "frontier.json has an invalid shape",
        ),
        (
            "{\"generation\":1,\"prs\":[{}],\"lowestUnmerged\":null}",
            "frontier.json has an invalid PR row",
        ),
    ] {
        fs::write(directory.path().join("frontier.json"), raw).unwrap();
        assert_eq!(store.frontier_show().unwrap_err().message, expected);
    }
    fs::write(directory.path().join("inbox/bad.tsv"), "too\tshort\n").unwrap();
    assert_eq!(
        store.inbox_peek().unwrap_err().message,
        "inbox pointer bad.tsv is malformed"
    );
    fs::write(directory.path().join("gates.md"), "# Wrong\n").unwrap();
    assert_eq!(
        store.gates().unwrap_err().message,
        "gates.md has an invalid heading"
    );
    fs::write(directory.path().join("preferences.md"), "2. wrong\n").unwrap();
    assert_eq!(
        store.standing_show().unwrap_err().message,
        "preferences.md has malformed numbering"
    );
}
#[test]
fn operations_after_close_and_uninitialized_writes_fail() {
    let (_directory, mut store) = initialized();
    store.close().unwrap();
    assert_eq!(store.units().unwrap_err().message, "store is closed");
    assert_eq!(
        store.status_render().unwrap_err().message,
        "store is closed"
    );
    let directory = TempDir::new().unwrap();
    let mut missing = Store::open(directory.path().join("missing"), false).unwrap();
    assert!(missing
        .unit_add("u1", "build", None)
        .unwrap_err()
        .message
        .contains("run orch init"));
    assert!(!directory.path().join("missing").exists());
}
#[test]
fn cli_help_and_invalid_parsing_keep_exit_one() {
    let help = run(None, &["--help"]);
    assert_eq!(help.code, 0);
    assert!(help.stdout.contains("Commands:"));
    assert!(help.stdout.contains("unit"));
    assert!(help.stdout.contains("ledger"));
    let help = run(None, &["frontier", "set", "--help"]);
    for flag in ["--repo <dir>", "--forge <forge>", "--prs <n,...>"] {
        assert!(help.stdout.contains(flag));
    }
    let directory = TempDir::new().unwrap();
    let invalid = run(Some(&directory), &["unit", "add", "u1"]);
    assert_eq!(invalid.code, 1);
    assert!(invalid.stderr.contains("required option '--track <track>'"));
    let invalid = run(
        Some(&directory),
        &["frontier", "set", "--repo", ".", "--forge", "gt"],
    );
    assert_eq!(invalid.code, 1);
    assert!(invalid.stderr.contains("Allowed choices are gh, origin"));
    for args in [
        &["unit", "get"][..],
        &["unit", "get", "a", "extra"],
        &["--unknown"],
        &["unit", "set", "u1", "--state", "done", "--pr", "01"],
        &["frontier", "set", "--prs", "1,,2"],
    ] {
        assert_eq!(run(Some(&directory), args).code, 1);
    }
}
#[test]
fn cli_accepts_environment_store_and_complete_json() {
    let directory = TempDir::new().unwrap();
    assert_eq!(run(Some(&directory), &["init"]).code, 0);
    assert_eq!(
        json_output(run(
            Some(&directory),
            &["unit", "add", "u1", "--track", "build", "--json"]
        )),
        json!({"id":"u1","track":"build","state":"pending","branch":"","pr":"","sha":"","brief":""})
    );
    assert_eq!(
        json_output(run(Some(&directory), &["--json", "unit", "list"]))
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        run(Some(&directory), &["unit", "counts"]).stdout,
        "pending=1\n"
    );
    assert_eq!(
        run(Some(&directory), &["unit", "get", "u1"]).stdout,
        "u1\tbuild\tpending\t\t\t\t\n"
    );
}
#[test]
fn cli_maps_user_and_not_found_outcomes() {
    let directory = TempDir::new().unwrap();
    assert_eq!(run(Some(&directory), &["init"]).code, 0);
    let missing = run(Some(&directory), &["frontier", "set"]);
    assert_eq!(missing.code, 1);
    assert!(missing.stderr.contains("set --repo <dir> or ORCH_REPO"));
    let invalid = run(Some(&directory), &["unit", "add", "", "--track", "build"]);
    assert_eq!(invalid.code, 1);
    assert!(invalid.stderr.contains("unit id must not be empty"));
    let missing = run(Some(&directory), &["unit", "get", "missing"]);
    assert_eq!(missing.code, 2);
    assert!(missing.stderr.contains("unit missing not found"));
    let missing = run(
        Some(&directory),
        &["--json", "ledger", "check", "184530", "abc123"],
    );
    assert_eq!(missing.code, 2);
    assert!(missing.stderr.is_empty());
    assert_eq!(
        serde_json::from_str::<Value>(&missing.stdout).unwrap(),
        json!({"pr":"184530","sha":"abc123","verdict":"NOT-VERIFIED"})
    );
    assert_eq!(
        run(Some(&directory), &["ledger", "check", "1", "sha"]).stdout,
        "NOT-VERIFIED\n"
    );
    assert_eq!(run(None, &["unit"]).code, 1);
}
#[test]
fn cli_drains_every_pointer_but_limits_other_human_lists() {
    let directory = TempDir::new().unwrap();
    assert_eq!(run(Some(&directory), &["init"]).code, 0);
    for index in 0..6 {
        let id = format!("u{index}");
        assert_eq!(
            run(Some(&directory), &["unit", "add", &id, "--track", "build"]).code,
            0
        );
        assert_eq!(
            run(Some(&directory), &["inbox", "push", "agent", &id, "done"]).code,
            0
        );
        assert_eq!(run(Some(&directory), &["standing", "add", &id]).code, 0);
        assert_eq!(
            run(
                Some(&directory),
                &[
                    "gate",
                    "park",
                    &id,
                    "--question",
                    "Q",
                    "--options",
                    "a,b",
                    "--default",
                    "a"
                ]
            )
            .code,
            0
        );
    }
    for args in [
        &["unit", "list"][..],
        &["standing", "show"],
        &["gate", "list"],
    ] {
        assert!(run(Some(&directory), args)
            .stdout
            .contains("... 2 more; use --json"));
    }
    assert_eq!(
        json_output(run(Some(&directory), &["--json", "unit", "list"]))
            .as_array()
            .unwrap()
            .len(),
        6
    );
    assert_eq!(
        run(Some(&directory), &["inbox", "drain", "--peek"])
            .stdout
            .lines()
            .count(),
        6
    );
    assert_eq!(
        json_output(run(Some(&directory), &["--json", "inbox", "count"])),
        json!({"count":6})
    );
    assert_eq!(
        run(Some(&directory), &["inbox", "drain"])
            .stdout
            .lines()
            .count(),
        6
    );
    assert_eq!(run(Some(&directory), &["inbox", "count"]).stdout, "0\n");
    let status = run(Some(&directory), &["status"]);
    assert_eq!(status.stdout.lines().count(), 3);
    assert!(status.stdout.contains("ids=u0,u1,u2,u3,+2 more"));
}
#[test]
fn cli_explicit_store_overrides_environment() {
    let directory = TempDir::new().unwrap();
    let other = TempDir::new().unwrap();
    let args = vec![
        "--store".into(),
        directory.path().to_string_lossy().into_owned(),
        "init".into(),
    ];
    let result = cli::run(
        &args,
        Some(other.path().to_string_lossy().into_owned()),
        None,
    );
    assert_eq!(result.code, 0);
    assert!(directory.path().join("units.tsv").exists());
    assert!(!other.path().join("units.tsv").exists());
}

#[test]
fn cli_persists_dash_prefixed_required_values() {
    let directory = TempDir::new().unwrap();
    assert_eq!(run(Some(&directory), &["init"]).code, 0);
    let gate = json_output(run(
        Some(&directory),
        &[
            "--json",
            "gate",
            "park",
            "release",
            "--question",
            "- ship now?",
            "--options",
            "- ship,- wait",
            "--default",
            "- wait",
        ],
    ));
    assert_eq!(gate["question"], "- ship now?");
    let resolved = json_output(run(
        Some(&directory),
        &[
            "--json",
            "gate",
            "resolve",
            "release",
            "--answer",
            "- defer deployment",
        ],
    ));
    assert_eq!(resolved["answer"], "- defer deployment");
    let store = Store::open(directory.path(), false).unwrap();
    assert_eq!(
        store.gates().unwrap()[0].answer.as_deref(),
        Some("- defer deployment")
    );
}

#[test]
fn status_delta_reads_gate_ids_containing_comment_delimiters() {
    let (_directory, mut store) = initialized();
    store
        .gate_park("release --> production", "Ship?", "yes,no", "no")
        .unwrap();
    store.status_render().unwrap();
    store.gate_resolve("release --> production", "no").unwrap();
    assert_eq!(store.status_render().unwrap().changed, "open gates 1->0");
}
