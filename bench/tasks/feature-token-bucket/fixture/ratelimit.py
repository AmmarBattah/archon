"""Token-bucket rate limiting. See SPEC.md."""

import time


class TokenBucket:
    def __init__(self, capacity, refill_per_sec, clock=time.monotonic):
        raise NotImplementedError

    def try_acquire(self, n=1):
        raise NotImplementedError

    def available(self):
        raise NotImplementedError

    def wait_time(self, n=1):
        raise NotImplementedError


class KeyedLimiter:
    def __init__(self, capacity, refill_per_sec, max_keys=1024, clock=time.monotonic):
        raise NotImplementedError

    def try_acquire(self, key, n=1):
        raise NotImplementedError

    def wait_time(self, key, n=1):
        raise NotImplementedError

    def __len__(self):
        raise NotImplementedError
