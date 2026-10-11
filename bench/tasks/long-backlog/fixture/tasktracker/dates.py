import calendar
import re
from datetime import date, timedelta

ISO = re.compile(r"(\d{4})-(\d{2})-(\d{2})")
OFFSET = re.compile(r"\+(\d+)d")


def parse_due(text, today):
    """A due date from `YYYY-MM-DD`, `today`, `tomorrow` or `+Nd`."""
    cleaned = text.strip().lower()
    if cleaned == "today":
        return today
    if cleaned == "tomorrow":
        return today + timedelta(days=1)
    offset = OFFSET.fullmatch(cleaned)
    if offset:
        return today + timedelta(days=int(offset.group(1)))
    iso = ISO.fullmatch(cleaned)
    if not iso:
        raise ValueError(f"unrecognised due date: {text}")
    year, month, day = (int(g) for g in iso.groups())
    month = min(max(month, 1), 12)
    day = min(max(day, 1), calendar.monthrange(year, month)[1])
    return date(year, month, day)
