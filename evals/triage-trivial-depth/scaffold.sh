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
cat >greeting.py <<'PY'
def greet(name):
    return f"Helo, {name}"
PY
cat >tests/test_greeting.py <<'PY'
import unittest

from greeting import greet


class GreetTest(unittest.TestCase):
    def test_returns_name(self):
        self.assertIn("Ada", greet("Ada"))


if __name__ == "__main__":
    unittest.main()
PY
touch tests/__init__.py
git init -q && git add -A && git -c user.email=e@x -c user.name=e commit -qm init
