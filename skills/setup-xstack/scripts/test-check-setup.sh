#!/bin/sh
# Proves every bundled skill carries the setup gate and that check-setup.sh tells a configured project from one that is not.
set -u
here=$(cd "$(dirname "$0")" && pwd)
xstack=$(cd "$here/../../.." && pwd)
fail=0
check() { if [ "$1" = ok ]; then :; else
	echo "FAIL $2"
	fail=1
fi; }

for skill in "$xstack"/skills/*/SKILL.md; do
	# setup-xstack runs the setup, and poteto-help explains it, so both work before setup.
	case "$skill" in */setup-xstack/SKILL.md | */poteto-help/SKILL.md) continue ;; esac
	grep -q '^\*\*Setup gate\.\*\* .*setup-xstack/scripts/check-setup\.sh' "$skill" && r=ok || r=no
	check $r "missing setup gate in ${skill#"$xstack"/}"
done

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/home" "$tmp/project"
git -C "$tmp/project" init -q
run() { (cd "$tmp/project" && HOME="$tmp/home" XSTACK_ROOT="$xstack" sh "$here/check-setup.sh"); }

out=$(run) && r=no || r=ok
check $r "an unconfigured project passed: $out"
printf '%s\n' "$out" | grep -q 'no ## Xstack block' && r=ok || r=no
check $r "missing block not reported: $out"
printf '%s\n' "$out" | grep -q 'no model settings' && r=ok || r=no
check $r "missing model settings not reported: $out"

printf '# Project\n\n## Xstack\n\n- Before every commit, run the formatter from `docs/agents/formatter.md`.\n' >"$tmp/project/AGENTS.md"
mkdir -p "$tmp/home/.agents"
: >"$tmp/home/.agents/pstack-models.md"
out=$(run) && r=no || r=ok
check $r "a dangling pointer passed: $out"
printf '%s\n' "$out" | grep -q 'docs/agents/formatter.md does not exist' && r=ok || r=no
check $r "dangling pointer not reported: $out"

mkdir -p "$tmp/project/docs/agents"
: >"$tmp/project/docs/agents/formatter.md"
out=$(run) && r=ok || r=no
check $r "a configured project failed: $out"
[ "$out" = "XSTACK READY" ] && r=ok || r=no
check $r "ready output was: $out"

rm "$tmp/home/.agents/pstack-models.md"
printf -- '- Models: configured by the harness policy\n- See `GLOSSARY.md` once it exists.\n' >>"$tmp/project/AGENTS.md"
out=$(run) && r=ok || r=no
check $r "a harness-managed model policy or a future glossary failed: $out"

[ "$fail" = 0 ] && echo "CHECK-SETUP OK"
exit "$fail"
