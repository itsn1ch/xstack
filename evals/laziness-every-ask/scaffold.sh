#!/bin/sh
set -e
mkdir -p tests
cat > cli.py <<'PY'
import argparse


def build_parser():
    parser = argparse.ArgumentParser(prog="sync")
    parser.add_argument("--verbose", action="store_true", help="print every file synced")
    return parser


def log_level(args):
    return "debug" if args.verbose else "info"
PY
cat > tests/test_cli.py <<'PY'
import unittest

from cli import build_parser, log_level


class CliTest(unittest.TestCase):
    def test_verbose(self):
        self.assertEqual(log_level(build_parser().parse_args(["--verbose"])), "debug")


if __name__ == "__main__":
    unittest.main()
PY
touch tests/__init__.py
git init -q && git add -A && git -c user.email=e@x -c user.name=e commit -qm init
