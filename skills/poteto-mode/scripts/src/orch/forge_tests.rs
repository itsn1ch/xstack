use super::*;
use std::cell::RefCell;
use tempfile::TempDir;

struct Fixture {
    forge: String,
    rows: BTreeMap<(String, String), String>,
    calls: RefCell<Vec<(String, String)>>,
    current: String,
    remote_head: Option<String>,
    invalid_sha: bool,
}
impl Fixture {
    fn new(forge: &str) -> Self {
        let mut fixture = Self {
            forge: forge.into(),
            rows: BTreeMap::new(),
            calls: RefCell::new(Vec::new()),
            current: "stack/open".into(),
            remote_head: None,
            invalid_sha: false,
        };
        fixture.put(
            "--head",
            "stack/open",
            vec![(11, "open", "stack/open", "stack/closed")],
        );
        fixture.put(
            "--head",
            "stack/closed",
            vec![(13, "closed", "stack/closed", "stack/merged")],
        );
        fixture.put(
            "--head",
            "stack/merged",
            vec![(10, "merged", "stack/merged", "main")],
        );
        fixture.put(
            "--base",
            "stack/open",
            vec![
                (14, "draft", "stack/draft", "stack/open"),
                (15, "closed", "stack/abandoned", "stack/open"),
            ],
        );
        fixture.put("--base", "stack/draft", vec![]);
        fixture
    }
    fn put(&mut self, filter: &str, branch: &str, rows: Vec<(u64, &str, &str, &str)>) {
        let rows: Vec<_> = rows.into_iter().map(|(pr, state, head, base)| {
            if self.forge == "gh" {
                json!({"number":pr,"state":if state == "draft" { "OPEN".into() } else { state.to_uppercase() },"headRefName":head,"baseRefName":base})
            } else { json!({"number":pr,"status":state,"headRef":head,"baseRef":base}) }
        }).collect();
        self.rows.insert(
            (filter.into(), branch.into()),
            serde_json::to_string(&rows).unwrap(),
        );
    }
}
impl ForgeQuery for Fixture {
    fn git(&self, args: &[&str]) -> Result<String> {
        match args {
            ["symbolic-ref", "--short", "refs/remotes/origin/HEAD"] => self
                .remote_head
                .clone()
                .ok_or_else(|| Error::user("no origin HEAD")),
            ["symbolic-ref", "--short", "HEAD"] => Ok(self.current.clone()),
            ["rev-parse", branch] if !self.invalid_sha => Ok(format!(
                "{:0<40}",
                branch
                    .replace("stack/", "")
                    .bytes()
                    .map(|b| format!("{:x}", b % 16))
                    .collect::<String>()
            )),
            ["rev-parse", _] => Ok("bad-sha".into()),
            _ => Err(Error::user("unexpected git arguments")),
        }
    }
    fn list(&self, filter: &str, branch: &str) -> Result<Vec<PullRequest>> {
        self.calls.borrow_mut().push((filter.into(), branch.into()));
        let raw = self
            .rows
            .get(&(filter.into(), branch.into()))
            .ok_or_else(|| Error::user(format!("unexpected query {filter} {branch}")))?;
        parse_pull_requests(raw, &self.forge, filter, branch)
    }
}
#[test]
fn both_forges_resolve_full_ordered_chain_and_validate_pin() {
    for forge in ["gh", "origin"] {
        let fixture = Fixture::new(forge);
        let frontier = resolve_with(&fixture, forge).unwrap();
        assert_eq!(
            frontier.iter().map(|r| r.pr).collect::<Vec<_>>(),
            [10, 13, 11, 14]
        );
        assert_eq!(
            frontier
                .iter()
                .map(|r| r.state.as_str())
                .collect::<Vec<_>>(),
            ["MERGED", "CLOSED", "OPEN", "OPEN"]
        );
        assert_eq!(
            frontier
                .iter()
                .map(|r| r.branches.as_str())
                .collect::<Vec<_>>(),
            ["stack/merged", "stack/closed", "stack/open", "stack/draft"]
        );
        assert!(frontier.iter().all(|r| r.sha.len() == 40));
        validate_pin(&[10, 13, 11, 14], &[10, 13, 11, 14]).unwrap();
        assert_eq!(validate_pin(&[10, 13, 11, 14], &[10, 11, 12]).unwrap_err().message, "frontier pin mismatch: missing from resolved chain: 12; extra in resolved chain: 13,14");
        assert_eq!(
            validate_pin(&[10, 13, 11, 14], &[13, 10, 11, 14])
                .unwrap_err()
                .message,
            "frontier pin mismatch: order differs: expected 13,10,11,14; resolved 10,13,11,14"
        );
    }
}
#[test]
fn replacement_active_pr_is_preferred_to_closed_or_merged_history() {
    for forge in ["gh", "origin"] {
        for history in ["closed", "merged"] {
            let mut fixture = Fixture::new(forge);
            fixture.put(
                "--head",
                "stack/open",
                vec![
                    (9, history, "stack/open", "main"),
                    (11, "open", "stack/open", "stack/closed"),
                ],
            );
            assert_eq!(
                resolve_with(&fixture, forge)
                    .unwrap()
                    .iter()
                    .map(|r| r.pr)
                    .collect::<Vec<_>>(),
                [10, 13, 11, 14]
            );
        }
    }
}
#[test]
fn ambiguous_active_and_historical_prs_are_rejected() {
    for forge in ["gh", "origin"] {
        for states in [["open", "open"], ["closed", "merged"]] {
            let mut fixture = Fixture::new(forge);
            fixture.put(
                "--head",
                "stack/open",
                vec![
                    (11, states[0], "stack/open", "stack/closed"),
                    (12, states[1], "stack/open", "main"),
                ],
            );
            assert!(resolve_with(&fixture, forge)
                .unwrap_err()
                .message
                .contains("multiple pull requests (#11, #12)"));
        }
    }
}
#[test]
fn malformed_forge_output_is_never_treated_as_an_empty_stack() {
    for forge in ["gh", "origin"] {
        for (raw, suffix) in [
            ("this line is not forge output", "output is not valid JSON"),
            ("{}", "output is not a JSON array"),
            ("[{}]", "output has an invalid PR row"),
        ] {
            let mut fixture = Fixture::new(forge);
            fixture
                .rows
                .insert(("--head".into(), "stack/open".into()), raw.into());
            assert_eq!(
                resolve_with(&fixture, forge).unwrap_err().message,
                format!("{forge} pr list --head stack/open {suffix}")
            );
        }
        let mut fixture = Fixture::new(forge);
        fixture.put("--head", "stack/open", vec![]);
        assert!(resolve_with(&fixture, forge)
            .unwrap_err()
            .message
            .contains(&format!(
                "branch stack/open has no pull request; open one with {forge} pr create"
            )));
    }
}
#[test]
fn forked_and_cyclic_chains_and_duplicate_prs_fail() {
    for forge in ["gh", "origin"] {
        let mut fixture = Fixture::new(forge);
        fixture.put(
            "--base",
            "stack/open",
            vec![
                (14, "open", "one", "stack/open"),
                (15, "open", "two", "stack/open"),
            ],
        );
        assert_eq!(
            resolve_with(&fixture, forge).unwrap_err().message,
            "stack is not linear: branches one, two all target stack/open"
        );
        let mut fixture = Fixture::new(forge);
        fixture.put(
            "--head",
            "stack/closed",
            vec![(13, "closed", "stack/closed", "stack/open")],
        );
        assert_eq!(
            resolve_with(&fixture, forge).unwrap_err().message,
            "pull request base chain loops at branch stack/open"
        );
        let mut fixture = Fixture::new(forge);
        fixture.put(
            "--base",
            "stack/open",
            vec![(14, "open", "stack/closed", "stack/open")],
        );
        assert_eq!(
            resolve_with(&fixture, forge).unwrap_err().message,
            "pull request base chain loops at branch stack/closed"
        );
        let mut fixture = Fixture::new(forge);
        fixture.put(
            "--head",
            "stack/closed",
            vec![(11, "closed", "stack/closed", "stack/merged")],
        );
        assert_eq!(
            resolve_with(&fixture, forge).unwrap_err().message,
            "pull request base chain contains duplicate pull requests"
        );
    }
}
#[test]
fn trunk_and_invalid_branch_sha_are_handled() {
    let mut fixture = Fixture::new("gh");
    fixture.current = "main".into();
    assert!(resolve_with(&fixture, "gh").unwrap().is_empty());
    assert!(fixture.calls.borrow().is_empty());
    fixture.current = "develop".into();
    fixture.remote_head = Some("origin/develop".into());
    assert!(resolve_with(&fixture, "gh").unwrap().is_empty());
    let mut fixture = Fixture::new("gh");
    fixture.invalid_sha = true;
    assert_eq!(
        resolve_with(&fixture, "gh").unwrap_err().message,
        "git rev-parse stack/merged returned an invalid SHA"
    );
}
#[test]
fn native_git_frontier_generation_persistence_pin_and_repo_environment() {
    let directory = TempDir::new().unwrap();
    let repo = directory.path().join("repo");
    fs::create_dir(&repo).unwrap();
    run_tool("git", &["init", "--initial-branch=main"], &repo).unwrap();
    let mut store = Store::open(directory.path().join("store"), false).unwrap();
    store.init().unwrap();
    let repo = repo.to_string_lossy();
    assert_eq!(
        store.frontier_set(&repo, "gh", None).unwrap(),
        Frontier {
            generation: 1,
            prs: vec![],
            lowest_unmerged: None
        }
    );
    assert_eq!(
        store
            .frontier_set(&repo, "origin", Some(&[]))
            .unwrap()
            .generation,
        2
    );
    assert_eq!(store.frontier_show().unwrap().generation, 2);
    assert!(store
        .frontier_set(&repo, "gh", Some(&[1]))
        .unwrap_err()
        .message
        .contains("missing from resolved chain: 1"));
    assert_eq!(store.frontier_show().unwrap().generation, 2);
    assert_eq!(
        store
            .frontier_set(&repo, "gh", Some(&[10, 10]))
            .unwrap_err()
            .message,
        "--prs must not contain duplicates"
    );
    assert_eq!(store.frontier_show().unwrap().generation, 2);
    store.close().unwrap();
    let args = ["frontier", "set", "--json"].map(str::to_string);
    let output = super::super::cli::run(
        &args,
        Some(
            directory
                .path()
                .join("store")
                .to_string_lossy()
                .into_owned(),
        ),
        Some(repo.into_owned()),
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(
        serde_json::from_str::<Value>(&output.stdout).unwrap(),
        json!({"generation":3,"prs":[],"lowestUnmerged":null})
    );
    let raw = fs::read_to_string(directory.path().join("store/frontier.json")).unwrap();
    assert!(raw.starts_with("{\n  \"generation\": 3,"));
    assert!(raw.ends_with('\n'));
}
#[test]
fn frontier_shape_accepts_legacy_empty_object_and_safe_integral_numbers() {
    assert_eq!(parse_frontier("{}").unwrap(), Frontier::default());
    assert_eq!(
        parse_frontier("{\"generation\":1.0,\"prs\":[],\"lowestUnmerged\":-1.0}").unwrap(),
        Frontier {
            generation: 1,
            prs: vec![],
            lowest_unmerged: Some(-1)
        }
    );
    for raw in [
        "{\"generation\":-1,\"prs\":[],\"lowestUnmerged\":null}",
        "{\"generation\":9007199254740992,\"prs\":[],\"lowestUnmerged\":null}",
        "{\"generation\":0,\"prs\":[],\"lowestUnmerged\":0.5}",
    ] {
        assert_eq!(
            parse_frontier(raw).unwrap_err().message,
            "frontier.json has an invalid shape"
        );
    }
}
