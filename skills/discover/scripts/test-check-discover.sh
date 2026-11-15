#!/bin/sh
set -u
here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0

repo=$tmp/repo
mkdir -p "$repo/src" "$repo/tests" "$tmp/bin"
printf '%s\n' 'pub fn acquire() {}' 'pub fn hold() {}' 'pub fn release() {}' >"$repo/src/lock.rs"
printf '%s\n' 'fn two_runs_never_share_a_lock() {' '    assert_eq!(held(), 1);' '}' >"$repo/tests/lock.rs"
printf '%s\n%s' 'first line' 'last line without a newline' >"$repo/src/tail.txt"
git -C "$repo" init -q
git -C "$repo" -c user.name=t -c user.email=t@example.invalid -c commit.gpgsign=false add .
git -C "$repo" -c user.name=t -c user.email=t@example.invalid -c commit.gpgsign=false commit -q -m "Lock per run because two runs on one machine would share a slot"
sha=$(git -C "$repo" rev-parse --short HEAD)

printf '%s\n' '#!/bin/sh' '[ "$1 $2 $3" = "pr view 7" ] || exit 1' 'echo "One lock file per run keeps runs apart"' >"$tmp/bin/gh"
chmod +x "$tmp/bin/gh"

cat >"$tmp/good.md" <<EOF
# Discover: locks

- topic: locks
- scope: src/lock.rs
- commit: $sha
- date: 2026-11-15

## run-lock

- intent: Keep two runs from using one slot at the same time.
- reason: Runs on one machine would otherwise share a slot. (source: $sha "two runs on one machine would share")
- invariant: Two runs never hold one slot. (guard: two_runs_never_share_a_lock tests/lock.rs:2)
- invariant: A crashed run frees its slot. (guard: unguarded)
- evidence: src/lock.rs:1, src/lock.rs:3

## release

- intent: Give a slot back when a run ends.
- reason: Separate lock files keep runs apart. (source: PR #7 "lock file per run keeps runs apart")
- invariant: Release frees exactly one slot. (guard: unguarded)
- evidence: src/lock.rs:3
EOF

run() { GH="$tmp/bin/gh" sh "$here/check-discover.sh" "$1" "$repo" 2>&1; }

expect_ok() {
	name=$1
	shift
	"$@" <"$tmp/good.md" >"$tmp/case.md"
	out=$(run "$tmp/case.md") || {
		echo "FAIL $name was rejected: $out"
		fail=1
	}
}

expect_reject() {
	name=$1
	pattern=$2
	shift 2
	"$@" <"$tmp/good.md" >"$tmp/case.md"
	if out=$(run "$tmp/case.md"); then
		echo "FAIL $name passed: $out"
		fail=1
	elif ! printf '%s\n' "$out" | grep -q -- "$pattern"; then
		echo "FAIL $name gave the wrong message: $out"
		fail=1
	fi
}

expect_ok "the valid file" cat
expect_ok "last line without a newline" sed -e 's/^- evidence: src\/lock.rs:3$/- evidence: src\/lock.rs:3, src\/tail.txt:2/'
expect_reject "hyphenated stray key" "unknown key see-also" awk '{ print } END { print "- see-also: x" }'
expect_reject "capitalized stray key" "unknown key Notes" awk '{ print } END { print "- Notes: x" }'
expect_reject "indented key" "invariant must start at the beginning of the line" sed -e 's/^- invariant: A crashed/  &/'
expect_reject "missing topic" "header needs - topic" sed -e '/^- topic:/d'
expect_reject "missing scope" "header needs - scope" sed -e '/^- scope:/d'
expect_reject "missing date" "header needs - date" sed -e '/^- date:/d'
expect_reject "stray header key" "unknown header key author" awk '{ print } /^- date:/ { print "- author: me" }'
expect_reject "made-up PR quote" "PR #7 does not contain" sed -e 's/"lock file per run keeps runs apart"/"anything at all goes"/'
expect_reject "unknown PR" "PR #99 does not contain" sed -e 's/PR #7/PR #99/'
expect_reject "evidence line 0" "evidence src/lock.rs:0 is not a line" sed -e 's/src\/lock.rs:1,/src\/lock.rs:0,/'
expect_reject "guard line 0" "guard tests/lock.rs:0 is not a line" sed -e 's/tests\/lock.rs:2)/tests\/lock.rs:0)/'
expect_reject "one-word quote" "quote needs at least three words" sed -e "s/\"two runs on one machine would share\"/\"two\"/"
expect_reject "source without a line" "is not a path:line" sed -e "s/(source: $sha /(source: src\/lock.rs /"
expect_reject "made-up commit quote" "message does not contain" sed -e "s/\"two runs on one machine would share\"/\"made up words here\"/"
expect_reject "made-up guard" "does not appear" sed -e 's/two_runs_never_share_a_lock/made_up_test/'
expect_reject "evidence past the end" "past the end" sed -e 's/src\/lock.rs:1,/src\/lock.rs:99,/'
expect_reject "duplicate key" "repeats line" sed -e 's/^## release$/## run-lock/'
expect_reject "star bullet key" "unexpected line" awk '{ print } END { print "* notes: x" }'
expect_reject "bare key" "unexpected line" awk '{ print } END { print "notes: x" }'
expect_reject "tab after dash" "unexpected line" awk '{ print } END { print "-\tnotes: x" }'
expect_reject "source line 0" "source src/lock.rs:0 is not a line" sed -e "s/(source: $sha /(source: src\/lock.rs:0 /"
expect_reject "source leading zero" "source src/lock.rs:01 is not a line" sed -e "s/(source: $sha /(source: src\/lock.rs:01 /"
expect_reject "path outside the repo" "outside the repository" sed -e 's/src\/lock.rs:1,/..\/outside.txt:1,/'
expect_reject "absolute path" "outside the repository" sed -e 's/src\/lock.rs:1,/\/etc\/hosts:1,/'
expect_reject "bad date" "date must be YYYY-MM-DD" sed -e 's/^- date: .*/- date: whenever/'
expect_reject "CRLF line endings" "CRLF" awk '{ printf "%s\r\n", $0 }'
expect_reject "star header line" "unexpected line in the header" awk '{ print } /^- date:/ { print "* author: me" }'
expect_reject "bare header line" "unexpected line in the header" awk '{ print } /^- date:/ { print "author: me" }'
expect_reject "heading without a space" "unexpected line in the header" awk '{ print } /^- date:/ { print "##stray" }'
expect_reject "empty evidence" "evidence needs at least one path:line" sed -e 's/^- evidence: src\/lock.rs:3$/- evidence: , ,/'
expect_reject "empty intent" "intent needs a value" sed -e 's/^- intent: Give a slot back.*/- intent: /'
expect_reject "intent without a space" "intent needs a value" sed -e 's/^- intent: Give/- intent:Give/'
expect_reject "header commit not in the repo" "commit ffffffff is not a commit" sed -e 's/^- commit: .*/- commit: ffffffff/'
expect_reject "impossible date" "date must be YYYY-MM-DD" sed -e 's/^- date: .*/- date: 2026-13-45/'
expect_reject "ref with two colons" "is not a line in a file" sed -e 's/src\/lock.rs:1,/src\/lock.rs:1:3,/'
expect_reject "empty topic" "topic needs a value" sed -e 's/^- topic: .*/- topic: /'
expect_reject "topic without a space" "topic needs a value" sed -e 's/^- topic: /- topic:/'
expect_reject "indented header key" "topic must start at the beginning of the line" sed -e 's/^- topic:/  - topic:/'
expect_reject "backslash in a quote" 'does not contain "a\\c made up words"' sed -e 's/"two runs on one machine would share"/"a\\c made up words"/'
expect_reject "missing source" "reason must end with" sed -e 's/ (source: PR #7 "lock file per run keeps runs apart")//'

if out=$(GH="$tmp/no-gh" sh "$here/check-discover.sh" "$tmp/good.md" "$repo" 2>&1); then
	echo "FAIL a PR quote passed without gh: $out"
	fail=1
elif ! printf '%s\n' "$out" | grep -q "needs gh"; then
	echo "FAIL a PR quote without gh gave the wrong message: $out"
	fail=1
fi

[ "$fail" = 0 ] && echo "CHECK-DISCOVER TESTS OK"
exit "$fail"
