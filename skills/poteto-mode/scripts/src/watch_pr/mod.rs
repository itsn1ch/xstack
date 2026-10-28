mod cli;
mod github;
mod policy;
mod render;
mod types;

pub fn main() -> i32 {
    cli::main()
}

#[cfg(test)]
mod tests;
