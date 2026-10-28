#!/bin/sh
set -e
mkdir -p tests
cat > netcfg.py <<'PY'
def parse_port(text):
    """Return the TCP port written in text, such as "8080"."""
    return int(text)
PY
cat > tests/test_netcfg.py <<'PY'
import unittest

from netcfg import parse_port


class ParsePortTest(unittest.TestCase):
    def test_reads_a_port(self):
        self.assertEqual(parse_port("8080"), 8080)


if __name__ == "__main__":
    unittest.main()
PY
touch tests/__init__.py
git init -q && git add -A && git -c user.email=e@x -c user.name=e commit -qm init
