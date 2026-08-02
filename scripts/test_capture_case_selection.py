import contextlib
import io
import os
import sys
import tempfile
import unittest
from unittest.mock import patch

from test_capture_publication import module


class CaptureCaseSelectionTest(unittest.TestCase):
    def manifest(self):
        return {
            "requiredCaseIds": {
                "M1": ["missing", "tool.edit"],
                "M0": [],
                "M1a": [],
                "M2": [],
                "M3": [],
            },
            "cases": {
                "missing": {"oracle": "upstream-pi", "captured": False},
                "tool.edit": {"oracle": "upstream-pi", "captured": False},
            },
            "captureEnvironment": {},
        }

    def run_main(self, case=None, adapters=None):
        argv = ["capture-upstream-fixtures", "--milestone", "M1"]
        if case:
            argv += ["--case", case]
        published = []
        manifest = self.manifest()
        with (
            tempfile.TemporaryDirectory() as root,
            patch.object(module, "load_manifest", return_value=manifest),
            patch.object(module, "ADAPTERS", adapters or {}),
            patch.object(
                module,
                "publish_staged",
                side_effect=lambda updates: published.append(manifest),
            ),
            patch.dict(os.environ, {"PI_UPSTREAM_ROOT": root}),
            patch.object(sys, "argv", argv),
            contextlib.redirect_stdout(io.StringIO()),
            contextlib.redirect_stderr(io.StringIO()),
            patch.object(sys, "argv", argv),
        ):
            module.main()
        return published, manifest

    def test_unknown_case_rejected(self):
        with (
            patch.object(
                sys,
                "argv",
                ["capture-upstream-fixtures", "--milestone", "M1", "--case", "unknown"],
            ),
            patch.object(module, "load_manifest", return_value=self.manifest()),
            self.assertRaises(SystemExit) as caught,
        ):
            module.main()
        self.assertEqual(caught.exception.code, 1)

    def test_selected_case_bypasses_missing_sibling(self):
        published, manifest = self.run_main(
            "tool.edit", {"tool.edit": lambda _: {"ok": True}}
        )
        self.assertTrue(published)
        self.assertEqual(
            manifest["captureEnvironment"]["captureScope"]["caseIds"], ["tool.edit"]
        )
        self.assertEqual(manifest["captureEnvironment"]["captureStatus"], "completed")

    def test_default_mode_still_fails_missing_adapter(self):
        with self.assertRaises(SystemExit) as caught:
            self.run_main(None, {"tool.edit": lambda _: {"ok": True}})
        self.assertEqual(caught.exception.code, 1)

    def test_out_of_milestone_case_rejected(self):
        manifest = self.manifest()
        manifest["requiredCaseIds"]["M0"] = ["other.case"]
        manifest["cases"]["other.case"] = {"oracle": "upstream-pi", "captured": False}
        with (
            patch.object(
                sys,
                "argv",
                [
                    "capture-upstream-fixtures",
                    "--milestone",
                    "M1",
                    "--case",
                    "other.case",
                ],
            ),
            patch.object(module, "load_manifest", return_value=manifest),
            self.assertRaises(SystemExit) as caught,
        ):
            module.main()
        self.assertEqual(caught.exception.code, 1)

    def test_out_of_oracle_case_rejected(self):
        manifest = self.manifest()
        manifest["cases"]["tool.edit"]["oracle"] = "pi-rs-invariant"
        with (
            patch.object(
                sys,
                "argv",
                [
                    "capture-upstream-fixtures",
                    "--milestone",
                    "M1",
                    "--case",
                    "tool.edit",
                ],
            ),
            patch.object(module, "load_manifest", return_value=manifest),
            self.assertRaises(SystemExit) as caught,
        ):
            module.main()
        self.assertEqual(caught.exception.code, 1)


if __name__ == "__main__":
    unittest.main()
