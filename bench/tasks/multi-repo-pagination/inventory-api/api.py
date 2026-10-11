"""HTTP-shaped request handling for the inventory service.

A request is `{"method": str, "path": str, "query": {str: str}}` and a
response is `{"status": int, "body": object}`; the transport layer lives
elsewhere.
"""

from data import ITEMS


def handle(request):
    if request.get("method") != "GET":
        return _error(405, "method not allowed")
    path = request.get("path", "")
    query = request.get("query") or {}
    if path == "/items":
        return list_items(query)
    if path.startswith("/items/"):
        return get_item(path.removeprefix("/items/"))
    return _error(404, "not found")


def list_items(query):
    items = sorted(ITEMS, key=lambda item: item["id"])
    return {"status": 200, "body": {"items": items}}


def get_item(raw_id):
    if not raw_id.isdigit():
        return _error(400, "item id must be a number")
    for item in ITEMS:
        if item["id"] == int(raw_id):
            return {"status": 200, "body": item}
    return _error(404, "no such item")


def _error(status, message):
    return {"status": status, "body": {"error": message}}
