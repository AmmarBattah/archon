# Rate limiter specification

`ratelimit.py` provides two classes. Time is read only through the injected
`clock` callable (seconds as a float, monotonic), so tests can control it.

## `TokenBucket(capacity, refill_per_sec, clock=time.monotonic)`

- `capacity` (> 0) is the maximum number of tokens; `refill_per_sec` (> 0) is
  the rate tokens are added. Either being `<= 0` raises `ValueError`.
- A new bucket starts **full**.
- Tokens accrue continuously (fractional tokens are allowed) and never exceed
  `capacity`.
- `try_acquire(n=1) -> bool`: if at least `n` tokens are available, remove
  them and return `True`; otherwise remove nothing and return `False`.
  `n <= 0` or `n > capacity` raises `ValueError` (such a request can never be
  meaningful).
- `available() -> float`: the tokens available now, after refill.
- `wait_time(n=1) -> float`: seconds until `try_acquire(n)` would succeed
  (`0.0` if it would succeed now). Does not consume tokens. Same `ValueError`
  rules as `try_acquire`.
- If the clock goes backwards, nothing changes: no tokens are added or
  removed until the clock passes the latest time the bucket has already seen.

## `KeyedLimiter(capacity, refill_per_sec, max_keys=1024, clock=time.monotonic)`

One independent `TokenBucket` per key (any hashable), created on first use.

- `try_acquire(key, n=1) -> bool` and `wait_time(key, n=1) -> float` behave
  like the bucket methods for that key.
- At most `max_keys` buckets are kept. When a new key would exceed the limit,
  the **least recently used** key is evicted first (both `try_acquire` and
  `wait_time` count as a use). An evicted key that returns starts full.
- `max_keys < 1` raises `ValueError`, as do invalid `capacity` / `refill_per_sec`.
- `len(limiter)` is the number of keys currently tracked.
