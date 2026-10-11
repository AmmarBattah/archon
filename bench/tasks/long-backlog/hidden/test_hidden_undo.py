import unittest
from datetime import date

from tasktracker.store import NothingToUndo, TaskStore, UnknownTask


class UndoTest(unittest.TestCase):
    def test_nothing_to_undo_on_a_fresh_store(self):
        with self.assertRaises(NothingToUndo):
            TaskStore().undo()

    def test_undo_walks_back_across_kinds(self):
        store = TaskStore()
        a = store.add("a", tags=["x"], due=date(2026, 5, 1))
        b = store.add("b")
        store.complete(a.id)
        store.delete(a.id)
        store.undo()
        restored = store.get(a.id)
        self.assertEqual((restored.title, restored.tags, restored.status), ("a", ["x"], "done"))
        self.assertEqual(restored.due, date(2026, 5, 1))
        store.undo()
        self.assertEqual(store.get(a.id).status, "open")
        store.undo()
        with self.assertRaises(UnknownTask):
            store.get(b.id)
        store.undo()
        self.assertEqual(store.all(), [])
        with self.assertRaises(NothingToUndo):
            store.undo()

    def test_ids_are_never_reused(self):
        store = TaskStore()
        first = store.add("first")
        store.undo()
        second = store.add("second")
        self.assertNotEqual(first.id, second.id)
        self.assertEqual([t.id for t in store.all()], [second.id])

    def test_failed_operations_are_not_recorded(self):
        store = TaskStore()
        a = store.add("a")
        with self.assertRaises(UnknownTask):
            store.complete(999)
        with self.assertRaises(UnknownTask):
            store.delete(999)
        with self.assertRaises(ValueError):
            store.add("   ")
        store.undo()
        self.assertEqual(store.all(), [])
        with self.assertRaises(UnknownTask):
            store.get(a.id)

    def test_restored_task_keeps_its_position(self):
        store = TaskStore()
        a, b, c = store.add("a"), store.add("b"), store.add("c")
        store.delete(b.id)
        store.undo()
        self.assertEqual([t.id for t in store.all()], [a.id, b.id, c.id])


if __name__ == "__main__":
    unittest.main()
