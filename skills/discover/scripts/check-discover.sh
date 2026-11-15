#!/bin/sh
if [ $# -lt 1 ]; then
	echo "usage: sh check-discover.sh <docs/discover/topic.md> [repo root]" >&2
	exit 2
fi
file=$1
root=${2:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}
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
	/^# / && NR == 1 { title = 1; next }
	/^- commit: [0-9a-f]+$/ && key == "" { commit = 1; next }
	/^## / {
		close_entry()
		key = substr($0, 4); start = NR; intent = reason = inv = ev = 0
		if (key !~ /^[a-z0-9]+(-[a-z0-9]+)*$/) print f ":" NR ": key " key " is not kebab-case"
		if (key in seen) print f ":" NR ": key " key " repeats line " seen[key]
		seen[key] = NR
		next
	}
	key == "" { next }
	/^- intent: ./ { intent++; next }
	/^- reason: ./ {
		reason++
		if ($0 ~ /\(source: inferred\)$/) next
		if (match($0, /\(source: [^ ()"]+ "[^"]+"\)$/) || match($0, /\(source: PR #[0-9]+ "[^"]+"\)$/)) {
			src = substr($0, RSTART + 9, RLENGTH - 10)
			ref = src; sub(/ "[^"]*"$/, "", ref)
			quote = src; sub(/^[^"]*"/, "", quote); sub(/"$/, "", quote)
			print "SOURCE\t" NR "\t" ref "\t" quote
		} else {
			print f ":" NR ": reason must end with (source: inferred) or (source: <path:line, commit sha, or PR #n> \"<words quoted from it>\")"
		}
		next
	}
	/^- invariant: ./ {
		inv++
		if ($0 ~ /\(guard: unguarded\)$/) next
		if (match($0, /\(guard: [^ ()]+ [^ ()]+:[0-9]+\)$/)) {
			g = substr($0, RSTART + 8, RLENGTH - 9)
			print "GUARD\t" NR "\t" g
		} else {
			print f ":" NR ": invariant must end with (guard: unguarded) or (guard: <test, assert, or type name> <path:line>)"
		}
		next
	}
	/^- evidence: ./ { ev++; print "EVIDENCE\t" NR "\t" substr($0, 13); next }
	/^- [a-z_]+:/ { print f ":" NR ": unknown key " $2 " (use intent, reason, invariant, evidence)"; next }
	END {
		close_entry()
		if (!title) print f ":1: first line must be the # title"
		if (!commit) print f ": header needs a - commit: <sha> line"
		if (!start) print f ": no ## entries"
		print "ENTRIES\t" length(seen)
	}
' "$file")

problems=$(printf '%s\n' "$out" | grep -v '^EVIDENCE	' | grep -v '^SOURCE	' | grep -v '^GUARD	' | grep -v '^ENTRIES	')
entries=$(printf '%s\n' "$out" | awk -F '\t' '/^ENTRIES\t/ { print $2 }')
evidence_problems=$(printf '%s\n' "$out" | awk -F '\t' '/^EVIDENCE\t/ { print $2 "\t" $3 }' | while IFS='	' read -r line refs; do
	printf '%s\n' "$refs" | tr ',' '\n' | sed 's/^ *//; s/ *$//' | while read -r ref; do
		[ -n "$ref" ] || continue
		path=${ref%:*}
		num=${ref##*:}
		case "$num" in ('' | *[!0-9]*)
			echo "$file:$line: evidence $ref is not path:line"
			continue
			;;
		esac
		if [ ! -f "$root/$path" ]; then
			echo "$file:$line: evidence $ref names a missing file"
		elif [ "$(wc -l <"$root/$path")" -lt "$num" ]; then
			echo "$file:$line: evidence $ref is past the end of the file"
		fi
	done
done)

source_problems=$(printf '%s\n' "$out" | awk -F '\t' '/^SOURCE\t/ { print $2 "\t" $3 "\t" $4 }' | while IFS='	' read -r line ref quote; do
	case "$ref" in
	(PR\ \#*) continue ;;
	(*:*)
		path=${ref%:*}
		num=${ref##*:}
		if [ ! -f "$root/$path" ]; then
			echo "$file:$line: source $ref names a missing file"
		elif ! sed -n "${num}p" "$root/$path" | grep -qF -- "$quote"; then
			echo "$file:$line: source $ref does not contain \"$quote\""
		fi
		;;
	(*)
		if ! git -C "$root" log -1 --format=%B "$ref" 2>/dev/null | grep -qF -- "$quote"; then
			echo "$file:$line: commit $ref message does not contain \"$quote\""
		fi
		;;
	esac
done)

guard_problems=$(printf '%s\n' "$out" | awk -F '\t' '/^GUARD\t/ { print $2 "\t" $3 }' | while IFS='	' read -r line guard; do
	name=${guard% *}
	ref=${guard##* }
	path=${ref%:*}
	num=${ref##*:}
	if [ ! -f "$root/$path" ]; then
		echo "$file:$line: guard $ref names a missing file"
	elif [ "$(wc -l <"$root/$path")" -lt "$num" ]; then
		echo "$file:$line: guard $ref is past the end of the file"
	elif ! grep -qF -- "$name" "$root/$path"; then
		echo "$file:$line: guard $name does not appear in $path"
	fi
done)

all=$(printf '%s\n%s\n%s\n%s\n' "$problems" "$evidence_problems" "$source_problems" "$guard_problems" | sed '/^$/d')
if [ -n "$all" ]; then
	printf '%s\n' "$all"
	exit 1
fi
echo "DISCOVER OK $entries entries in $file"
