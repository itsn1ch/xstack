//! check-tickets <epic-dir>: the folder holding spec.md and issues/. Exit 0 when the chain is continuous.
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: check-tickets <epic-dir>");
        return ExitCode::from(2);
    };
    let report = check_tickets::check_epic(std::path::Path::new(&dir));
    if report.errors.is_empty() {
        println!("OK {} ticket(s) form one continuous chain from spec.md", report.tickets);
        return ExitCode::SUCCESS;
    }
    for error in &report.errors {
        eprintln!("FAIL {error}");
    }
    eprintln!("{} continuity error(s) in {} ticket(s)", report.errors.len(), report.tickets);
    ExitCode::from(1)
}
