import unittest

from retry_budget import retry_delay


class RetryBasics(unittest.TestCase):
    def test_backoff(self):
        self.assertEqual(retry_delay(1, 0), 100)
        self.assertEqual(retry_delay(2, 100), 200)
        self.assertEqual(retry_delay(3, 300), 400)

    def test_attempt_limit(self):
        self.assertIsNone(retry_delay(4, 0))
        self.assertIsNone(retry_delay(1, 0, max_attempts=1))

    def test_retry_after(self):
        self.assertEqual(retry_delay(1, 0, retry_after_ms=250), 250)
        self.assertEqual(retry_delay(1, 0, retry_after_ms=25), 100)
