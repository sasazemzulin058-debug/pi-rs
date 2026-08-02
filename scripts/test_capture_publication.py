#!/usr/bin/env python3
import importlib.util
import os
import tempfile
import unittest
from importlib.machinery import SourceFileLoader
from unittest.mock import patch

SCRIPT = os.path.join(os.path.dirname(__file__), "capture-upstream-fixtures")
loader = SourceFileLoader("capture_upstream_fixtures", SCRIPT)
spec = importlib.util.spec_from_loader(loader.name, loader)
assert spec is not None
module = importlib.util.module_from_spec(spec)
loader.exec_module(module)


class PublicationRollbackTest(unittest.TestCase):
    def test_failure_restores_existing_files(self):
        with tempfile.TemporaryDirectory() as root:
            first = os.path.join(root, "first")
            second = os.path.join(root, "second")
            staged_first = os.path.join(root, "staged-first")
            staged_second = os.path.join(root, "staged-second")
            for path, data in (
                (first, b"old first"),
                (second, b"old second"),
                (staged_first, b"new first"),
                (staged_second, b"new second"),
            ):
                with open(path, "wb") as handle:
                    handle.write(data)

            original_replace = os.replace
            calls = 0

            def fail_on_fourth(src, dst):
                nonlocal calls
                calls += 1
                if calls == 4:
                    raise OSError("simulated publication failure")
                original_replace(src, dst)

            with (
                patch.object(module.os, "replace", side_effect=fail_on_fourth),
                self.assertRaises(OSError),
            ):
                module.publish_staged([(staged_first, first), (staged_second, second)])

            with open(first, "rb") as handle:
                self.assertEqual(handle.read(), b"old first")
            with open(second, "rb") as handle:
                self.assertEqual(handle.read(), b"old second")
            self.assertFalse(os.path.exists(staged_first))
            self.assertTrue(os.path.exists(staged_second))


if __name__ == "__main__":
    unittest.main()
