"""The item catalogue (in memory for this service)."""

COLOURS = ["red", "green", "blue", "black", "white", "grey"]
KINDS = ["bolt", "nut", "washer", "screw", "bracket", "hinge", "spring"]

ITEMS = [
    {
        "id": n,
        "sku": f"SKU-{n:04d}",
        "name": f"{COLOURS[n % len(COLOURS)]} {KINDS[n % len(KINDS)]}",
        "qty": (n * 37) % 101,
    }
    for n in range(57, 0, -1)
]
