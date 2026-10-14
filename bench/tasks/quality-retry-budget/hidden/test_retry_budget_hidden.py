import unittest

from retry_budget import retry_delay


class RetryEdges(unittest.TestCase):
    def test_strict_deadline(self):
        self.assertIsNone(retry_delay(1, 900))
        self.assertIsNone(retry_delay(1, 901))
        self.assertEqual(retry_delay(1, 899), 100)

    def test_retry_after_can_exceed_cap(self):
        self.assertEqual(retry_delay(1, 0, retry_after_ms=600), 600)
        self.assertIsNone(retry_delay(1, 400, retry_after_ms=600))
        self.assertIsNone(retry_delay(1, 0, retry_after_ms=1001))

    def test_no_budget_clamping(self):
        self.assertIsNone(retry_delay(3, 700))
        self.assertEqual(retry_delay(3, 599), 400)
        self.assertIsNone(retry_delay(3, 600))

    def test_zero_delay_still_needs_remaining_budget(self):
        self.assertEqual(retry_delay(1, 9, base_ms=0, budget_ms=10), 0)
        self.assertIsNone(retry_delay(1, 10, base_ms=0, budget_ms=10))
        self.assertIsNone(retry_delay(1, 0, base_ms=0, budget_ms=0))
        self.assertEqual(retry_delay(1, 0, cap_ms=0, retry_after_ms=3), 3)

    def test_cap_below_base_and_exact_saturation(self):
        self.assertEqual(retry_delay(1, 0, base_ms=50, cap_ms=7), 7)
        self.assertEqual(retry_delay(3, 0, base_ms=3, cap_ms=13), 12)
        self.assertEqual(retry_delay(4, 0, max_attempts=5, base_ms=3, cap_ms=13), 13)

    def test_attempt_count_includes_first(self):
        self.assertEqual(retry_delay(1, 0, max_attempts=2), 100)
        self.assertIsNone(retry_delay(2, 0, max_attempts=2))
        self.assertIsNone(retry_delay(99, 0, max_attempts=2))

    def test_huge_attempts_saturate_without_huge_powers(self):
        attempt = 10**30
        self.assertEqual(retry_delay(attempt, 0, max_attempts=attempt + 1), 400)
        self.assertEqual(retry_delay(attempt, 0, max_attempts=attempt + 1, base_ms=0), 0)
        self.assertEqual(retry_delay(attempt, 0, max_attempts=attempt + 1, cap_ms=0), 0)

    def test_arbitrary_precision_times(self):
        huge = 10**100
        self.assertEqual(retry_delay(3, huge, budget_ms=huge * 10,
                                    base_ms=huge, cap_ms=huge * 3), huge * 3)
        self.assertIsNone(retry_delay(1, huge, budget_ms=huge * 2,
                                     base_ms=huge, cap_ms=huge))

    def test_validation_precedes_exhaustion(self):
        for field in ["attempt", "max_attempts", "elapsed_ms", "budget_ms",
                      "base_ms", "cap_ms", "retry_after_ms"]:
            for value in [-1, True, 1.0, "1"]:
                args = dict(attempt=4, elapsed_ms=1000, max_attempts=4,
                            budget_ms=1000, base_ms=100, cap_ms=400,
                            retry_after_ms=None)
                args[field] = value
                with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                    retry_delay(**args)
        with self.assertRaises(ValueError):
            retry_delay(0, 0)
        with self.assertRaises(ValueError):
            retry_delay(1, 0, max_attempts=0)

    def test_none_only_allowed_for_retry_after(self):
        for field in ["attempt", "max_attempts", "elapsed_ms", "budget_ms", "base_ms", "cap_ms"]:
            args = dict(attempt=1, elapsed_ms=0)
            args[field] = None
            with self.subTest(field=field), self.assertRaises(ValueError):
                retry_delay(**args)
        self.assertEqual(retry_delay(1, 0, retry_after_ms=None), 100)
        self.assertEqual(retry_delay(1, 0, retry_after_ms=0), 100)

    def test_small_domain_matches_direct_formula(self):
        for attempt in range(1, 7):
            for elapsed in [0, 3, 9, 10]:
                for base in [0, 1, 4]:
                    for cap in [0, 3, 7]:
                        for floor in [None, 0, 5, 12]:
                            delay = max(min(cap, base * 2**(attempt - 1)), floor or 0)
                            expected = delay if attempt < 6 and elapsed + delay < 10 else None
                            actual = retry_delay(attempt, elapsed, max_attempts=6,
                                                 budget_ms=10, base_ms=base,
                                                 cap_ms=cap, retry_after_ms=floor)
                            self.assertEqual(actual, expected)
                            self.assertTrue(actual is None or type(actual) is int)
