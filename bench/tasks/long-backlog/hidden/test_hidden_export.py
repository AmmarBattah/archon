import csv
import io
import unittest
from datetime import date

from tasktracker.export import to_csv
from tasktracker.models import Task


class ExportTest(unittest.TestCase):
    def test_plain_rows_are_unquoted_with_crlf(self):
        tasks = [Task(1, "plain", ["a", "b"], "open", date(2026, 4, 1)), Task(2, "other")]
        self.assertEqual(
            to_csv(tasks),
            "id,title,tags,status,due\r\n1,plain,a;b,open,2026-04-01\r\n2,other,,open,\r\n",
        )

    def test_special_characters_are_quoted(self):
        tasks = [
            Task(1, "a, b"),
            Task(2, 'say "hi"'),
            Task(3, "two\nlines"),
            Task(4, "carriage\rreturn"),
            Task(5, "tags", ["x,y", "z"]),
        ]
        lines = to_csv(tasks).split("\r\n")
        self.assertEqual(lines[1], '1,"a, b",,open,')
        self.assertEqual(lines[2], '2,"say ""hi""",,open,')
        self.assertEqual(lines[3], '3,"two\nlines",,open,')
        self.assertEqual(lines[4], '4,"carriage\rreturn",,open,')
        self.assertEqual(lines[5], '5,tags,"x,y;z",open,')

    def test_round_trips_through_the_csv_module(self):
        tasks = [Task(7, 'mixed, "all"\r\nof it', ["t"], "done", date(2026, 1, 2))]
        rows = list(csv.reader(io.StringIO(to_csv(tasks), newline="")))
        self.assertEqual(rows[1], ["7", 'mixed, "all"\r\nof it', "t", "done", "2026-01-02"])

    def test_empty_list_is_just_the_header(self):
        self.assertEqual(to_csv([]), "id,title,tags,status,due\r\n")


if __name__ == "__main__":
    unittest.main()
