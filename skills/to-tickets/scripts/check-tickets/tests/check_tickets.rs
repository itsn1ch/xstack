use check_tickets::check_epic;
use regex::Regex;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

const SPEC: &str = r#"# Checkout
Status: ready-for-agent

## User Stories

1. **US-001: Pay for an order**
   - **US-001-AC1:** Given a pending order, when the customer pays, then it is paid.
   - **US-001-AC2:** Given a paid order, when the customer pays again, then nothing is charged.
2. **US-002: Ship a paid order**
   - **US-002-AC1:** Given a paid order, when it ships, then it is shipped.
   - **US-002-AC9:** (removed) replaced by US-002-AC1.

# Design

## Contract

### `checkout(cartId: CartId): Order`
### `payOrder(orderId: OrderId, payment: Payment): Result<Order, PayError>`
### `shipOrder(orderId: OrderId): Order`

| `INV-1` | An Order has at most one OrderPaid. |
"#;

const OVERVIEW: &str = r#"# 00: Overview

**Status:** reference
**Contract:** `.scratch/checkout/spec.md`, section `# Design`.

## Files

```
src/
├── orders/
│   ├── model.ts          [01, 02] Order and OrderStatus
│   └── pay-order.ts      [02] payOrder
└── shipping/
    └── ship-order.ts     [03] shipOrder
```

## Architecture

cart → orders (01) → payment (02) → shipping (03)

## Order

checkout flow: 01 → 02 → 03

## In plain words

First we turn a cart into an order. Then the customer can pay for it once. Then it ships.
"#;

struct T<'a> {
    n: &'a str,
    title: &'a str,
    blocked_by: &'a str,
    source: &'a str,
    types: &'a str,
    function: &'a str,
    where_: &'a str,
    inv: &'a str,
    flow: &'a str,
}

fn ticket(t: T) -> String {
    format!(
        "# {n}: {title}\n\n**Status:** ready-for-agent\n**Blocked by:** {blocked}\n**Source:** {source}\n**Contract:** `.scratch/checkout/spec.md`, section `# Design`.\n\n\
## Goal\n\n{title}.\n\n## Types\n\n```ts\n{types}\n```\n\n## Functions\n\n### `{function}`\n\n- **Intent.** {title}.\n- **Where.** `{where_}`\n\n\
## Invariants\n\n| ID | Statement | Role | Proof |\n|---|---|---|---|\n{inv}\n\n## Flow\n\n{flow}\n\n## Steps\n\n1. Red then green per criterion.\n\n\
## Acceptance criteria\n\n- [ ] {source}\n\n## Verify\n\n`bun test`\n\n## Do not touch\n\nOther tickets' functions.\n",
        n = t.n,
        title = t.title,
        blocked = t.blocked_by,
        source = t.source,
        types = t.types,
        function = t.function,
        where_ = t.where_,
        inv = t.inv,
        flow = t.flow,
    )
}

type Files = BTreeMap<String, Option<String>>;

fn files() -> Files {
    let tickets = [
        (
            "01-checkout.md",
            T {
                n: "01",
                title: "Create an order from a cart",
                blocked_by: "None (can start immediately)",
                source: "US-001-AC1",
                types: "type Order = { id: OrderId; status: OrderStatus }",
                function: "checkout(cartId: CartId): Order",
                where_: "src/orders/model.ts",
                inv: "| `INV-1` | at most one OrderPaid | establish | concurrent pay test |",
                flow: "- In: `cartId` ← external\n- Out: `Order` → 02",
            },
        ),
        (
            "02-pay-order.md",
            T {
                n: "02",
                title: "Pay for an order",
                blocked_by: "01",
                source: "US-001-AC1, US-001-AC2",
                types: "Order // from 01\ntype PayError = 'AlreadyPaid'",
                function: "payOrder(orderId: OrderId, payment: Payment): Result<Order, PayError>",
                where_: "src/orders/pay-order.ts",
                inv: "| `INV-1` | at most one OrderPaid | preserve | concurrent pay test |",
                flow: "- In: `Order` ← 01\n- Out: `OrderPaid` → 03",
            },
        ),
        (
            "03-ship-order.md",
            T {
                n: "03",
                title: "Ship a paid order",
                blocked_by: "02",
                source: "US-002-AC1",
                types: "Order // from 01",
                function: "shipOrder(orderId: OrderId): Order",
                where_: "src/shipping/",
                inv: "| `INV-1` | at most one OrderPaid | preserve | concurrent pay test |",
                flow: "- In: `OrderPaid` ← 02\n- Out: `Order` → final",
            },
        ),
    ];
    let mut files = Files::new();
    files.insert("spec.md".into(), Some(SPEC.into()));
    files.insert("issues/00-overview.md".into(), Some(OVERVIEW.into()));
    for (name, t) in tickets {
        files.insert(format!("issues/{name}"), Some(ticket(t)));
    }
    files
}

fn epic(files: &Files) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("check-tickets-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("issues")).unwrap();
    for (rel, text) in files {
        if let Some(text) = text {
            fs::write(dir.join(rel), text).unwrap();
        }
    }
    dir
}

fn errors(files: &Files) -> Vec<String> {
    let dir = epic(files);
    let report = check_epic(&dir);
    fs::remove_dir_all(&dir).unwrap();
    report.errors
}

fn edit(mut files: Files, rel: &str, from: &str, to: &str) -> Files {
    let text = files[rel].clone().unwrap();
    assert!(text.contains(from), "fixture {rel} lacks {from}");
    files.insert(rel.into(), Some(text.replace(from, to)));
    files
}

fn expect_error(files: Files, fragment: &str) {
    let errors = errors(&files);
    assert!(errors.iter().any(|e| e.contains(fragment)), "expected an error containing \"{fragment}\", got:\n{}", errors.join("\n"));
}

#[test]
fn continuous_epic_passes() {
    assert_eq!(errors(&files()), Vec::<String>::new());
}

#[test]
fn spec_criterion_no_ticket_covers_is_a_gap() {
    expect_error(edit(files(), "issues/03-ship-order.md", "US-002-AC1", "US-001-AC1"), "US-002-AC1 is not delivered or guarded by any ticket");
}

#[test]
fn removed_criterion_needs_no_ticket() {
    let errors = errors(&files());
    assert!(!errors.iter().any(|e| e.contains("US-002-AC9")), "{}", errors.join("\n"));
}

#[test]
fn undefined_criterion_fails() {
    expect_error(edit(files(), "issues/02-pay-order.md", "US-001-AC2", "US-001-AC7"), "US-001-AC7 is not defined in spec.md");
}

#[test]
fn number_gap_breaks_the_chain() {
    let mut f = files();
    let t = f.remove("issues/03-ship-order.md").unwrap().unwrap();
    f.insert("issues/04-ship-order.md".into(), Some(t.replacen("# 03:", "# 04:", 1)));
    expect_error(f, "expected ticket 03");
}

#[test]
fn blocker_not_earlier_fails() {
    expect_error(edit(files(), "issues/02-pay-order.md", "**Blocked by:** 01", "**Blocked by:** 03"), "blocked by 03, which is not earlier");
}

#[test]
fn input_without_matching_output_fails() {
    expect_error(
        edit(files(), "issues/02-pay-order.md", "- Out: `OrderPaid` → 03", "- Out: `Receipt` → 03"),
        "that ticket has no matching \"Out: `OrderPaid` → 03\"",
    );
}

#[test]
fn output_nobody_consumes_fails() {
    expect_error(
        edit(files(), "issues/03-ship-order.md", "- In: `OrderPaid` ← 02", "- In: `OrderPaid` ← existing"),
        "that ticket has no matching \"In: `OrderPaid` ← 02\"",
    );
}

#[test]
fn input_outside_blocker_chain_fails() {
    expect_error(edit(files(), "issues/03-ship-order.md", "**Blocked by:** 02", "**Blocked by:** None"), "which does not block it");
}

#[test]
fn borrowed_type_not_defined_fails() {
    expect_error(edit(files(), "issues/03-ship-order.md", "Order // from 01", "PayError // from 01"), "type PayError is not defined in 01-checkout.md");
}

#[test]
fn function_owned_twice_fails() {
    expect_error(edit(files(), "issues/03-ship-order.md", "### `shipOrder(", "### `payOrder("), "function payOrder is already owned by 02-pay-order.md");
}

#[test]
fn function_missing_from_design_fails() {
    expect_error(
        edit(files(), "issues/03-ship-order.md", "### `shipOrder(", "### `dispatchOrder("),
        "function dispatchOrder is not in the spec's # Design contract",
    );
}

#[test]
fn preserve_before_establish_fails() {
    let f = edit(files(), "issues/01-checkout.md", "| establish |", "| preserve |");
    let f = edit(f, "issues/02-pay-order.md", "| preserve |", "| establish |");
    expect_error(f, "preserves INV-1 before 02-pay-order.md establishes it");
}

#[test]
fn no_flow_reaching_user_fails() {
    expect_error(edit(files(), "issues/03-ship-order.md", "- Out: `Order` → final", ""), "no flow reaches the user");
}

#[test]
fn missing_section_fails() {
    expect_error(edit(files(), "issues/01-checkout.md", "## Do not touch", "## Notes"), "missing \"## Do not touch\" section");
}

#[test]
fn contract_na_ticket_skips_contract_sections() {
    let docs = "# 01: Update the docs\n\n**Status:** ready-for-agent\n**Blocked by:** None\n**Source:** docs only\n**Contract:** contract: n/a, docs only\n\n\
## Goal\nDocs.\n## Steps\n1. Edit.\n## Acceptance criteria\n- [ ] Docs updated.\n## Verify\n`true`\n## Do not touch\nCode.\n";
    let start = OVERVIEW.find("```\n").unwrap();
    let end = start + 4 + OVERVIEW[start + 4..].find("```").unwrap() + 3;
    let overview = format!("{}```\nREADME.md  [01] usage docs\n```{}", &OVERVIEW[..start], &OVERVIEW[end..]).replace("01 → 02 → 03", "01");
    let mut f = Files::new();
    f.insert("spec.md".into(), Some("# Docs\nStatus: ready-for-agent\n".into()));
    f.insert("issues/00-overview.md".into(), Some(overview));
    f.insert("issues/01-update-docs.md".into(), Some(docs.into()));
    assert_eq!(errors(&f), Vec::<String>::new());
}

#[test]
fn without_spec_criteria_tickets_need_not_cite_ids() {
    let design_only = format!("# Checkout\nStatus: ready-for-agent\n\n{}", &SPEC[SPEC.find("# Design").unwrap()..]);
    let id = Regex::new(r"US-\d+-AC\d+(, )?").unwrap();
    let mut f = files();
    f.insert("spec.md".into(), Some(design_only));
    for (rel, text) in f.iter_mut() {
        if rel.starts_with("issues/") {
            *text = text.as_deref().map(|t| id.replace_all(t, "").into_owned());
        }
    }
    assert_eq!(errors(&f), Vec::<String>::new());
}

#[test]
fn spec_criteria_ticket_citing_none_fails() {
    expect_error(edit(files(), "issues/03-ship-order.md", "US-002-AC1", "shipping"), "names no spec criterion ID");
}

#[test]
fn missing_overview_fails() {
    let mut f = files();
    f.insert("issues/00-overview.md".into(), None);
    expect_error(f, "00-overview.md: missing");
}

#[test]
fn overview_naming_nonexistent_ticket_fails() {
    expect_error(edit(files(), "issues/00-overview.md", "[03] shipOrder", "[03, 07] shipOrder"), "names ticket 07, which does not exist");
}

#[test]
fn ticket_touching_no_file_fails() {
    expect_error(edit(files(), "issues/00-overview.md", "[03] shipOrder", "shipOrder"), "ticket 03 touches no file in Files");
}

#[test]
fn where_missing_from_tree_fails() {
    expect_error(edit(files(), "issues/02-pay-order.md", "src/orders/pay-order.ts", "src/billing/pay.ts"), "is not in 00-overview.md's Files tree");
}

#[test]
fn where_without_ticket_number_fails() {
    expect_error(edit(files(), "issues/00-overview.md", "[02] payOrder", "[01] payOrder"), "pay-order.ts` is in 00-overview.md's Files tree without [02]");
}

#[test]
fn folder_where_matches_files_under_it() {
    let errors = errors(&files());
    assert!(!errors.iter().any(|e| e.contains("src/shipping")), "{}", errors.join("\n"));
}

#[test]
fn order_missing_ticket_fails() {
    expect_error(edit(files(), "issues/00-overview.md", "01 → 02 → 03", "01 → 02"), "Order does not name ticket 03");
}

#[test]
fn overview_ready_for_agent_fails() {
    expect_error(edit(files(), "issues/00-overview.md", "**Status:** reference", "**Status:** ready-for-agent"), "never ready-for-agent");
}

#[test]
fn blocked_by_overview_fails() {
    expect_error(
        edit(files(), "issues/01-checkout.md", "**Blocked by:** None (can start immediately)", "**Blocked by:** 00"),
        "the overview is never a blocker",
    );
}

#[test]
fn empty_plain_words_fails() {
    expect_error(
        edit(files(), "issues/00-overview.md", "First we turn a cart into an order. Then the customer can pay for it once. Then it ships.", ""),
        "\"## In plain words\" is empty",
    );
}

#[test]
fn cli_reports_ok_and_fail() {
    let bin = env!("CARGO_BIN_EXE_check-tickets");
    let dir = epic(&files());
    let ok = Command::new(bin).arg(&dir).output().unwrap();
    assert_eq!(ok.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&ok.stdout).starts_with("OK 3 ticket(s)"));
    fs::remove_file(dir.join("issues/00-overview.md")).unwrap();
    let fail = Command::new(bin).arg(&dir).output().unwrap();
    assert_eq!(fail.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&fail.stderr).contains("FAIL issues/00-overview.md: missing"));
    fs::remove_dir_all(&dir).unwrap();
    assert_eq!(Command::new(bin).output().unwrap().status.code(), Some(2));
}
