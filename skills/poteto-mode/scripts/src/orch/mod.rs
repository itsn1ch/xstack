mod cli;
mod store;

pub fn main() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = cli::run(
        &args,
        std::env::var("ORCH_STORE").ok(),
        std::env::var("ORCH_REPO").ok(),
    );
    use std::io::Write;
    let _ = std::io::stdout().write_all(result.stdout.as_bytes());
    let _ = std::io::stderr().write_all(result.stderr.as_bytes());
    result.code
}

#[cfg(test)]
mod tests;
