from .models import DONE, Task


class UnknownTask(KeyError):
    pass


class TaskStore:
    def __init__(self):
        self._tasks = {}
        self._next_id = 1

    def add(self, title, tags=(), due=None):
        if not title.strip():
            raise ValueError("a task needs a title")
        task = Task(self._next_id, title.strip(), list(tags), due=due)
        self._tasks[task.id] = task
        self._next_id += 1
        return task

    def get(self, task_id):
        try:
            return self._tasks[task_id]
        except KeyError:
            raise UnknownTask(task_id) from None

    def complete(self, task_id):
        task = self.get(task_id)
        task.status = DONE
        return task

    def delete(self, task_id):
        task = self.get(task_id)
        del self._tasks[task_id]
        return task

    def all(self):
        return sorted(self._tasks.values(), key=lambda t: t.id)
