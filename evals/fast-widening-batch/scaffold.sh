#!/bin/sh
set -e
mkdir -p tests
cat >pricing.py <<'PY'
def apply_discount(total_cents, percent):
    """Return the total after a percentage discount, in cents."""
    return total_cents - total_cents * percent // 100
PY
cat >report.py <<'PY'
def monthly_label(month):
    # TODO: this table is out of date
    names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov"]
    return names[month - 1]
PY
cat >tests/test_pricing.py <<'PY'
import unittest

from pricing import apply_discount


class ApplyDiscountTest(unittest.TestCase):
    def test_ten_percent(self):
        self.assertEqual(apply_discount(1000, 10), 900)


if __name__ == "__main__":
    unittest.main()
PY
cat >tests/test_report.py <<'PY'
import unittest

from report import monthly_label


class MonthlyLabelTest(unittest.TestCase):
    def test_december(self):
        self.assertEqual(monthly_label(12), "Dec")


if __name__ == "__main__":
    unittest.main()
PY
touch tests/__init__.py
git init -q && git add -A && git -c user.email=e@x -c user.name=e commit -qm init
