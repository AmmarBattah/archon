# Atomic ledger

Implement `Ledger(balances)`, `transfer(source, destination, amount) -> bool`,
and `snapshot() -> dict`. `MAX_BALANCE = 2**63 - 1` is a per-account ceiling.

- Construction accepts a dict of nonempty string account names to built-in
  integers in `[0, MAX_BALANCE]`. Empty ledgers are allowed. Invalid names,
  balances, or a non-dict input raise ValueError. Booleans are not integers
  for this API. Copy the input; neither it nor returned snapshots may alias
  internal mutable state. Accounts cannot be created by transfers.
- Amount must be a built-in integer in `[1, MAX_BALANCE]`, otherwise ValueError.
  Validate it before looking up accounts. Either unknown account raises KeyError.
- Return False for insufficient source funds or destination overflow. Every
  failure leaves all balances unchanged. A successful transfer returns True,
  debits and credits exactly the amount, and preserves the total balance.
- A transfer to the same account returns True iff that account has sufficient
  funds and never changes it (even at MAX_BALANCE); still validate the amount
  and account first.
- All calls on one ledger must be linearizable across threads: source checks,
  overflow checks, and both writes form one indivisible operation; snapshots
  must never see half a transfer. Opposite transfers must not deadlock.

Tests use no external services, wall-clock calculations, or random inputs.
