#!/bin/sh
if [ $# -lt 1 ]; then
	echo "usage: sh check-discover.sh <docs/discover/topic.md> [repo root]" >&2
	exit 2
fi
file=$1
root=${2:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}
gh=${GH:-gh}
tab=$(printf '\t')
[ -f "$file" ] || {
	echo "$file: not found"
	exit 1
}
if grep -q "$(printf '\r')" "$file"; then
	echo "$file: uses CRLF line endings; convert it to LF"
	exit 1
fi

scan() {
	awk -v f="$file" '
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
		/^[ \t]*$/ { next }
		key == "" && /^[ \t]*- / {
			name = key_name($0)
			if ($0 ~ /^- (topic|scope): ./) header[name]++
			else if ($0 ~ /^- commit: [0-9a-f]+$/) { header[name]++; print "COMMIT\t" NR "\t" substr($0, 11) }
			else if ($0 ~ /^- commit: /) { reported[name] = 1; print f ":" NR ": commit must be a hex sha" }
			else if ($0 ~ /^- date: [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]$/ && substr($0, 14, 2) + 0 >= 1 && substr($0, 14, 2) + 0 <= 12 && substr($0, 17, 2) + 0 >= 1 && substr($0, 17, 2) + 0 <= 31) header[name]++
			else if ($0 ~ /^- date: /) { reported[name] = 1; print f ":" NR ": date must be YYYY-MM-DD" }
			else if ($0 ~ /^[ \t]+- (topic|scope|commit|date):/) { reported[name] = 1; print f ":" NR ": " name " must start at the beginning of the line" }
			else if ($0 ~ /^- (topic|scope|commit|date):/) { reported[name] = 1; print f ":" NR ": " name " needs a value after - " name ":" }
			else print f ":" NR ": unknown header key " name " (use topic, scope, commit, date)"
			next
		}
		key == "" { print f ":" NR ": unexpected line in the header (use - topic:, - scope:, - commit:, - date:, then ## entries)"; next }
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
		/^- evidence: ./ {
			ev++
			refs = substr($0, 13)
			bare = refs
			gsub(/[ ,]/, "", bare)
			if (bare == "") print f ":" NR ": evidence needs at least one path:line"
			else print "EVIDENCE\t" NR "\t" refs
			next
		}
		/^[ \t]*- (intent|reason|invariant|evidence):/ {
			name = key_name($0)
			if (name == "intent") intent++
			else if (name == "reason") reason++
			else if (name == "invariant") inv++
			else ev++
			if ($0 ~ /^[ \t]/) print f ":" NR ": " name " must start at the beginning of the line"
			else print f ":" NR ": " name " needs a value after - " name ":"
			next
		}
		/^[ \t]*- [^ :]+:/ { print f ":" NR ": unknown key " key_name($0) " (use intent, reason, invariant, evidence at the start of the line)"; next }
		{ print f ":" NR ": unexpected line in entry " key " (each line is - intent:, - reason:, - invariant:, or - evidence:)" }
		END {
			close_entry()
			split("topic scope commit date", wanted, " ")
			for (i = 1; i <= 4; i++) if (header[wanted[i]] != 1 && !reported[wanted[i]]) print f ": header needs - " wanted[i] ": line exactly once"
			if (!start) print f ": no ## entries"
			n = 0
			for (k in seen) n++
			print "ENTRIES\t" n
		}
	' "$file"
}

check_ref() {
	kind=$1
	at=$2
	ref_path=$3
	ref_line=$4
	case "$ref_line" in
	'' | *[!0-9]* | 0*)
		echo "$at: $kind $ref_path:$ref_line is not a line in a file"
		return 1
		;;
	esac
	case "$ref_path" in
	*:*)
		echo "$at: $kind $ref_path:$ref_line is not a line in a file"
		return 1
		;;
	/* | .. | ../* | */../* | */..)
		echo "$at: $kind $ref_path:$ref_line is outside the repository"
		return 1
		;;
	esac
	if [ ! -f "$root/$ref_path" ]; then
		echo "$at: $kind $ref_path:$ref_line names a missing file"
		return 1
	fi
	if [ "$(awk 'END { print NR }' "$root/$ref_path")" -lt "$ref_line" ]; then
		echo "$at: $kind $ref_path:$ref_line is past the end of the file"
		return 1
	fi
}

check_evidence() {
	while IFS=$tab read -r line refs; do
		printf '%s\n' "$refs" | tr ',' '\n' | sed 's/^ *//; s/ *$//' | while read -r ref; do
			case "$ref" in
			'') ;;
			*:*) check_ref evidence "$file:$line" "${ref%:*}" "${ref##*:}" ;;
			*) echo "$file:$line: evidence $ref is not a line in a file" ;;
			esac
		done
	done
}

check_sources() {
	while IFS=$tab read -r line ref quote; do
		case "$ref" in
		'PR #'*)
			if ! command -v "$gh" >/dev/null 2>&1; then
				printf '%s\n' "$file:$line: source $ref needs gh to check its quote"
			elif ! "$gh" pr view "${ref#PR #}" --json title,body -q '.title + "\n" + .body' 2>/dev/null | grep -qF -- "$quote"; then
				printf '%s\n' "$file:$line: source $ref does not contain \"$quote\""
			fi
			;;
		*:*)
			path=${ref%:*}
			number=${ref##*:}
			if check_ref source "$file:$line" "$path" "$number" && ! sed -n "${number}p" "$root/$path" | grep -qF -- "$quote"; then
				printf '%s\n' "$file:$line: source $ref does not contain \"$quote\""
			fi
			;;
		'' | *[!0-9a-f]*)
			echo "$file:$line: source $ref is not a path:line, commit sha, or PR #n"
			;;
		*)
			if ! git -C "$root" cat-file -e "$ref^{commit}" 2>/dev/null; then
				echo "$file:$line: source $ref is not a commit in $root"
			elif ! git -C "$root" log -1 --format=%B "$ref^{commit}" | grep -qF -- "$quote"; then
				printf '%s\n' "$file:$line: commit $ref message does not contain \"$quote\""
			fi
			;;
		esac
	done
}

check_commits() {
	while IFS=$tab read -r line sha; do
		git -C "$root" cat-file -e "$sha^{commit}" 2>/dev/null ||
			echo "$file:$line: commit $sha is not a commit in $root"
	done
}

check_guards() {
	while IFS=$tab read -r line guard; do
		name=${guard% *}
		ref=${guard##* }
		path=${ref%:*}
		if check_ref guard "$file:$line" "$path" "${ref##*:}" && ! grep -qF -- "$name" "$root/$path"; then
			echo "$file:$line: guard $name does not appear in $path"
		fi
	done
}

rows() {
	printf '%s\n' "$out" | awk -F '\t' -v tag="$1" '$1 == tag { sub(/^[^\t]*\t/, ""); print }'
}

out=$(scan)
problems=$(printf '%s\n' "$out" | grep -v -e "^EVIDENCE$tab" -e "^SOURCE$tab" -e "^GUARD$tab" -e "^COMMIT$tab" -e "^ENTRIES$tab")
all=$(
	printf '%s\n' "$problems"
	rows EVIDENCE | check_evidence
	rows SOURCE | check_sources
	rows GUARD | check_guards
	rows COMMIT | check_commits
)
all=$(printf '%s\n' "$all" | sed '/^$/d')
if [ -n "$all" ]; then
	printf '%s\n' "$all"
	exit 1
fi
echo "DISCOVER OK $(rows ENTRIES) entries in $file"
