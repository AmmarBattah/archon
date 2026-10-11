import unittest

from window import sliding_windows


class SlidingWindowEdgeCases(unittest.TestCase):
    def test_empty_input_has_no_windows(self):
        self.assertEqual(sliding_windows([], 3), [])

    def test_window_longer_than_input(self):
        self.assertEqual(sliding_windows([1, 2], 3), [])

    def test_size_one(self):
        self.assertEqual(sliding_windows([4, 5, 6], 1), [[4], [5], [6]])

    def test_every_window_counted(self):
        items = list(range(10))
        windows = sliding_windows(items, 4)
        self.assertEqual(len(windows), 7)
        self.assertEqual(windows[-1], [6, 7, 8, 9])

    def test_negative_size_rejected(self):
        with self.assertRaises(ValueError):
            sliding_windows([1, 2], -1)


if __name__ == "__main__":
    unittest.main()
