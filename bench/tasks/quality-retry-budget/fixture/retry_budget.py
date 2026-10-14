"""Pure bounded retry scheduling. See SPEC.md."""


def retry_delay(attempt, elapsed_ms, *, max_attempts=4, budget_ms=1000,
                base_ms=100, cap_ms=400, retry_after_ms=None):
    raise NotImplementedError
