#!/bin/sh
if [ $# -lt 1 ]; then
	echo "usage: sh check-discover.sh <docs/discover/topic.md> [repo root]" >&2
	exit 2
fi
file=$1
root=${2:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}
gh=${GH:-gh}
[ -f "$file" ] || {
	echo "$file: not found"
	exit 1
}

out=$(awk -v f="$file" '
	function close_entry() {
		if (key == "") return
		if (intent != 1) print f ":" start ": " key ": needs exactly one intent line, found " intent
		if (reason != 1) print f ":" start ": " key ": needs exactly one reason line, found " reason
		if (inv < 1) print f ":" start ": " key ": needs at least one invariant line"
		if (ev != 1) print f ":" start ": " key ": needs exactly one evidence line, found " ev
	}
	function key_name(line) {
		sub(/^[ \t]*- /, "", line)
		sub(/:.*/, "", line)
		return line
	}
	NR == 1 { if ($0 ~ /^# ./) next; print f ":1: first line must be the # title" }
	/^## / {
		close_entry()
		key = substr($0, 4); start = NR; intent = reason = inv = ev = 0
		if (key !~ /^[a-z0-9]+(-[a-z0-9]+)*$/) print f ":" NR ": key " key " is not kebab-case"
		if (key in seen) print f ":" NR ": key " key " repeats line " seen[key]
		seen[key] = NR
		next
	}
	key == "" && /^[ \t]*- / {
		name = key_name($0)
		if ($0 ~ /^- (topic|scope|date): ./ || $0 ~ /^- commit: [0-9a-f]+$/) header[name]++
		else if ($0 ~ /^- commit: /) print f ":" NR ": commit must be a hex sha"
		else print f ":" NR ": unknown header key " name " (use topic, scope, commit, date)"
		next
	}
	key == "" { next }
	/^- intent: ./ { intent++; next }
	/^- reason: ./ {
		reason++
		if ($0 ~ /\(source: inferred\)$/) next
		if (match($0, /\(source: PR #[0-9]+ "[^"]+"\)$/) || match($0, /\(source: [^ ()"]+ "[^"]+"\)$/)) {
			src = substr($0, RSTART + 9, RLENGTH - 10)
			ref = src; sub(/ "[^"]*"$/, "", ref)
			quote = src; sub(/^[^"]*"/, "", quote); sub(/"$/, "", quote)
			if (split(quote, words, /[ \t]+/) < 3) print f ":" NR ": source quote needs at least three words"
			else print "SOURCE\t" NR "\t" ref "\t" quote
		} else {
			print f ":" NR ": reason must end with (source: inferred) or (source: <path:line, commit sha, or PR #n> \"<words quoted from it>\")"
		}
		next
	}
	/^- invariant: ./ {
		inv++
		if ($0 ~ /\(guard: unguarded\)$/) next
		if (match($0, /\(guard: [^ ()]+ [^ ()]+:[0-9]+\)$/)) print "GUARD\t" NR "\t" substr($0, RSTART + 8, RLENGTH - 9)
		else print f ":" NR ": invariant must end with (guard: unguarded) or (guard: <test, assert, or type name> <path:line>)"
		next
	}
	/^- evidence: ./ { ev++; print "EVIDENCE\t" NR "\t" substr($0, 13); next }
	/^[ \t]*- / { print f ":" NR ": unknown key " key_name($0) " (use intent, reason, invariant, evidence at the start of the line)"; next }
	END {
		close_entry()
		split("topic scope commit date", wanted, " ")
		for (i = 1; i <= 4; i++) if (header[wanted[i]] != 1) print f ": header needs - " wanted[i] ": line exactly once"
		if (!start) print f ": no ## entries"
		n = 0
		for (k in seen) n++
		print "ENTRIES\t" n
	}
' "$file")

check_line() {
	kind=$1
	ref=$2
	path=${ref%:*}
	num=${ref##*:}
	case "$num" in
	('' | *[!0-9]* | 0*)
		echo "$file:$line: $kind $ref is not a line in a file"
		return 1
		;;
	esac
	if [ ! -f "$root/$path" ]; then
		echo "$file:$line: $kind $ref names a missing file"
		return 1
	elif [ "$(wc -l <"$root/$path")" -lt "$num" ]; then
		echo "$file:$line: $kind $ref is past the end of the file"
		return 1
	fi
}

problems=$(printf '%s\n' "$out" | grep -v '^EVIDENCE	' | grep -v '^SOURCE	' | grep -v '^GUARD	' | grep -v '^ENTRIES	')
entries=$(printf '%s\n' "$out" | awk -F '\t' '/^ENTRIES\t/ { print $2 }')

evidence_problems=$(printf '%s\n' "$out" | awk -F '\t' '/^EVIDENCE\t/ { print $2 "\t" $3 }' | while IFS='	' read -r line refs; do
	printf '%s\n' "$refs" | tr ',' '\n' | sed 's/^ *//; s/ *$//' | while read -r ref; do
		[ -n "$ref" ] || continue
		case "$ref" in
		(*:*) check_line evidence "$ref" ;;
		(*) echo "$file:$line: evidence $ref is not a line in a file" ;;
		esac
	done
done)

source_problems=$(printf '%s\n' "$out" | awk -F '\t' '/^SOURCE\t/ { print $2 "\t" $3 "\t" $4 }' | while IFS='	' read -r line ref quote; do
	case "$ref" in
	(PR\ \#*)
		number=${ref#PR #}
		if ! command -v "$gh" >/dev/null 2>&1; then
			echo "$file:$line: source $ref needs gh to check its quote"
		elif ! "$gh" pr view "$number" --json title,body -q '.title + "\n" + .body' 2>/dev/null | grep -qF -- "$quote"; then
			echo "$file:$line: source $ref does not contain \"$quote\""
		fi
		;;
	(*:*)
		check_line source "$ref" && ! sed -n "${num}p" "$root/$path" | grep -qF -- "$quote" &&
			echo "$file:$line: source $ref does not contain \"$quote\""
		;;
	('' | *[!0-9a-f]*)
		echo "$file:$line: source $ref is not a path:line, commit sha, or PR #n"
		;;
	(*)
		if ! git -C "$root" cat-file -e "$ref^{commit}" 2>/dev/null; then
			echo "$file:$line: source $ref is not a commit in $root"
		elif ! git -C "$root" log -1 --format=%B "$ref^{commit}" | grep -qF -- "$quote"; then
			echo "$file:$line: commit $ref message does not contain \"$quote\""
		fi
		;;
	esac
done)

guard_problems=$(printf '%s\n' "$out" | awk -F '\t' '/^GUARD\t/ { print $2 "\t" $3 }' | while IFS='	' read -r line guard; do
	name=${guard% *}
	ref=${guard##* }
	check_line guard "$ref" && ! grep -qF -- "$name" "$root/$path" &&
		echo "$file:$line: guard $name does not appear in $path"
done)

all=$(printf '%s\n%s\n%s\n%s\n' "$problems" "$evidence_problems" "$source_problems" "$guard_problems" | sed '/^$/d')
if [ -n "$all" ]; then
	printf '%s\n' "$all"
	exit 1
fi
echo "DISCOVER OK $entries entries in $file"
