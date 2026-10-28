use super::types::*;
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use url::Url;

pub const REVIEW_THREADS_QUERY: &str = "\nquery ReviewThreads($owner: String!, $repo: String!, $pr: Int!, $after: String) {\n  repository(owner: $owner, name: $repo) {\n    pullRequest(number: $pr) {\n      reviewThreads(first: 100, after: $after) {\n        pageInfo {\n          hasNextPage\n          endCursor\n        }\n        nodes {\n          id\n          isResolved\n          comments(first: 10) {\n            nodes {\n              body\n              createdAt\n              path\n              line\n              author { login }\n            }\n          }\n        }\n      }\n    }\n  }\n}\n";
pub const PR_COMMIT_STATUS_QUERY: &str = "\nquery PrCommitStatuses($owner: String!, $repo: String!, $pr: Int!) {\n  repository(owner: $owner, name: $repo) {\n    pullRequest(number: $pr) {\n      commits(last: 50) {\n        nodes {\n          commit {\n            oid\n            statusCheckRollup {\n              state\n            }\n          }\n        }\n      }\n    }\n  }\n}\n";
pub const PR_CHECK_ROLLUP_QUERY: &str = "\nquery PrCheckRollup($owner: String!, $repo: String!, $pr: Int!, $after: String) {\n  repository(owner: $owner, name: $repo) {\n    pullRequest(number: $pr) {\n      commits(last: 1) {\n        nodes {\n          commit {\n            statusCheckRollup {\n              contexts(first: 100, after: $after) {\n                pageInfo {\n                  hasNextPage\n                  endCursor\n                }\n                nodes {\n                  __typename\n                  ... on CheckRun {\n                    name\n                    status\n                    conclusion\n                    detailsUrl\n                  }\n                  ... on StatusContext {\n                    context\n                    state\n                    targetUrl\n                  }\n                }\n              }\n            }\n          }\n        }\n      }\n    }\n  }\n}\n";

pub struct CommandResult {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}
pub trait Runner {
    fn run(&mut self, argv: &[String]) -> QueryResult<CommandResult>;
}
pub struct ProcessRunner;
impl Runner for ProcessRunner {
    fn run(&mut self, argv: &[String]) -> QueryResult<CommandResult> {
        let output = Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| {
                let mut failure = Failure::new("command-spawn", format!("{}: {e}", argv[0]));
                failure.retryable = false;
                failure
            })?;
        Ok(CommandResult {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}
pub fn first_line(s: &str) -> String {
    s.trim()
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(240)
        .collect()
}
fn missing(path: &str, value: Option<&Value>) -> Failure {
    let mut failure = Failure::new(
        "missing-key",
        match value {
            Some(v) => format!("invalid {path}: {v}"),
            None => format!("missing {path}"),
        },
    );
    failure.raw_value = value.map(Value::to_string);
    failure
}
fn at<'a>(value: &'a Value, path: &[&str]) -> QueryResult<&'a Value> {
    let label = path.join(".");
    let mut current = value;
    for key in path {
        if !current.is_object() {
            return Err(missing(&label, Some(current)));
        }
        current = current.get(*key).ok_or_else(|| missing(&label, None))?;
    }
    Ok(current)
}
fn field<'a>(v: &'a Value, key: &str, path: &str) -> QueryResult<&'a Value> {
    v.get(key).ok_or_else(|| missing(path, None))
}
fn text(v: &Value, path: &str) -> QueryResult<String> {
    v.as_str()
        .map(str::to_owned)
        .ok_or_else(|| missing(path, Some(v)))
}
fn string(v: &Value, key: &str, path: &str) -> QueryResult<String> {
    text(field(v, key, path)?, path)
}
fn optional_text(v: &Value, path: &str) -> QueryResult<Option<String>> {
    if v.is_null() {
        Ok(None)
    } else {
        text(v, path).map(Some)
    }
}
fn array<'a>(v: &'a Value, path: &str) -> QueryResult<&'a Vec<Value>> {
    v.as_array().ok_or_else(|| missing(path, Some(v)))
}
fn boolean(v: &Value, key: &str, path: &str) -> QueryResult<bool> {
    let value = field(v, key, path)?;
    value.as_bool().ok_or_else(|| missing(path, Some(value)))
}
fn enumeration(v: &Value, options: &[&str], path: &str) -> QueryResult<String> {
    if let Some(s) = v.as_str() {
        if options.contains(&s) {
            return Ok(s.into());
        }
    }
    Err(missing(path, Some(v)))
}
fn number(v: &Value, path: &str) -> QueryResult<u64> {
    let parsed = v.as_u64().or_else(|| {
        v.as_f64()
            .filter(|n| {
                n.is_finite() && n.fract() == 0.0 && *n > 0.0 && *n <= 9_007_199_254_740_991.0
            })
            .map(|n| n as u64)
    });
    parsed
        .filter(|n| *n > 0)
        .ok_or_else(|| missing(path, Some(v)))
}
fn nullable_enum(v: &Value, options: &[&str], path: &str) -> QueryResult<Option<String>> {
    if v.is_null() {
        Ok(None)
    } else {
        enumeration(v, options, path).map(Some)
    }
}
fn canonical_url(s: &str) -> Option<Url> {
    let url = Url::parse(s).ok()?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        None
    } else {
        Some(url)
    }
}
pub fn parse_remote(s: &str) -> Option<Repository> {
    let s = s.trim();
    let normalized = if let Some(rest) = s.strip_prefix("git@github.com:") {
        format!("https://github.com/{rest}")
    } else if let Some(rest) = s.strip_prefix("ssh://git@github.com/") {
        format!("https://github.com/{rest}")
    } else {
        s.into()
    };
    let url = canonical_url(&normalized)?;
    let path = url.path().strip_suffix(".git").unwrap_or(url.path());
    let parts: Vec<_> = path.split('/').filter(|x| !x.is_empty()).collect();
    if parts.len() != 2 {
        return None;
    }
    Some(Repository {
        owner: parts[0].into(),
        repo: parts[1].into(),
    })
}
pub fn parse_pr_url(s: &str) -> QueryResult<Context> {
    let parsed = (|| {
        let url = canonical_url(s)?;
        let parts: Vec<_> = url.path().split('/').filter(|x| !x.is_empty()).collect();
        if parts.len() != 4 || parts[2] != "pull" {
            return None;
        }
        let n = super::cli::parse_pr(parts[3]).ok()?;
        Some(Context {
            owner: parts[0].into(),
            repo: parts[1].into(),
            number: n,
        })
    })();
    parsed.ok_or_else(|| Failure {
        kind: "invalid-context-url".into(),
        retryable: false,
        detail: format!(
            "could not infer owner/repo from PR URL: {s} (not a canonical GitHub pull URL)"
        ),
        raw_value: Some(s.into()),
        code: None,
    })
}
fn details(v: &Value, key: &str) -> QueryResult<Check> {
    if !v.is_object() {
        return Err(missing("check", Some(v)));
    }
    Ok(Check {
        name: string(v, key, key)?,
        reported_state: String::new(),
        description: v["description"].as_str().unwrap_or("").into(),
        link: v["link"]
            .as_str()
            .or_else(|| v["detailsUrl"].as_str())
            .unwrap_or("")
            .into(),
        workflow: v["workflow"].as_str().unwrap_or("").into(),
        kind: CheckKind::Failed,
    })
}
fn gate_or(check: &mut Check, kind: CheckKind) {
    check.kind = if check.name == "Code Review Gate" {
        CheckKind::CodeReviewGate
    } else {
        kind
    };
}
pub fn parse_fast_check(v: &Value) -> QueryResult<Check> {
    let mut check = details(v, "name")?;
    let state = string(v, "state", "check.state")?.to_uppercase();
    let bucket = string(v, "bucket", "check.bucket")?;
    check.reported_state = state.clone();
    if state == "ACTION_REQUIRED" {
        gate_or(&mut check, CheckKind::Failed);
    } else if bucket == "fail" || ["FAILURE", "ERROR"].contains(&state.as_str()) {
        check.kind = CheckKind::Failed;
    } else if bucket == "pending" {
        gate_or(&mut check, CheckKind::Pending);
    } else if bucket == "pass" {
        check.kind = CheckKind::Passed;
    } else if bucket == "skipping" {
        check.kind = CheckKind::Skipped;
    }
    Ok(check)
}
pub fn map_rollup_node(v: &Value) -> QueryResult<Option<Check>> {
    if !v.is_object() {
        return Err(missing("rollup node", Some(v)));
    }
    let typename = v["__typename"].as_str().unwrap_or("");
    if typename != "CheckRun" && typename != "StatusContext" {
        return Ok(None);
    }
    let mut check = details(
        v,
        if typename == "CheckRun" {
            "name"
        } else {
            "context"
        },
    )?;
    if let Some(link) = v["targetUrl"].as_str() {
        check.link = link.into();
    }
    if typename == "CheckRun" {
        let status = v["status"].as_str().unwrap_or("").to_uppercase();
        let conclusion = v["conclusion"].as_str().unwrap_or("").to_uppercase();
        if status != "COMPLETED" {
            gate_or(&mut check, CheckKind::Pending);
            check.reported_state = "PENDING".into();
        } else {
            match conclusion.as_str() {
                "SUCCESS" => {
                    check.kind = CheckKind::Passed;
                    check.reported_state = conclusion;
                }
                "NEUTRAL" | "SKIPPED" => {
                    check.kind = CheckKind::Skipped;
                    check.reported_state = conclusion;
                }
                "ACTION_REQUIRED" => {
                    gate_or(&mut check, CheckKind::Failed);
                    check.reported_state = conclusion;
                }
                _ => {
                    check.kind = CheckKind::Failed;
                    check.reported_state = "FAILURE".into();
                }
            }
        }
    } else {
        let state = v["state"].as_str().unwrap_or("").to_uppercase();
        if state == "PENDING" || state == "EXPECTED" {
            gate_or(&mut check, CheckKind::Pending);
            check.reported_state = "PENDING".into();
        } else {
            check.kind = if state == "SUCCESS" {
                CheckKind::Passed
            } else {
                CheckKind::Failed
            };
            check.reported_state = if state.is_empty() {
                "FAILURE".into()
            } else {
                state
            };
        }
    }
    Ok(Some(check))
}
pub fn parse_pull_request(v: &Value, context: &Context) -> QueryResult<Facts> {
    if !v.is_object() {
        return Err(missing("pull request", Some(v)));
    }
    let is_draft = boolean(v, "isDraft", "pull request.isDraft")?;
    let review = field(v, "reviewDecision", "pull request.reviewDecision")?;
    Ok(Facts {
        context: context.clone(),
        mergeable: enumeration(
            field(v, "mergeable", "pull request.mergeable")?,
            &["MERGEABLE", "CONFLICTING", "UNKNOWN"],
            "pull request.mergeable",
        )?,
        merge_state_status: enumeration(
            field(v, "mergeStateStatus", "pull request.mergeStateStatus")?,
            &[
                "BEHIND",
                "BLOCKED",
                "CLEAN",
                "CONFLICTING",
                "DIRTY",
                "DRAFT",
                "HAS_HOOKS",
                "UNKNOWN",
                "UNSTABLE",
            ],
            "pull request.mergeStateStatus",
        )?,
        review_decision: if review == "" {
            None
        } else {
            nullable_enum(
                review,
                &["APPROVED", "CHANGES_REQUESTED", "REVIEW_REQUIRED"],
                "pull request.reviewDecision",
            )?
        },
        head_ref_oid: optional_text(
            field(v, "headRefOid", "pull request.headRefOid")?,
            "pull request.headRefOid",
        )?,
        head_ref_name: string(v, "headRefName", "pull request.headRefName")?,
        base_ref_name: string(v, "baseRefName", "pull request.baseRefName")?,
        state: enumeration(
            field(v, "state", "pull request.state")?,
            &["OPEN", "CLOSED", "MERGED"],
            "pull request.state",
        )?,
        merged_at: optional_text(
            field(v, "mergedAt", "pull request.mergedAt")?,
            "pull request.mergedAt",
        )?,
        is_draft,
    })
}
fn parse_comment(v: &Value) -> QueryResult<Comment> {
    let author = field(v, "author", "review comment.author")?;
    let line = field(v, "line", "review comment.line")?;
    Ok(Comment {
        author_login: if author.is_null() {
            None
        } else {
            optional_text(
                field(author, "login", "review comment.author.login")?,
                "review comment.author.login",
            )?
        },
        body: string(v, "body", "review comment.body")?,
        path: optional_text(
            field(v, "path", "review comment.path")?,
            "review comment.path",
        )?,
        line: if line.is_null() {
            None
        } else {
            Some(
                line.as_i64()
                    .or_else(|| line.as_f64().filter(|n| n.fract() == 0.0).map(|n| n as i64))
                    .ok_or_else(|| missing("review comment.line", Some(line)))?,
            )
        },
        created_at: string(v, "createdAt", "review comment.createdAt")?,
    })
}
fn cursor(v: &Value, path: &str) -> QueryResult<Option<String>> {
    let has_next = boolean(v, "hasNextPage", &format!("{path}.hasNextPage"))?;
    let cursor = optional_text(
        field(v, "endCursor", &format!("{path}.endCursor"))?,
        &format!("{path}.endCursor"),
    )?;
    Ok(cursor.filter(|s| has_next && !s.is_empty()))
}
pub fn parse_review_thread_page(v: &Value) -> QueryResult<ThreadPage> {
    let reviews = at(v, &["data", "repository", "pullRequest", "reviewThreads"])?;
    let mut threads = Vec::new();
    for thread in array(
        field(reviews, "nodes", "reviewThreads.nodes")?,
        "reviewThreads.nodes",
    )? {
        let resolved = boolean(thread, "isResolved", "review thread.isResolved")?;
        let comments = array(
            at(thread, &["comments", "nodes"])?,
            "review thread.comments.nodes",
        )?;
        threads.push(RawThread {
            id: string(thread, "id", "review thread.id")?,
            first_comment: comments.first().map(parse_comment).transpose()?,
            resolved,
        });
    }
    Ok(ThreadPage {
        threads,
        end_cursor: cursor(
            field(reviews, "pageInfo", "reviewThreads.pageInfo")?,
            "reviewThreads.pageInfo",
        )?,
    })
}
fn is_bugbot(comment: Option<&Comment>) -> bool {
    let Some(c) = comment else { return false };
    let author = c.author_login.as_deref().unwrap_or("").to_lowercase();
    let body = c.body.to_lowercase();
    author.contains("bugbot")
        || (author == "cursor"
            && [
                "bugbot",
                "cursor_automation_id",
                "agentic security review",
                "description start",
                "severity",
            ]
            .iter()
            .any(|s| body.contains(s)))
}
pub fn build_review_threads(threads: Vec<RawThread>) -> Vec<Thread> {
    static PATTERNS: LazyLock<[Regex; 2]> = LazyLock::new(|| {
        [
            Regex::new(r"RUN_ID:\s*([a-zA-Z0-9_.:-]+)").unwrap(),
            Regex::new(r"CURSOR_AUTOMATION_ID:\s*([a-zA-Z0-9_.:-]+)").unwrap(),
        ]
    });
    let patterns = &*PATTERNS;
    let mut keys = HashSet::new();
    let mut keyless = false;
    for t in &threads {
        if is_bugbot(t.first_comment.as_ref()) {
            let key = t.first_comment.as_ref().and_then(|c| {
                patterns
                    .iter()
                    .find_map(|r| r.captures(&c.body).map(|m| m[1].to_owned()))
            });
            if let Some(key) = key {
                keys.insert(key);
            } else {
                keyless = true;
            }
        }
    }
    let passes = if keys.is_empty() {
        usize::from(keyless)
    } else {
        keys.len()
    };
    threads
        .into_iter()
        .filter(|t| !t.resolved)
        .map(|t| Thread {
            is_bugbot: is_bugbot(t.first_comment.as_ref()),
            id: t.id,
            first_comment: t.first_comment,
            bugbot_review_passes: passes,
        })
        .collect()
}
fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).into()).collect()
}
fn graphql_args(query: &str, c: &Context) -> Vec<String> {
    vec![
        "gh".into(),
        "api".into(),
        "graphql".into(),
        "-f".into(),
        format!("query={query}"),
        "-f".into(),
        format!("owner={}", c.owner),
        "-f".into(),
        format!("repo={}", c.repo),
        "-F".into(),
        format!("pr={}", c.number),
    ]
}
pub struct GhReader<R = ProcessRunner> {
    pub runner: R,
}
impl<R: Runner> GhReader<R> {
    fn run_json(&mut self, argv: Vec<String>) -> QueryResult<Value> {
        let result = self.runner.run(&argv)?;
        if result.code != 0 {
            let mut failure = Failure::new(
                "command-exit",
                if first_line(&result.stderr).is_empty() {
                    format!("{} exited {}", argv.join(" "), result.code)
                } else {
                    first_line(&result.stderr)
                },
            );
            failure.code = Some(result.code);
            return Err(failure);
        }
        serde_json::from_str(&result.stdout)
            .map_err(|e| Failure::new("json-parse", format!("{}: {e}", argv.join(" "))))
    }
}
impl<R: Runner> Reader for GhReader<R> {
    fn origin_repo(&mut self) -> QueryResult<Option<Repository>> {
        let r = self
            .runner
            .run(&args(&["git", "remote", "get-url", "origin"]))?;
        Ok(if r.code == 0 {
            parse_remote(&r.stdout)
        } else {
            None
        })
    }
    fn current_pr(&mut self, pr: Option<u64>) -> QueryResult<Context> {
        let mut argv = args(&["gh", "pr", "view"]);
        if let Some(n) = pr {
            argv.push(n.to_string());
        }
        argv.extend(args(&["--json", "number,url"]));
        let v = self.run_json(argv)?;
        let mut c = parse_pr_url(&string(&v, "url", "current PR.url")?)?;
        c.number = match pr {
            Some(n) => n,
            None => number(
                field(&v, "number", "current PR.number")?,
                "current PR.number",
            )?,
        };
        Ok(c)
    }
    fn pull_request(&mut self, c: &Context) -> QueryResult<Facts> {
        let v=self.run_json(args(&["gh","pr","view",&c.number.to_string(),"--repo",&format!("{}/{}",c.owner,c.repo),"--json","mergeable,mergeStateStatus,reviewDecision,headRefOid,headRefName,baseRefName,state,mergedAt,isDraft"]))?;
        parse_pull_request(&v, c)
    }
    fn open_pull_requests(&mut self, r: &Repository) -> QueryResult<Vec<OpenPr>> {
        let v = self.run_json(args(&[
            "gh",
            "pr",
            "list",
            "--repo",
            &format!("{}/{}", r.owner, r.repo),
            "--state",
            "open",
            "--limit",
            "300",
            "--json",
            "number,headRefName,baseRefName,isCrossRepository",
        ]))?;
        array(&v, "open PRs")?
            .iter()
            .enumerate()
            .map(|(i, v)| {
                Ok(OpenPr {
                    number: number(
                        field(v, "number", &format!("open PRs[{i}].number"))?,
                        &format!("open PRs[{i}].number"),
                    )?,
                    is_cross_repository: boolean(
                        v,
                        "isCrossRepository",
                        &format!("open PRs[{i}].isCrossRepository"),
                    )?,
                    head_ref_name: string(v, "headRefName", &format!("open PRs[{i}].headRefName"))?,
                    base_ref_name: string(v, "baseRefName", &format!("open PRs[{i}].baseRefName"))?,
                })
            })
            .collect()
    }
    fn checks_fast_path(&mut self, c: &Context) -> QueryResult<FastChecks> {
        let r = self.runner.run(&args(&[
            "gh",
            "pr",
            "checks",
            &c.number.to_string(),
            "--repo",
            &format!("{}/{}", c.owner, c.repo),
            "--json",
            "name,state,description,link,workflow,bucket",
        ]))?;
        if [0, 1, 8].contains(&r.code) && !r.stdout.trim().is_empty() {
            if let Ok(Value::Array(values)) = serde_json::from_str::<Value>(&r.stdout) {
                if let Ok(checks) = values
                    .iter()
                    .map(parse_fast_check)
                    .collect::<QueryResult<Vec<_>>>()
                {
                    return Ok(FastChecks::Checks(checks));
                }
            }
        }
        Ok(FastChecks::Unusable {
            exit_code: r.code,
            stderr: r.stderr,
        })
    }
    fn check_rollup_page(&mut self, c: &Context, after: Option<&str>) -> QueryResult<RollupPage> {
        let mut argv = graphql_args(PR_CHECK_ROLLUP_QUERY, c);
        if let Some(s) = after {
            argv.extend(["-f".into(), format!("after={s}")]);
        }
        let v = self.run_json(argv)?;
        let commits = array(
            at(
                &v,
                &["data", "repository", "pullRequest", "commits", "nodes"],
            )?,
            "commits.nodes",
        )?;
        let Some(last) = commits.last() else {
            return Ok(RollupPage {
                checks: vec![],
                end_cursor: None,
            });
        };
        let commit = at(last, &["commit"])?;
        let rollup = field(commit, "statusCheckRollup", "commit.statusCheckRollup")?;
        if rollup.is_null() {
            return Ok(RollupPage {
                checks: vec![],
                end_cursor: None,
            });
        }
        let contexts = at(commit, &["statusCheckRollup", "contexts"])?;
        let checks = array(
            field(contexts, "nodes", "contexts.nodes")?,
            "contexts.nodes",
        )?
        .iter()
        .map(map_rollup_node)
        .collect::<QueryResult<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();
        Ok(RollupPage {
            checks,
            end_cursor: cursor(
                field(contexts, "pageInfo", "contexts.pageInfo")?,
                "contexts.pageInfo",
            )?,
        })
    }
    fn review_thread_page(&mut self, c: &Context, after: Option<&str>) -> QueryResult<ThreadPage> {
        let mut argv = graphql_args(REVIEW_THREADS_QUERY, c);
        if let Some(s) = after {
            argv.extend(["-f".into(), format!("after={s}")]);
        }
        parse_review_thread_page(&self.run_json(argv)?)
    }
    fn commit_rollups(&mut self, c: &Context) -> QueryResult<Vec<CommitRollup>> {
        let v = self.run_json(graphql_args(PR_COMMIT_STATUS_QUERY, c))?;
        array(
            at(
                &v,
                &["data", "repository", "pullRequest", "commits", "nodes"],
            )?,
            "commits.nodes",
        )?
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let commit = at(v, &["commit"])?;
            let rollup = field(
                commit,
                "statusCheckRollup",
                &format!("commits[{i}].statusCheckRollup"),
            )?;
            Ok(CommitRollup {
                oid: string(commit, "oid", &format!("commits[{i}].oid"))?,
                state: if rollup.is_null() {
                    None
                } else {
                    nullable_enum(
                        at(rollup, &["state"])?,
                        &["ERROR", "EXPECTED", "FAILURE", "PENDING", "SUCCESS"],
                        &format!("commits[{i}].statusCheckRollup.state"),
                    )?
                },
            })
        })
        .collect()
    }
}
pub fn resolve_checks(reader: &mut dyn Reader, c: &Context) -> QueryResult<(String, Vec<Check>)> {
    let fast = reader.checks_fast_path(c)?;
    let fast = match fast {
        FastChecks::Checks(checks) if !checks.is_empty() => {
            return Ok(("gh-pr-checks".into(), checks))
        }
        other => other,
    };
    let mut checks = Vec::new();
    let mut after = None;
    loop {
        let page = reader.check_rollup_page(c, after.as_deref())?;
        checks.extend(page.checks);
        after = page.end_cursor;
        if after.is_none() {
            break;
        }
    }
    if !checks.is_empty() {
        return Ok(("graphql-rollup".into(), checks));
    }
    let suffix = match fast {
        FastChecks::Unusable { exit_code, stderr } => format!(
            "fast path exit={exit_code}; GraphQL rollup was empty{}",
            if first_line(&stderr).is_empty() {
                String::new()
            } else {
                format!("; {}", first_line(&stderr))
            }
        ),
        _ => "fast path and GraphQL rollup were empty".into(),
    };
    Err(Failure::new(
        "checks-unavailable",
        format!("could not read PR checks: {suffix}"),
    ))
}
pub fn resolve_review_threads(reader: &mut dyn Reader, c: &Context) -> QueryResult<Vec<Thread>> {
    let mut threads = Vec::new();
    let mut after = None;
    loop {
        let page = reader.review_thread_page(c, after.as_deref())?;
        threads.extend(page.threads);
        after = page.end_cursor;
        if after.is_none() {
            break;
        }
    }
    Ok(build_review_threads(threads))
}
pub fn resolve_context(
    reader: &mut dyn Reader,
    owner: Option<&str>,
    repo: Option<&str>,
    pr: Option<u64>,
) -> QueryResult<Context> {
    if let (Some(owner), Some(repo), Some(number)) = (owner, repo, pr) {
        return Ok(Context {
            owner: owner.into(),
            repo: repo.into(),
            number,
        });
    }
    if let Some(number) = pr {
        if let Some(origin) = reader.origin_repo()? {
            return Ok(Context {
                owner: owner.unwrap_or(&origin.owner).into(),
                repo: repo.unwrap_or(&origin.repo).into(),
                number,
            });
        }
    }
    let c = reader.current_pr(pr)?;
    Ok(Context {
        owner: owner.unwrap_or(&c.owner).into(),
        repo: repo.unwrap_or(&c.repo).into(),
        number: pr.unwrap_or(c.number),
    })
}
pub fn order_stack(c: &Context, open: &[OpenPr]) -> Vec<Context> {
    let by_number: HashMap<_, _> = open.iter().map(|p| (p.number, p)).collect();
    let by_head: HashMap<_, _> = open
        .iter()
        .filter(|p| !p.is_cross_repository)
        .map(|p| (p.head_ref_name.as_str(), p))
        .collect();
    let mut children: HashMap<&str, Vec<&OpenPr>> = HashMap::new();
    for p in open {
        children.entry(&p.base_ref_name).or_default().push(p);
    }
    for values in children.values_mut() {
        values.sort_by_key(|p| p.number);
    }
    let Some(start) = by_number.get(&c.number).copied() else {
        return vec![c.clone()];
    };
    let mut down = Vec::new();
    let mut current = start;
    let mut ancestry = HashSet::from([start.number]);
    while let Some(parent) = by_head.get(current.base_ref_name.as_str()).copied() {
        if !ancestry.insert(parent.number) {
            break;
        }
        down.push(parent);
        current = parent;
    }
    let mut seen: HashSet<_> = down
        .iter()
        .map(|p| p.number)
        .chain([start.number])
        .collect();
    fn visit<'a>(
        parent: &OpenPr,
        children: &HashMap<&str, Vec<&'a OpenPr>>,
        seen: &mut HashSet<u64>,
        up: &mut Vec<&'a OpenPr>,
    ) {
        if parent.is_cross_repository {
            return;
        }
        if let Some(values) = children.get(parent.head_ref_name.as_str()) {
            for child in values {
                if seen.insert(child.number) {
                    up.push(child);
                    visit(child, children, seen, up);
                }
            }
        }
    }
    let mut up = Vec::new();
    visit(start, &children, &mut seen, &mut up);
    down.into_iter()
        .rev()
        .chain([start])
        .chain(up)
        .map(|p| c.with_number(p.number))
        .collect()
}
pub fn discover_stack(reader: &mut dyn Reader, c: &Context) -> QueryResult<Vec<Context>> {
    Ok(order_stack(c, &reader.open_pull_requests(&c.repository())?))
}
