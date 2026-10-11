def sliding_windows(items, size):
    """Every contiguous window of `size` items, in order."""
    if size <= 0:
        raise ValueError("size must be positive")
    return [items[i : i + size] for i in range(len(items) - size)]
