import tempfile
import unittest
from pathlib import Path

from safe_extract import extract


class ExtractBasics(unittest.TestCase):
    def test_nested_files(self):
        with tempfile.TemporaryDirectory(dir=".") as directory:
            root = Path(directory)
            entries = [("docs/readme.txt", b"hello"), ("empty", b"")]
            self.assertEqual(extract(entries, root), ["docs/readme.txt", "empty"])
            self.assertEqual((root / "docs/readme.txt").read_bytes(), b"hello")
            self.assertEqual((root / "empty").read_bytes(), b"")

    def test_empty_batch(self):
        with tempfile.TemporaryDirectory(dir=".") as directory:
            self.assertEqual(extract([], Path(directory)), [])
            self.assertEqual(list(Path(directory).iterdir()), [])
