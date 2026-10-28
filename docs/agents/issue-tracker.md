# Issue tracker

- Type: GitHub.
- Host: github.com.
- Destination: itsn1ch/xstack (https://github.com/itsn1ch/xstack).
- Tool: authenticated `gh` CLI. Never store credentials in this file.
- Publication label: resolve the `ready-for-agent` role in `docs/agents/triage-labels.md`.

## Operations

Run from the project root; every operation names the destination explicitly.

- Verify access: `gh repo view github.com/itsn1ch/xstack`.
- Read body: `gh issue view <number> --repo github.com/itsn1ch/xstack --json number,title,body,url,labels`.
- Read comments: `gh issue view <number> --repo github.com/itsn1ch/xstack --comments`.
- Publish: `gh issue create --repo github.com/itsn1ch/xstack --title "<title>" --body-file <body.md> --label "ready-for-agent"`.
- For a source issue, attach a new ticket as a native sub-issue by adding `--parent <source-number>` to the create command.
- Native blocking links: add `--blocked-by <number>[,<number>...]` to the create command. The installed CLI supports both relationship flags.
- Reference issues as `itsn1ch/xstack#<number>` or `https://github.com/itsn1ch/xstack/issues/<number>`. Also include human-readable dependency references in ticket bodies.

Do not silently omit required relationships if the server denies an operation: report the blocker. Setup verifies access and the publication label read-only, except for explicitly approved missing-label creation. It never creates a sample issue.
