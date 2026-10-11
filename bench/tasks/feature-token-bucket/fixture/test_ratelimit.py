import unittest

from ratelimit import KeyedLimiter, TokenBucket


class FakeClock:
    def __init__(self):
        self.now = 0.0

    def __call__(self):
        return self.now


class BasicTest(unittest.TestCase):
    def test_bucket_starts_full_and_drains(self):
        clock = FakeClock()
        bucket = TokenBucket(3, 1.0, clock=clock)
        self.assertTrue(bucket.try_acquire())
        self.assertTrue(bucket.try_acquire(2))
        self.assertFalse(bucket.try_acquire())

    def test_bucket_refills_over_time(self):
        clock = FakeClock()
        bucket = TokenBucket(2, 2.0, clock=clock)
        bucket.try_acquire(2)
        clock.now = 0.5
        self.assertTrue(bucket.try_acquire())

    def test_keys_are_independent(self):
        limiter = KeyedLimiter(1, 1.0, clock=FakeClock())
        self.assertTrue(limiter.try_acquire("a"))
        self.assertTrue(limiter.try_acquire("b"))
        self.assertFalse(limiter.try_acquire("a"))


if __name__ == "__main__":
    unittest.main()
