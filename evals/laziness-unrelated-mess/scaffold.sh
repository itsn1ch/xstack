#!/bin/sh
set -e
mkdir -p tests
cat > invoice.py <<'PY'
def total(items):
    """Sum item prices in cents."""
    result = items[0]["price"]
    for item in items[1:]:
        result = result + item["price"]
    return result


def legacy_format(x):
    s = ""
    s = s + "$"
    s = s + str(x // 100) + "." + str(x % 100).zfill(2)
    return s
PY
cat > tests/test_invoice.py <<'PY'
import unittest

from invoice import total


class TotalTest(unittest.TestCase):
    def test_sums_prices(self):
        self.assertEqual(total([{"price": 150}, {"price": 250}]), 400)


if __name__ == "__main__":
    unittest.main()
PY
touch tests/__init__.py
git init -q && git add -A && git -c user.email=e@x -c user.name=e commit -qm init
