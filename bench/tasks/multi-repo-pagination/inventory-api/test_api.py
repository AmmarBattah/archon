import unittest

from api import handle


def get(path, **query):
    return handle({"method": "GET", "path": path, "query": query})


class ApiTest(unittest.TestCase):
    def test_items_are_ordered_by_id(self):
        res = get("/items")
        self.assertEqual(res["status"], 200)
        ids = [item["id"] for item in res["body"]["items"]]
        self.assertEqual(ids[0], 1)
        self.assertEqual(ids, sorted(ids))

    def test_single_item(self):
        res = get("/items/5")
        self.assertEqual(res["body"]["sku"], "SKU-0005")
        self.assertEqual(get("/items/999")["status"], 404)
        self.assertEqual(get("/items/x")["status"], 400)

    def test_only_get(self):
        self.assertEqual(handle({"method": "POST", "path": "/items"})["status"], 405)


if __name__ == "__main__":
    unittest.main()
