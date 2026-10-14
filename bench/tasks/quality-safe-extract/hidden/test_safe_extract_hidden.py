import tempfile
import unittest
from pathlib import Path

from safe_extract import extract


class ExtractEdges(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(dir=".")
        self.addCleanup(self.scratch.cleanup)
        self.base = Path(self.scratch.name)
        self.root = self.base / "out"
        self.root.mkdir()
        self.assertTrue(self.root.is_dir())
        self.assertEqual(list(self.root.iterdir()), [])

    def test_traversal_and_noncanonical_names(self):
        bad = ["../escape", "/absolute", "a/../../escape", "a/../b", "a/./b",
               "a//b", "a/", "", ".", "..", "C:/x", "a\\b", "a\x00b", "//host/x"]
        for name in bad:
            with self.subTest(name=name), self.assertRaises(ValueError):
                extract([("valid/file", b"first"), (name, b"bad")], self.root)
            self.assertEqual(list(self.root.iterdir()), [])
        self.assertFalse((self.base / "escape").exists())

    def test_sibling_prefix_escape(self):
        sibling = self.base / "out-sibling"
        sibling.mkdir()
        with self.assertRaises(ValueError):
            extract([("../out-sibling/leak", b"bad")], self.root)
        self.assertEqual(list(sibling.iterdir()), [])
        self.assertEqual(list(self.root.iterdir()), [])

    def test_duplicate_and_prefix_collisions(self):
        for names in [("a", "a"), ("a", "a/b"), ("a/b", "a"), ("a/b/c", "a")]:
            with self.subTest(names=names), self.assertRaises(ValueError):
                extract([(name, b"data") for name in names], self.root)
            self.assertEqual(list(self.root.iterdir()), [])
        self.assertEqual(extract([("a/b", b"1"), ("a/c", b"2")], self.root), ["a/b", "a/c"])
        self.assertEqual((self.root / "a/c").read_bytes(), b"2")

    def test_existing_target_is_not_overwritten(self):
        (self.root / "kept").write_bytes(b"original")
        with self.assertRaises(ValueError):
            extract([("new/first", b"ok"), ("kept", b"changed")], self.root)
        self.assertEqual((self.root / "kept").read_bytes(), b"original")
        self.assertFalse((self.root / "new").exists())

    def test_existing_file_as_parent_and_directory_as_target(self):
        (self.root / "file").write_bytes(b"kept")
        (self.root / "directory").mkdir()
        for name in ["file/child", "directory"]:
            with self.subTest(name=name), self.assertRaises(ValueError):
                extract([("new", b"ok"), (name, b"bad")], self.root)
            self.assertFalse((self.root / "new").exists())
        self.assertEqual((self.root / "file").read_bytes(), b"kept")

    def test_symlink_parent_even_inside_root(self):
        (self.root / "real").mkdir()
        (self.root / "link").symlink_to("real", target_is_directory=True)
        with self.assertRaises(ValueError):
            extract([("link/child", b"bad")], self.root)
        self.assertEqual(list((self.root / "real").iterdir()), [])
        self.assertTrue((self.root / "link").is_symlink())

    def test_external_symlink_preserves_outside(self):
        outside = self.base / "outside"
        outside.mkdir()
        (outside / "kept").write_bytes(b"original")
        (self.root / "link").symlink_to(outside.resolve(), target_is_directory=True)
        with self.assertRaises(ValueError):
            extract([("new", b"ok"), ("link/kept", b"bad")], self.root)
        self.assertEqual((outside / "kept").read_bytes(), b"original")
        self.assertFalse((self.root / "new").exists())

    def test_dangling_symlink_and_target_symlink(self):
        (self.root / "dangling").symlink_to("absent")
        (self.root / "kept").write_bytes(b"original")
        (self.root / "link").symlink_to("kept")
        for name in ["dangling", "dangling/child", "link"]:
            with self.subTest(name=name), self.assertRaises(ValueError):
                extract([(name, b"bad")], self.root)
        self.assertFalse((self.root / "absent").exists())
        self.assertEqual((self.root / "kept").read_bytes(), b"original")

    def test_root_validation_even_for_empty_batch(self):
        alias = self.base / "alias"
        alias.symlink_to(self.root.resolve(), target_is_directory=True)
        file = self.base / "file"
        file.write_bytes(b"kept")
        for root in [alias, file, self.base / "missing", None]:
            with self.subTest(root=root), self.assertRaises(ValueError):
                extract([], root)
        self.assertFalse((self.base / "missing").exists())
        self.assertEqual(file.read_bytes(), b"kept")

    def test_malformed_batch_preflight(self):
        for entry in [None, ("x",), ("x", b"a", b"b"), (3, b"a"), ("x", "text")]:
            with self.subTest(entry=entry), self.assertRaises(ValueError):
                extract([("first", b"ok"), entry], self.root)
            self.assertEqual(list(self.root.iterdir()), [])
        with self.assertRaises(ValueError):
            extract(iter([]), self.root)

    def test_literal_percent_unicode_binary_and_existing_directory(self):
        (self.root / "existing").mkdir()
        entries = [("%2e%2e/data", bytes(range(256))), ("existing/λ", b"\x00\xff")]
        self.assertEqual(extract(entries, str(self.root)), [name for name, _ in entries])
        for name, data in entries:
            self.assertEqual((self.root / name).read_bytes(), data)
        self.assertEqual(sorted(path.name for path in self.root.iterdir()), ["%2e%2e", "existing"])
