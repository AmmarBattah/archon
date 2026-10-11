import unittest

from tasktracker.models import Task
from tasktracker.search import QueryError, search

TASKS = [
    Task(1, "Fix login page", ["web", "Urgent"], "open"),
    Task(2, "fix the login flow docs", ["docs"], "done"),
    Task(3, "Write report", ["work"], "open"),
    Task(4, "login: audit", ["web"], "done"),
]


def ids(query):
    return [t.id for t in search(TASKS, query)]


class SearchTest(unittest.TestCase):
    def test_words_are_case_insensitive_and_anded(self):
        self.assertEqual(ids("LOGIN"), [1, 2, 4])
        self.assertEqual(ids("fix login"), [1, 2])
        self.assertEqual(ids("fix   report"), [])

    def test_phrases(self):
        self.assertEqual(ids('"fix login"'), [1])
        self.assertEqual(ids('"login page" fix'), [1])

    def test_tag_and_status(self):
        self.assertEqual(ids("tag:web"), [1, 4])
        self.assertEqual(ids("tag:urgent"), [1])
        self.assertEqual(ids("tag:we"), [])
        self.assertEqual(ids("status:done"), [2, 4])
        self.assertEqual(ids("status:open tag:web login"), [1])

    def test_blank_query_matches_everything(self):
        self.assertEqual(ids(""), [1, 2, 3, 4])
        self.assertEqual(ids("   "), [1, 2, 3, 4])

    def test_bad_queries_raise(self):
        for query in ["owner:me", "status:blocked", '"unterminated', 'fix "login']:
            with self.subTest(query=query):
                with self.assertRaises(QueryError):
                    search(TASKS, query)


if __name__ == "__main__":
    unittest.main()
