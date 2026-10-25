# Code formatter

Every language in the target project gets one code formatter, recorded in `docs/agents/formatter.md`. **poteto-mode** reads that file and must run the formatter on every file it changes before each commit. Setup detects what the project already uses, installs a standard formatter only where none exists and the user approves, and never reformats the existing codebase.

## 1. Detect languages

List the languages the project actually contains, from tracked source files (`git ls-files`), ignoring vendored, generated, and lockfile paths. A language with only a handful of incidental files (one helper script) still gets an entry, marked minor.

## 2. Detect the formatter per language

An existing project command wins over everything else, because it encodes the project's own flags and paths. Look in this order and stop at the first hit:

1. A format script or target: `format` / `fmt` / `lint:fix` in `package.json` scripts, a `Makefile` or `justfile` target, a `Taskfile`, or a `composer.json` script.
2. A pre-commit or CI hook that formats: `.pre-commit-config.yaml`, `lefthook.yml`, `.husky/`, `lint-staged` config, or a CI step running a formatter in check mode.
3. A formatter config file or dependency:

| Language | Formatter | Detect by |
|---|---|---|
| JavaScript, TypeScript, CSS, JSON, Markdown | Biome | `biome.json`, `biome.jsonc`, `@biomejs/biome` dependency |
| JavaScript, TypeScript, CSS, JSON, Markdown | Prettier | `.prettierrc*`, `prettier.config.*`, `"prettier"` key or dependency in `package.json` |
| JavaScript, TypeScript | dprint | `dprint.json`, `.dprint.json` |
| Python | Ruff formatter | `[tool.ruff]` in `pyproject.toml`, `ruff.toml`, `ruff` dependency |
| Python | Black | `[tool.black]` in `pyproject.toml`, `black` dependency |
| Go | gofmt or goimports | `go.mod` (gofmt ships with Go) |
| Rust | rustfmt | `Cargo.toml` (rustfmt ships with rustup), `rustfmt.toml` |
| C, C++, Objective-C | clang-format | `.clang-format` |
| C# | dotnet format | `*.csproj`, `*.sln` |
| Java, Kotlin | Spotless, ktlint, ktfmt, google-java-format | Gradle or Maven plugin config, `.editorconfig` ktlint keys |
| PHP | Pint, PHP-CS-Fixer | `pint.json`, `.php-cs-fixer*.php`, dependency |
| Ruby | RuboCop, Standard | `.rubocop.yml`, `.standard.yml`, Gemfile |
| Swift | swift-format, SwiftFormat | `.swift-format`, `.swiftformat` |
| Dart, Flutter | dart format | `pubspec.yaml` (ships with Dart) |
| Elixir | mix format | `.formatter.exs` |
| Shell | shfmt | `.editorconfig` shell keys, dependency or binary |
| Lua | StyLua | `stylua.toml`, `.stylua.toml` |
| Zig | zig fmt | `build.zig` (ships with Zig) |

When two formatters claim the same language (Prettier and Biome both configured), the one the project's format script or hook runs wins. If neither runs one, ask the user which is authoritative. Never configure two formatters for the same files.

Confirm each detected formatter resolves: run its version command through the project's runner (`npx`, `bunx`, `pnpm exec`, `uv run`, `poetry run`, `bundle exec`, or the toolchain binary). A configured formatter that does not resolve is "configured, not installed".

## 3. Propose installs

For each language with no formatter, or "configured, not installed", propose the standard one from the table: Prettier for JavaScript, TypeScript, CSS, JSON, and Markdown (Biome only when the project already uses Biome for linting, or the user asks for it), Ruff for Python, and the toolchain's own formatter where the language ships one. Install it the way the project installs dev tools:

- JavaScript, TypeScript: as a dev dependency with the package manager its lockfile names (`bun.lock`, `pnpm-lock.yaml`, `yarn.lock`, `package-lock.json`). Pin the exact version.
- Python: into the dev group with the project's tool (`uv add --dev`, `poetry add --group dev`, or a dev requirements file).
- Toolchain formatters (gofmt, rustfmt, dart format, mix format, zig fmt): no project dependency. If the binary is missing, give the toolchain command (`rustup component add rustfmt`) and do not install toolchains system-wide without approval.
- Others: the language's normal dev-dependency mechanism. If there is none, record the install command instead of running it.

Add a minimal config only when the formatter needs one to run, and match existing `.editorconfig` settings (indent style and size, line width, quotes, line endings). Add a `format` script or target that runs it, so humans and agents share one command.

Every install, config file, and script goes in the setup preview and waits for approval like every other write. A declined install is recorded as "none, declined" so poteto-mode reports it instead of guessing.

## 4. Write `docs/agents/formatter.md`

One section per language, in this shape:

```
## TypeScript
- Formatter: Prettier 3.3.3
- Source: package.json script `format` (existing)
- Config: .prettierrc
- Binary: node_modules/.bin/prettier (pinned, relative to the repo root)
- Format files: node_modules/.bin/prettier --write --ignore-unknown <files>
- Check files: node_modules/.bin/prettier --check --ignore-unknown <files>
- Applies to: *.ts, *.tsx, *.js, *.jsx, *.json, *.css, *.md
- Commit hook: none (or the hook that already formats staged files, such as lint-staged)
```

Record commands that take a file list, so agents format only what they changed, in one call. If the formatter cannot take a file list, record the narrowest command and say so.

Record the project-local binary path, never a bare `npx <tool>` or `bunx <tool>`. In a directory without the project's installed dependencies, such as a fresh worktree, those runners fall back to a globally cached or freshly downloaded version, which can format differently from the pinned one. Add the flag that skips files the formatter does not handle (`--ignore-unknown` for Prettier) so a mixed file list never fails.

If a commit hook already formats staged files, record it under Commit hook. poteto-mode then relies on the hook instead of formatting a second time. Record "none, declined" languages in the same shape with no commands.

Do not run the formatter over the existing codebase during setup. A repo-wide reformat buries real history in one diff and conflicts with every open branch. If the user explicitly asks for it, do it as its own commit and offer to add that commit to `.git-blame-ignore-revs`.

## 5. Verify

For each language with a formatter, run the check command on one tracked file read-only. Success means the command runs, whatever it reports about that file. A command that fails to start is a blocker for that language, not a successful setup.
