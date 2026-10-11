import unittest

from api import handle
from data import ITEMS


def get(**query):
    return handle({"method": "GET", "path": "/items", "query": query})


def walk(limit=None):
    pages, cursor = [], None
    while True:
        query = {}
        if limit is not None:
            query["limit"] = str(limit)
        if cursor is not None:
            query["cursor"] = cursor
        res = get(**query)
        assert res["status"] == 200, res
        pages.append(res["body"]["items"])
        cursor = res["body"]["next_cursor"]
        if cursor is None:
            return pages
        assert isinstance(cursor, str) and cursor, cursor
        assert len(pages) < 100, "pagination never ends"


class PaginationTest(unittest.TestCase):
    def test_default_page_is_20(self):
        body = get()["body"]
        self.assertEqual([i["id"] for i in body["items"]], list(range(1, 21)))
        self.assertIsInstance(body["next_cursor"], str)

    def test_walk_covers_everything_once_in_order(self):
        for limit in (None, 1, 7, 20, 57, 100):
            pages = walk(limit)
            ids = [i["id"] for page in pages for i in page]
            self.assertEqual(ids, sorted(i["id"] for i in ITEMS), f"limit={limit}")
            if limit:
                self.assertTrue(all(len(p) <= limit for p in pages))
                self.assertEqual(len(pages), -(-len(ITEMS) // limit))

    def test_last_page_has_null_cursor(self):
        body = get(limit="100")["body"]
        self.assertEqual(len(body["items"]), 57)
        self.assertIsNone(body["next_cursor"])

    def test_bad_limits(self):
        for bad in ("0", "101", "-3", "abc", "2.5", ""):
            res = get(limit=bad)
            self.assertEqual(res["status"], 400, bad)
            self.assertIn("error", res["body"])

    def test_bad_cursors(self):
        for bad in ("garbage", "!!!", "0" * 64):
            res = get(cursor=bad)
            self.assertEqual(res["status"], 400, bad)
            self.assertIn("error", res["body"])

    def test_cursor_is_reusable(self):
        cursor = get(limit="10")["body"]["next_cursor"]
        first = get(limit="10", cursor=cursor)["body"]
        again = get(limit="10", cursor=cursor)["body"]
        self.assertEqual(first, again)
        self.assertEqual(first["items"][0]["id"], 11)

    def test_single_item_route_unchanged(self):
        self.assertEqual(handle({"method": "GET", "path": "/items/3"})["body"]["id"], 3)


if __name__ == "__main__":
    unittest.main()
