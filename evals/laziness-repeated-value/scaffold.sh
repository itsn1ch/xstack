#!/bin/sh
set -e
mkdir -p client tests
cat > client/http.py <<'PY'
TIMEOUT_SECONDS = 10


def get(url, timeout=TIMEOUT_SECONDS):
    return ("GET", url, timeout)
PY
cat > client/upload.py <<'PY'
UPLOAD_TIMEOUT_SECONDS = 10


def put(url, body, timeout=UPLOAD_TIMEOUT_SECONDS):
    return ("PUT", url, len(body), timeout)
PY
cat > client/health.py <<'PY'
def ping(url):
    return ("GET", url + "/health", 10)
PY
touch client/__init__.py tests/__init__.py
cat > tests/test_client.py <<'PY'
import unittest

from client.http import get


class ClientTest(unittest.TestCase):
    def test_get(self):
        self.assertEqual(get("http://x")[0], "GET")


if __name__ == "__main__":
    unittest.main()
PY
git init -q && git add -A && git -c user.email=e@x -c user.name=e commit -qm init
