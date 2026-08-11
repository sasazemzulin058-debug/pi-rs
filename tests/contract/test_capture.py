import unittest
import sys
import os
import json
import tempfile
import shutil
import subprocess
import importlib.util

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(__file__))), 'scripts'))

from importlib.machinery import SourceFileLoader

script_path = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(__file__))), 'scripts', 'capture-upstream-fixtures')
# ponytail: load_module deprecated in Python 3.12, upgrade to importlib.util.module_from_spec when script gets .py extension
capture_upstream_fixtures = SourceFileLoader("capture_upstream_fixtures", script_path).load_module()

capture_main = capture_upstream_fixtures.main
validate_preflight = capture_upstream_fixtures.validate_preflight

def mock_adapter(upstream_root):
    return {"case_id": "cli.print.basic", "oracle": "upstream-pi", "expected": {"status": "ok"}}

class TestCapture(unittest.TestCase):
    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        self.fake_upstream = os.path.join(self.tmpdir, "fake_upstream")
        os.makedirs(os.path.join(self.fake_upstream, "packages", "coding-agent"), exist_ok=True)
        os.makedirs(os.path.join(self.fake_upstream, "packages", "ai"), exist_ok=True)

        # Init git repo in fake upstream
        subprocess.run(["git", "init"], cwd=self.fake_upstream, capture_output=True, check=True)
        subprocess.run(["git", "config", "user.name", "Test"], cwd=self.fake_upstream, check=True)
        subprocess.run(["git", "config", "user.email", "test@example.com"], cwd=self.fake_upstream, check=True)

        with open(os.path.join(self.fake_upstream, "packages", "coding-agent", "package.json"), "w") as f:
            json.dump({"version": "0.83.0"}, f)
        with open(os.path.join(self.fake_upstream, "packages", "ai", "package.json"), "w") as f:
            json.dump({"version": "0.83.0"}, f)
        with open(os.path.join(self.fake_upstream, "package-lock.json"), "w") as f:
            f.write("dummy lockfile content\n")

        subprocess.run(["git", "add", "."], cwd=self.fake_upstream, capture_output=True, check=True)
        subprocess.run(["git", "commit", "-m", "initial"], cwd=self.fake_upstream, capture_output=True, check=True)

        self.head_commit = subprocess.run(["git", "rev-parse", "HEAD"], cwd=self.fake_upstream, capture_output=True, text=True, check=True).stdout.strip()

        import hashlib
        self.lock_sha = hashlib.sha256(b"dummy lockfile content\n").hexdigest()

    def tearDown(self):
        shutil.rmtree(self.tmpdir)

    def test_preflight_valid(self):
        cases = ["cli.print.basic"]
        cases_cat = {"cli.print.basic": {"oracle": "upstream-pi"}}
        adapters = {"cli.print.basic": mock_adapter}

        res = validate_preflight(
            self.fake_upstream,
            self.head_commit,
            "0.83.0",
            self.lock_sha,
            cases,
            cases_cat,
            adapters=adapters
        )
        self.assertEqual(res["git_head"], self.head_commit)
        self.assertEqual(res["lockfile_sha256"], self.lock_sha)

    def test_preflight_dirty_tree_fails(self):
        with open(os.path.join(self.fake_upstream, "packages", "ai", "package.json"), "w") as f:
            json.dump({"version": "0.83.1"}, f)

        cases = ["cli.print.basic"]
        cases_cat = {"cli.print.basic": {"oracle": "upstream-pi"}}
        adapters = {"cli.print.basic": mock_adapter}

        with self.assertRaises(ValueError) as ctx:
            validate_preflight(
                self.fake_upstream,
                self.head_commit,
                "0.83.0",
                self.lock_sha,
                cases,
                cases_cat,
                adapters=adapters
            )
        self.assertIn("dirty", str(ctx.exception).lower())

    def test_staging_rejects_non_empty_directory(self):
        out_dir = os.path.join(self.tmpdir, "staged_output")
        os.makedirs(out_dir)
        with open(os.path.join(out_dir, "existing.txt"), "w") as f:
            f.write("existing")
        with self.assertRaises(SystemExit) as ctx:
            capture_main(argv=["--milestone", "M1a", "--staging-dir", out_dir], adapters={})
        self.assertEqual(ctx.exception.code, 2)

    def test_target_staging_with_output_directory(self):
        out_dir = os.path.join(self.tmpdir, "staged_output")
        env = dict(os.environ, PI_UPSTREAM_ROOT=self.fake_upstream)
        # ponytail: stage target capture without publishing committed fixtures
        argv = [
            "--milestone", "M1a",
            "--staging-dir", out_dir,
            "--reference-version", "0.83.0",
            "--reference-commit", self.head_commit,
            "--reference-lockfile-sha256", self.lock_sha
        ]

        old_env = os.environ.get("PI_UPSTREAM_ROOT")
        os.environ["PI_UPSTREAM_ROOT"] = self.fake_upstream
        try:
            adapters = dict(capture_upstream_fixtures.ADAPTERS)
            adapters.update({cid: mock_adapter for cid in adapters})
            capture_main(argv=argv, adapters=adapters)
        except SystemExit as e:
            self.assertEqual(e.code, 0)
        finally:
            if old_env is not None:
                os.environ["PI_UPSTREAM_ROOT"] = old_env
            else:
                os.environ.pop("PI_UPSTREAM_ROOT", None)

        self.assertTrue(os.path.exists(os.path.join(out_dir, "capture-report.json")))
        self.assertFalse(os.path.exists(os.path.join(os.path.dirname(capture_upstream_fixtures.MANIFEST_PATH), "manifest.json.bak")))
        self.assertTrue(os.path.exists(os.path.join(out_dir, "manifest.json")))
        with open(os.path.join(out_dir, "manifest.json"), "r") as f:
            m = json.load(f)
            self.assertEqual(m["reference"]["version"], "0.83.0")
            self.assertEqual(m["reference"]["commit"], self.head_commit)
        with open(os.path.join(out_dir, "capture-report.json"), "r") as f:
            self.assertEqual(json.load(f)["mode"], "staging")

if __name__ == "__main__":
    unittest.main()
