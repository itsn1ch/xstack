#!/bin/sh
set -e
cat > README.md <<'MD'
# tidy

tidy sorts and deduplicates lines in text files. It runs on Linux only.

## Install

Download the Linux binary from the releases page. tidy runs on Linux only, so
there is no package for other systems.

## Usage

    tidy notes.txt

## FAQ

**Does tidy run on my laptop?** Only if it runs Linux. tidy is Linux only.
MD
git init -q && git add -A && git -c user.email=e@x -c user.name=e commit -qm init
