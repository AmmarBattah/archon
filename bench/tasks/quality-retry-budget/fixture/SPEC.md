# Retry budget

Implement the pure function:

`retry_delay(attempt, elapsed_ms, *, max_attempts=4, budget_ms=1000,
base_ms=100, cap_ms=400, retry_after_ms=None) -> int | None`

All times are integer milliseconds. `attempt` is the number of attempts already
made (the initial attempt is number 1); `elapsed_ms` is total elapsed time at the
moment that attempt failed. Return the wait before the next attempt, or None
when no further attempt may start. Never sleep, read a clock, or mutate inputs.

1. Validate ALL arguments before any exhausted-budget or exhausted-attempt
   return. Every number must be a built-in int (not bool or float). Attempt and
   max_attempts are at least 1. Elapsed, budget, base, cap, and a non-None
   retry_after are at least 0. Invalid values raise ValueError. There is no
   upper integer limit. base_ms may exceed cap_ms.
2. If attempt >= max_attempts, return None. max_attempts counts the initial
   attempt, not just retries; attempt greater than max_attempts is legal.
3. The backoff is `min(cap_ms, base_ms * 2**(attempt - 1))`. A zero base always
   yields zero backoff; a zero cap also yields zero. Saturate efficiently: huge
   attempt values must not allocate an enormous intermediate power of two.
4. A server Retry-After is a minimum wait: if supplied, use the greater of
   backoff and retry_after_ms. This floor is NOT limited by cap_ms.
5. The next attempt must start strictly BEFORE budget_ms. Return None when
   elapsed_ms + delay >= budget_ms, otherwise return delay (including zero).
   Never shorten the required wait to squeeze a retry into the budget.

No randomness, network, clock, or third-party packages are needed.
