import unittest
from datetime import date

from tasktracker.dates import InvalidDate, parse_due

TODAY = date(2026, 3, 10)


class DatesTest(unittest.TestCase):
    def test_impossible_dates_raise(self):
        for text in ["2026-02-30", "2025-02-29", "2026-04-31", "2026-13-01", "2026-00-10", "2026-01-00"]:
            with self.subTest(text=text):
                with self.assertRaises(InvalidDate) as caught:
                    parse_due(text, TODAY)
                self.assertIn(text, str(caught.exception))

    def test_invalid_date_is_a_value_error(self):
        self.assertTrue(issubclass(InvalidDate, ValueError))

    def test_leap_day_is_valid(self):
        self.assertEqual(parse_due("2028-02-29", TODAY), date(2028, 2, 29))

    def test_unsupported_forms_raise_invalid_date(self):
        for text in ["", "soon", "2026/03/10", "-3d", "3d", "+d", "+3w", "26-03-10"]:
            with self.subTest(text=text):
                with self.assertRaises(InvalidDate):
                    parse_due(text, TODAY)

    def test_relative_forms(self):
        self.assertEqual(parse_due("+0d", TODAY), TODAY)
        self.assertEqual(parse_due(" +30D ", TODAY), date(2026, 4, 9))
        self.assertEqual(parse_due("TODAY", TODAY), TODAY)
        self.assertEqual(parse_due("tomorrow", date(2026, 12, 31)), date(2027, 1, 1))


if __name__ == "__main__":
    unittest.main()
