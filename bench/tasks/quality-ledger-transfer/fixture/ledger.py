"""Bounded, thread-safe ledger. See SPEC.md."""

MAX_BALANCE = 2**63 - 1


class Ledger:
    def __init__(self, balances):
        raise NotImplementedError

    def transfer(self, source, destination, amount):
        raise NotImplementedError

    def snapshot(self):
        raise NotImplementedError
