#!/bin/sh
# Prints XSTACK READY and exits 0 when /setup-xstack has configured this project and machine.
# Otherwise prints XSTACK SETUP MISSING with every missing part and exits 1.
here=$(cd "$(dirname "$0")" && pwd)
root=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
missing=""
add() { missing="$missing
- $1"; }

block=""
for f in CLAUDE.md AGENTS.md; do
	[ -f "$root/$f" ] || continue
	block=$(awk '/^## (Xstack|Agent skills)[[:space:]]*$/ { on = 1; next } on && /^## / { exit } on' "$root/$f")
	[ -n "$block" ] && break
done
if [ -z "$block" ]; then
	add "no ## Xstack block in CLAUDE.md or AGENTS.md"
else
	# Configuration files must exist. Domain documents such as a glossary may legitimately not exist yet.
	for pointer in $(printf '%s\n' "$block" | grep -i -e 'docs/agents/' -e 'formatter' | grep -o '`[^`<> ]*\.md`' | tr -d '`'); do
		[ -f "$root/$pointer" ] || add "configuration pointer $pointer does not exist"
	done
	printf '%s\n' "$block" | grep -qi 'formatter' || add "the Xstack block names no formatter configuration"
fi

models=""
for f in "$HOME/.claude/rules/pstack-models.md" "$HOME/.cursor/rules/pstack-models.mdc" "$HOME/.agents/pstack-models.md"; do
	[ -f "$f" ] && models=$f
done
# A harness-managed model policy has no file. Setup records its location as a `Models:` line in the block.
printf '%s\n' "$block" | grep -qi '^[-* ]*models:' && models=block
[ -n "${XSTACK_MODELS:-}" ] && models=env
[ -n "$models" ] || add "no model settings (pstack-models file, or a Models: line in the Xstack block)"

find_binary() {
	for root_dir in "${XSTACK_ROOT:-}" "${CLAUDE_PLUGIN_ROOT:-}" "$here/../../.."; do
		[ -n "$root_dir" ] && [ -x "$root_dir/$2/target/release/$1" ] && return 0
	done
	command -v "$1" >/dev/null 2>&1 || [ -x "$HOME/.cargo/bin/$1" ]
}
find_binary subtask subtasks || add "subtask binary not built or installed"
find_binary worker worker || add "worker binary not built or installed"

if [ -n "$missing" ]; then
	printf 'XSTACK SETUP MISSING in %s:%s\nRun /setup-xstack first.\n' "$root" "$missing"
	exit 1
fi
echo "XSTACK READY"
