import unittest

from ratelimit import KeyedLimiter, TokenBucket


class FakeClock:
    def __init__(self):
        self.now = 100.0

    def __call__(self):
        return self.now


class TokenBucketSpec(unittest.TestCase):
    def setUp(self):
        self.clock = FakeClock()
        self.bucket = TokenBucket(4, 2.0, clock=self.clock)

    def test_invalid_construction(self):
        for capacity, rate in [(0, 1), (-1, 1), (1, 0), (1, -0.5)]:
            with self.assertRaises(ValueError):
                TokenBucket(capacity, rate, clock=self.clock)

    def test_invalid_requests(self):
        for n in (0, -1, 5):
            with self.assertRaises(ValueError):
                self.bucket.try_acquire(n)
            with self.assertRaises(ValueError):
                self.bucket.wait_time(n)

    def test_failed_acquire_consumes_nothing(self):
        self.bucket.try_acquire(3)
        self.assertFalse(self.bucket.try_acquire(2))
        self.assertAlmostEqual(self.bucket.available(), 1.0)

    def test_refill_is_continuous_and_capped(self):
        self.bucket.try_acquire(4)
        self.clock.now += 0.25
        self.assertAlmostEqual(self.bucket.available(), 0.5)
        self.clock.now += 100
        self.assertAlmostEqual(self.bucket.available(), 4.0)

    def test_wait_time(self):
        self.assertEqual(self.bucket.wait_time(4), 0.0)
        self.bucket.try_acquire(4)
        self.assertAlmostEqual(self.bucket.wait_time(1), 0.5)
        self.assertAlmostEqual(self.bucket.wait_time(3), 1.5)
        self.assertAlmostEqual(self.bucket.available(), 0.0, msg="wait_time never consumes")
        self.clock.now += 0.5
        self.assertTrue(self.bucket.try_acquire(1))

    def test_clock_going_backwards_is_harmless(self):
        self.bucket.try_acquire(2)
        self.clock.now -= 50
        self.assertAlmostEqual(self.bucket.available(), 2.0)
        self.clock.now += 50.5
        self.assertAlmostEqual(self.bucket.available(), 3.0)

    def test_default_clock_works(self):
        bucket = TokenBucket(1, 1000.0)
        self.assertTrue(bucket.try_acquire())


class KeyedLimiterSpec(unittest.TestCase):
    def setUp(self):
        self.clock = FakeClock()

    def test_invalid_construction(self):
        with self.assertRaises(ValueError):
            KeyedLimiter(1, 1, max_keys=0, clock=self.clock)
        with self.assertRaises(ValueError):
            KeyedLimiter(0, 1, clock=self.clock)

    def test_lru_eviction(self):
        limiter = KeyedLimiter(1, 0.001, max_keys=2, clock=self.clock)
        self.assertTrue(limiter.try_acquire("a"))
        self.assertTrue(limiter.try_acquire("b"))
        self.assertFalse(limiter.try_acquire("a"))  # a is now most recent
        self.assertTrue(limiter.try_acquire("c"))  # evicts b
        self.assertEqual(len(limiter), 2)
        self.assertFalse(limiter.try_acquire("a"), "a was kept")
        self.assertTrue(limiter.try_acquire("b"), "b was evicted and returns full")

    def test_wait_time_counts_as_use(self):
        limiter = KeyedLimiter(1, 1.0, max_keys=2, clock=self.clock)
        limiter.try_acquire("a")
        limiter.try_acquire("b")
        self.assertAlmostEqual(limiter.wait_time("a"), 1.0)
        limiter.try_acquire("c")  # evicts b, not a
        self.assertAlmostEqual(limiter.wait_time("a"), 1.0)
        self.assertEqual(len(limiter), 2)

    def test_hashable_keys(self):
        limiter = KeyedLimiter(2, 1.0, clock=self.clock)
        self.assertTrue(limiter.try_acquire(("tenant", 7), 2))
        self.assertFalse(limiter.try_acquire(("tenant", 7)))
        with self.assertRaises(ValueError):
            limiter.try_acquire("x", 3)


if __name__ == "__main__":
    unittest.main()
