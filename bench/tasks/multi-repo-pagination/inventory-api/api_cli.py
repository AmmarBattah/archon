"""Serve one request: JSON request on stdin, JSON response on stdout."""

import json
import sys

from api import handle

if __name__ == "__main__":
    print(json.dumps(handle(json.load(sys.stdin))))
