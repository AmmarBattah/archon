import unittest
from datetime import date

from tasktracker.dates import parse_due
from tasktracker.export import to_csv
from tasktracker.search import search
from tasktracker.store import TaskStore, UnknownTask

TODAY = date(2026, 3, 10)


class BasicTest(unittest.TestCase):
    def test_store_lifecycle(self):
        store = TaskStore()
        a = store.add("write report", tags=["work"])
        b = store.add("buy milk")
        store.complete(a.id)
        store.delete(b.id)
        self.assertEqual([t.id for t in store.all()], [a.id])
        self.assertEqual(store.get(a.id).status, "done")
        with self.assertRaises(UnknownTask):
            store.get(b.id)

    def test_due_dates(self):
        self.assertEqual(parse_due("2026-03-12", TODAY), date(2026, 3, 12))
        self.assertEqual(parse_due(" Tomorrow ", TODAY), date(2026, 3, 11))
        self.assertEqual(parse_due("+5d", TODAY), date(2026, 3, 15))

    def test_export_simple(self):
        store = TaskStore()
        store.add("plain title", tags=["a", "b"], due=date(2026, 4, 1))
        lines = to_csv(store.all()).splitlines()
        self.assertEqual(lines, ["id,title,tags,status,due", "1,plain title,a;b,open,2026-04-01"])

    def test_search_by_word(self):
        store = TaskStore()
        store.add("write report")
        store.add("buy milk")
        self.assertEqual([t.title for t in search(store.all(), "report")], ["write report"])


if __name__ == "__main__":
    unittest.main()
