import unittest

from ledger import Ledger


class LedgerBasics(unittest.TestCase):
    def test_transfer(self):
        ledger = Ledger({"a": 12, "b": 3})
        self.assertIs(ledger.transfer("a", "b", 5), True)
        self.assertEqual(ledger.snapshot(), {"a": 7, "b": 8})

    def test_insufficient_funds(self):
        ledger = Ledger({"a": 2, "b": 0})
        self.assertIs(ledger.transfer("a", "b", 3), False)
        self.assertEqual(ledger.snapshot(), {"a": 2, "b": 0})
