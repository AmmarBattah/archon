from dataclasses import dataclass, field
from datetime import date
from typing import Optional

OPEN = "open"
DONE = "done"


@dataclass
class Task:
    id: int
    title: str
    tags: list = field(default_factory=list)
    status: str = OPEN
    due: Optional[date] = None
