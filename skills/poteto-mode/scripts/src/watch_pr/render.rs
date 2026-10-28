use serde_json::Value;
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn n(v: &Value, key: &str) -> u64 {
    v[key].as_u64().unwrap_or(0)
}
fn count(v: &Value, key: &str) -> usize {
    v[key].as_array().map_or(0, Vec::len)
}
fn js(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        s.to_owned()
    } else if let Some(n) = v.as_f64() {
        n.to_string()
    } else {
        v.to_string()
    }
}
fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}
pub fn render_json(v: &Value) -> String {
    format!("{v}\n")
}
fn ci_cell(row: &Value) -> String {
    if row["kind"] != "open" {
        return "—".into();
    }
    let ci = &row["ci"];
    let was = if ci["hadPreviousPassingCi"] == true {
        ", was ✅"
    } else {
        ""
    };
    match s(ci, "kind") {
        "ci-clean" => "✅".into(),
        "ci-pending" => format!("⏳ {} pending{was}", count(ci, "pending")),
        "ci-failing" => format!(
            "❌ {} failed{}{was}",
            count(ci, "failed"),
            if count(ci, "pending") > 0 {
                format!(", {} pending", count(ci, "pending"))
            } else {
                String::new()
            }
        ),
        "ci-github-rejected" => format!("❌ GitHub reports failing checks{was}"),
        _ => unreachable!("unknown CI state"),
    }
}
fn review_cell(row: &Value) -> String {
    if row["kind"] != "open" {
        return "—".into();
    }
    let open = count(row, "threads");
    if row["reviewAutomationRunning"] == true {
        if open > 0 {
            format!("🤖 running, {open} open")
        } else {
            "🤖 running".into()
        }
    } else if open > 0 {
        format!("📝 {open} open")
    } else {
        "✅".into()
    }
}
fn merge_cell(row: &Value) -> &'static str {
    if row["kind"] == "merged" {
        return "✅ merged";
    }
    if row["kind"] == "closed" {
        return "❌ closed";
    }
    let facts = &row["facts"];
    if facts["isDraft"] == true {
        return "⏸ draft";
    }
    if facts["reviewDecision"] == "CHANGES_REQUESTED" {
        return "⚠️ changes requested";
    }
    if facts["mergeable"] == "CONFLICTING"
        || facts["mergeStateStatus"] == "DIRTY"
        || facts["mergeStateStatus"] == "CONFLICTING"
    {
        "⚠️ conflict"
    } else {
        "✅"
    }
}
pub fn render_status_table(rows: &Value) -> String {
    let mut out = String::from("| PR | CI | Review | Merge |\n| --- | --- | --- | --- |\n");
    for row in rows.as_array().expect("status rows must be an array") {
        let c = &row["context"];
        let number = n(c, "number");
        out.push_str(&format!(
            "| [#{number}](https://github.com/{}/{}/pull/{number}) | {} | {} | {} |\n",
            s(c, "owner"),
            s(c, "repo"),
            ci_cell(row),
            review_cell(row),
            merge_cell(row)
        ));
    }
    out
}
fn thread_line(thread: &Value) -> String {
    let c = &thread["firstComment"];
    let nullable = |key: &str| {
        if c[key].is_null() {
            "None".into()
        } else {
            js(&c[key])
        }
    };
    let body = s(c, "body")
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(180)
        .collect::<String>();
    format!(
        "{} {} {} {} isBugBot={} bugbotReviewPasses={} {body}",
        s(thread, "id"),
        nullable("path"),
        nullable("line"),
        nullable("authorLogin"),
        thread["isBugbot"],
        thread["bugbotReviewPasses"]
    )
}
fn render_blocker(b: &Value) -> String {
    let pr = n(&b["pr"], "number");
    match s(b,"kind") {
        "merge-conflicts"=>format!("BLOCKER: merge-conflicts\npr={pr}\nmergeable={}\nmergeStateStatus={}\naction=resolve merge conflicts before waiting for CI",s(&b["facts"],"mergeable"),s(&b["facts"],"mergeStateStatus")),
        "review-threads"=>{
            let mut lines=vec!["BLOCKER: review-threads".into(),format!("pr={pr}"),format!("unresolved={}",count(b,"threads"))];
            lines.extend(b["threads"].as_array().unwrap().iter().map(thread_line));lines.join("\n")
        },
        "failing-checks"=>{
            let ci=&b["ci"];let failed=ci["failed"].as_array().unwrap();
            let mut lines=vec!["BLOCKER: failing-checks".into(),format!("pr={pr}"),format!("failed={}",failed.len())];
            for check in failed {lines.push(format!("{} {} {} {}",s(check,"name"),s(check,"reportedState"),s(check,"description"),s(check,"link")));}
            if ci["kind"]=="ci-github-rejected" {lines.push(format!("mergeStateStatus={}",js(&ci["github"]["mergeStateStatus"])));lines.push(format!("headRollupState={}",js(&ci["github"]["headRollupState"])));}
            lines.join("\n")
        },
        "merge-gate"=>{
            let reason=s(b,"reason");let action=match reason {"closed-without-merge"=>"restore or remove the closed PR from the queued stack","draft-pr"=>"mark the PR ready for review before waiting for the merge queue",_=>"resolve the changes-requested review before waiting for the merge queue"};
            format!("BLOCKER: {reason}\npr={pr}\naction={action}")
        },
        "status-query"=>format!("BLOCKER: status-query\nfailures={}\ndetail={}\naction=verify current PR context, GitHub authentication, and API availability, then rearm",n(b,"failures"),s(&b["failure"],"detail")),
        _=>unreachable!("unknown blocker"),
    }
}
pub fn render_pretty(v: &Value) -> String {
    match s(v, "kind") {
        "QUEUE" => format!(
            "QUEUE: captured {} PR{} bottom-to-top: {}\n",
            count(v, "queue"),
            plural(count(v, "queue")),
            v["queue"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| format!("#{}", n(c, "number")))
                .collect::<Vec<_>>()
                .join(",")
        ),
        "STATUS" => render_status_table(&v["rows"]),
        "WAITING" => {
            let reason = &v["reason"];
            let frontier = n(&v["frontier"], "number");
            if reason["kind"] == "pending-checks" {
                format!(
                    "WAITING: frontier=#{frontier}; {} check{} pending\n",
                    count(reason, "pending"),
                    plural(count(reason, "pending"))
                )
            } else {
                format!("WAITING: frontier=#{frontier} is blocker-free; waiting for merge queue ({} PR{} unmerged)\n",n(reason,"unmergedCount"),plural(n(reason,"unmergedCount") as usize))
            }
        }
        "ADVANCE" => format!(
            "ADVANCE: merged #{}; next=#{}; remaining={}\n",
            n(&v["merged"], "number"),
            n(&v["frontier"], "number"),
            n(v, "remaining")
        ),
        "RETRY" => format!(
            "RETRY: GitHub status query failed; retrying in {}s\ndetail={}\n",
            js(&v["retryInSeconds"]),
            s(&v["failure"], "detail")
        ),
        "BLOCKER" => format!("{}\n", render_blocker(&v["blocker"])),
        "READY" => {
            let scope = &v["scope"];
            let pr = &scope["pr"];
            let detail = if scope["kind"] == "single" && pr["kind"] == "ready-pr" {
                let draft = pr["proof"]["gate"]["draft"] == "draft-allowed";
                format!(
                    "\nmergeStateStatus={}\nreviewDecision={}\nisDraft={draft}{}",
                    js(&pr["proof"]["ci"]["github"]["mergeStateStatus"]),
                    js(&pr["proof"]["gate"]["reviewDecision"]),
                    if draft {
                        "\nnote=draft allowed (--allow-draft); leave draft — do not mark ready"
                    } else {
                        ""
                    }
                )
            } else {
                String::new()
            };
            format!("READY: no merge conflicts, no unresolved review threads, no failing or pending checks{detail}\n")
        }
        "COMPLETE" => format!(
            "COMPLETE: queued stack merged ({} PR{})\n",
            count(v, "queue"),
            plural(count(v, "queue"))
        ),
        "TIMEOUT" => match s(&v["reason"], "kind") {
            "pending-checks" => "TIMEOUT: checks still pending\n".into(),
            "status-unavailable" => "TIMEOUT: GitHub status remained unavailable\n".into(),
            _ => format!(
                "TIMEOUT: queued stack still has {} PR{} unmerged; frontier=#{}\n",
                n(&v["reason"], "unmergedCount"),
                plural(n(&v["reason"], "unmergedCount") as usize),
                n(&v["reason"]["frontier"], "number")
            ),
        },
        _ => unreachable!("unknown watcher verdict"),
    }
}
