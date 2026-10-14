import concurrent.futures
import threading
import unittest

from ledger import Ledger, MAX_BALANCE


class LedgerEdges(unittest.TestCase):
    def test_overflow_rolls_back(self):
        ledger = Ledger({"a": 4, "b": MAX_BALANCE - 1})
        self.assertIs(ledger.transfer("a", "b", 2), False)
        self.assertEqual(ledger.snapshot(), {"a": 4, "b": MAX_BALANCE - 1})
        self.assertIs(ledger.transfer("a", "b", 1), True)
        self.assertEqual(ledger.snapshot(), {"a": 3, "b": MAX_BALANCE})

    def test_self_transfer_at_ceiling(self):
        ledger = Ledger({"a": MAX_BALANCE, "b": 0})
        self.assertIs(ledger.transfer("a", "a", MAX_BALANCE), True)
        self.assertIs(ledger.transfer("b", "b", 1), False)
        self.assertEqual(ledger.snapshot(), {"a": MAX_BALANCE, "b": 0})

    def test_unknown_destination_is_atomic(self):
        ledger = Ledger({"a": 4})
        for source, destination in [("a", "missing"), ("missing", "a")]:
            with self.assertRaises(KeyError):
                ledger.transfer(source, destination, 2)
            self.assertEqual(ledger.snapshot(), {"a": 4})

    def test_invalid_amounts_preserve_state(self):
        ledger = Ledger({"a": 5, "b": 0})
        for amount in [0, -1, True, False, 1.0, "1", None, MAX_BALANCE + 1]:
            with self.subTest(amount=amount):
                with self.assertRaises(ValueError):
                    ledger.transfer("a", "b", amount)
                self.assertEqual(ledger.snapshot(), {"a": 5, "b": 0})
        with self.assertRaises(ValueError):
            ledger.transfer("missing", "missing", 0)

    def test_invalid_initial_state(self):
        for balances in [[], None, {"": 0}, {1: 0}, {"a": True}, {"a": -1},
                         {"a": MAX_BALANCE + 1}, {"a": 1.0}, {"a": "1"}]:
            with self.subTest(balances=balances), self.assertRaises(ValueError):
                Ledger(balances)
        self.assertEqual(Ledger({}).snapshot(), {})
        self.assertEqual(Ledger({"a": 0}).snapshot(), {"a": 0})

    def test_snapshot_and_input_ownership(self):
        initial = {"a": 5, "b": 0}
        ledger = Ledger(initial)
        initial["a"] = 99
        view = ledger.snapshot()
        view["b"] = 99
        self.assertEqual(ledger.snapshot(), {"a": 5, "b": 0})
        self.assertIs(ledger.transfer("a", "b", 1), True)
        self.assertEqual(view, {"a": 5, "b": 99})

    def test_parallel_spending_has_exact_success_count(self):
        ledger = Ledger({"source": 100, **{str(i): 0 for i in range(8)}})
        start = threading.Barrier(8)

        def spend(index):
            start.wait(timeout=5)
            outcomes = [ledger.transfer("source", str(index), 1) for _ in range(40)]
            self.assertTrue(all(type(outcome) is bool for outcome in outcomes))
            self.assertGreaterEqual(ledger.snapshot()[str(index)], 0)
            return sum(outcomes)

        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
            successes = list(pool.map(spend, range(8)))
        balances = ledger.snapshot()
        self.assertEqual(sum(successes), 100)
        self.assertEqual(balances["source"], 0)
        self.assertEqual(sum(balances.values()), 100)
        self.assertEqual([balances[str(i)] for i in range(8)], successes)

    def test_opposite_transfers_and_snapshots_preserve_total(self):
        ledger = Ledger({"a": 1000, "b": 1000})
        start = threading.Barrier(3)

        def move(pair):
            start.wait(timeout=5)
            outcomes = [ledger.transfer(*pair, 1) for _ in range(200)]
            self.assertTrue(all(outcomes))
            self.assertEqual(sum(ledger.snapshot().values()), 2000)

        def observe():
            start.wait(timeout=5)
            for _ in range(400):
                balances = ledger.snapshot()
                self.assertEqual(sum(balances.values()), 2000)
                self.assertTrue(all(0 <= value <= MAX_BALANCE for value in balances.values()))

        with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
            jobs = [pool.submit(move, pair) for pair in [("a", "b"), ("b", "a")]]
            jobs.append(pool.submit(observe))
            for job in jobs:
                job.result(timeout=5)
        self.assertEqual(ledger.snapshot(), {"a": 1000, "b": 1000})
