#!/bin/sh
set -e
mkdir -p docs/agents
cat >AGENTS.md <<'MD'
## Xstack

- Formatter: `docs/agents/formatter.md`
- Models: harness default
MD
printf '# Code formatters\n\n## Python\n\n- Formatter: none, declined\n' >docs/agents/formatter.md
mkdir -p tests
cat >stats.py <<'PY'
def average(values):
    return sum(values) / len(values)
PY
cat >tests/test_stats.py <<'PY'
import unittest

from stats import average


class AverageTest(unittest.TestCase):
    def test_mean(self):
        self.assertEqual(average([1, 2, 3]), 2)


if __name__ == "__main__":
    unittest.main()
PY
touch tests/__init__.py
printf 'test:\n\t./scripts/run-tests.sh\n' >Makefile
git init -q && git add -A && git -c user.email=e@x -c user.name=e commit -qm init
