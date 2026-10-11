import unittest

from window import sliding_windows


class SlidingWindowTest(unittest.TestCase):
    def test_includes_the_last_window(self):
        self.assertEqual(sliding_windows([1, 2, 3], 2), [[1, 2], [2, 3]])

    def test_whole_list_is_one_window(self):
        self.assertEqual(sliding_windows([1, 2], 2), [[1, 2]])

    def test_rejects_non_positive_sizes(self):
        with self.assertRaises(ValueError):
            sliding_windows([1], 0)


if __name__ == "__main__":
    unittest.main()
