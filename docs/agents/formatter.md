# Code formatters

Run from the project root on changed files only. Split mixed file lists by language; do not pass Rust or shell files to the other tool. Setup never reformats existing files. Generated files, lockfiles, images, and license texts are excluded. Mermaid `.mmd` fixtures are diagram data; no standalone formatter is configured. Embedded code samples and fixture strings are not independent source files.

Install JavaScript tools with `bun install --frozen-lockfile` in fresh worktrees. Never replace the pinned local binary with bare npx/bunx. Toolchain and user-local tools must already be installed. No formatting commit hook is configured.

## Rust

- Formatter: rustfmt 1.10.0
- Source: Rust toolchain; project Cargo packages use edition 2021
- Config: none; formatter defaults with explicit edition 2021
- Binary: `rustfmt`
- Format files: `rustfmt --edition 2021 <files>`
- Check files: `rustfmt --edition 2021 --check <files>`
- Applies to: *.rs
- Human command: `make format-rust FILES="<Rust files>"`
- Commit hook: none

## JavaScript

- Formatter: Prettier 3.9.10
- Source: package.json devDependency and format script
- Config: none; formatter defaults
- Binary: `node_modules/.bin/prettier`
- Format files: `node_modules/.bin/prettier --write --ignore-unknown <files>`
- Check files: `node_modules/.bin/prettier --check --ignore-unknown <files>`
- Applies to: *.mjs, *.js
- Human command: `bun run format -- <files>`
- Commit hook: none

## Markdown

- Formatter: Prettier 3.9.10
- Source: package.json devDependency and format script
- Config: none; formatter defaults
- Binary: `node_modules/.bin/prettier`
- Format files: `node_modules/.bin/prettier --write --ignore-unknown <files>`
- Check files: `node_modules/.bin/prettier --check --ignore-unknown <files>`
- Applies to: *.md
- Human command: `bun run format -- <files>`
- Commit hook: none

## JSON

- Formatter: Prettier 3.9.10
- Source: package.json devDependency and format script
- Config: none; formatter defaults
- Binary: `node_modules/.bin/prettier`
- Format files: `node_modules/.bin/prettier --write --ignore-unknown <files>`
- Check files: `node_modules/.bin/prettier --check --ignore-unknown <files>`
- Applies to: *.json
- Human command: `bun run format -- <files>`
- Commit hook: none

## YAML

- Formatter: Prettier 3.9.10
- Source: package.json devDependency and format script
- Config: none; formatter defaults
- Binary: `node_modules/.bin/prettier`
- Format files: `node_modules/.bin/prettier --write --ignore-unknown <files>`
- Check files: `node_modules/.bin/prettier --check --ignore-unknown <files>`
- Applies to: *.yaml, *.yml
- Human command: `bun run format -- <files>`
- Commit hook: none

## Shell (minor)

- Formatter: shfmt v3.14.1
- Source: Approved user-local Go tool install
- Config: none; formatter defaults
- Binary: `$HOME/.local/bin/shfmt`
- Format files: `$HOME/.local/bin/shfmt -w <files>`
- Check files: `$HOME/.local/bin/shfmt -d <files>`
- Applies to: *.sh
- Human command: `make format-shell FILES="<shell files>"`
- Commit hook: none

## TOML

- Formatter: Taplo 0.10.0
- Source: Approved cargo install taplo-cli --version 0.10.0 --locked
- Config: none; formatter defaults
- Binary: `$HOME/.cargo/bin/taplo`
- Format files: `$HOME/.cargo/bin/taplo format <files>`
- Check files: `$HOME/.cargo/bin/taplo format --check <files>`
- Applies to: *.toml (not generated Cargo.lock)
- Human command: `make format-toml FILES="<TOML files>"`
- Commit hook: none

## Makefile (minor)

- Formatter: none, declined
- Source: explicit setup choice
- Config: none
- Binary: none
- Format files: none
- Check files: none
- Applies to: Makefile
- Commit hook: none

## Reinstalling user tools

- Shell: `GOBIN="$HOME/.local/bin" go install mvdan.cc/sh/v3/cmd/shfmt@v3.14.1`.
- TOML: `cargo install taplo-cli --version 0.10.0 --locked`.
- Rust: provided by the installed Rust toolchain; if absent under rustup, use `rustup component add rustfmt`.
