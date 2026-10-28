#!/bin/sh
# Prints one `file:line: problem` per spacing or leftover problem in the text files given as arguments.
# Exits 1 when it printed anything, so a `subtask` summary lists the guilty batches first.
found=0
for f in "$@"; do
  [ -f "$f" ] || continue
  [ -s "$f" ] || continue
  grep -Iq . "$f" || continue
  # PEP 8 wants two blank lines between top-level definitions.
  case "$f" in *.py | *.pyi) blanks=3 ;; *) blanks=2 ;; esac
  # Blank-line rules belong to the embedded language inside shell heredocs and Markdown fences.
  case "$f" in *.sh | *.bash | *.zsh) embed=heredoc ;; *.md | *.markdown) embed=fence ;; *) embed=none ;; esac
  out=$(awk -v f="$f" -v max="$blanks" -v embed="$embed" '
    /\r$/ { print f ":" NR ": CRLF line ending" }
    { line = $0; sub(/\r$/, "", line) }
    line ~ /[ \t]+$/ { print f ":" NR ": trailing whitespace" }
    line ~ /^(\t+ | +\t)/ { print f ":" NR ": tabs and spaces mixed in indent" }
    line ~ /^(<<<<<<<|=======|>>>>>>>)( |$)/ { print f ":" NR ": merge conflict marker" }
    embed == "heredoc" && tag != "" { if (line ~ "^\t*" tag "$") tag = ""; blank = 0; next }
    embed == "heredoc" && line !~ /<<</ && match(line, /<<-?[ \t]*["'"'"']?[A-Za-z_][A-Za-z0-9_]*/) {
      tag = substr(line, RSTART, RLENGTH); sub(/^<<-?[ \t]*["'"'"']?/, "", tag)
    }
    embed == "fence" && line ~ /^[ \t]*(```|~~~)/ { fenced = !fenced; blank = 0; next }
    fenced { next }
    line ~ /^[ \t]*$/ { blank++; if (blank == max) print f ":" NR ": " max " blank lines in a row"; next }
    { blank = 0 }
  ' "$f")
  [ -n "$(tail -c1 "$f")" ] && out="$out${out:+
}$f: no newline at end of file"
  if [ -n "$out" ]; then
    printf '%s\n' "$out"
    found=1
  fi
done
exit "$found"
