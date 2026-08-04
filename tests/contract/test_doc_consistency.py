import tempfile
import unittest
from pathlib import Path
import runpy


ROOT = Path(__file__).resolve().parents[2]
checker = runpy.run_path(str(ROOT / "scripts/check-doc-consistency"))


class TestDocConsistency(unittest.TestCase):
    def test_current_docs_pass(self):
        self.assertEqual(checker["check_docs"](ROOT), [])

    def test_stale_session_path_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for path in ["crates/pi-coding-agent", "docs"]:
                (root / path).mkdir(parents=True, exist_ok=True)
            (root / "Cargo.toml").write_text('[workspace.package]\nversion = "0.83.0"\nrepository = "https://github.com/sasazemzulin058-debug/pi-rs"\n')
            (root / "README.md").write_text("pi-rs\nCargo.toml # workspace, version 0.83.0\nfb9be67")
            (root / "docs/compatibility-matrix.md").write_text("gemini-2.0-flash")
            (root / "crates/pi-coding-agent/README.md").write_text("$XDG_CONFIG_HOME/pi/sessions/<id>.json")
            (root / "ROADMAP.md").write_text("$XDG_CONFIG_HOME/pi-rs/sessions/<id>.jsonl $XDG_CONFIG_HOME/pi-rs/config.toml")
            (root / "CHANGELOG.md").write_text("$XDG_CONFIG_HOME/pi-rs/config.toml")
            self.assertTrue(any("stale path" in error for error in checker["check_docs"](root)))

    def test_wrong_repository_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "crates/pi-coding-agent").mkdir(parents=True)
            (root / "docs").mkdir()
            (root / "Cargo.toml").write_text('[workspace.package]\nversion = "0.83.0"\nrepository = "https://github.com/nktkt/pi"\n')
            (root / "README.md").write_text("pi-rs\nCargo.toml # workspace, version 0.83.0\nhttps://github.com/nktkt/pi\nfb9be67")
            (root / "docs/compatibility-matrix.md").write_text("gemini-2.0-flash")
            (root / "crates/pi-coding-agent/README.md").write_text("$XDG_CONFIG_HOME/pi-rs/sessions/<id>.jsonl")
            (root / "ROADMAP.md").write_text("$XDG_CONFIG_HOME/pi-rs/sessions/<id>.jsonl $XDG_CONFIG_HOME/pi-rs/config.toml")
            (root / "CHANGELOG.md").write_text("$XDG_CONFIG_HOME/pi-rs/config.toml")
            self.assertTrue(any("unexpected current repository URL" in error for error in checker["check_docs"](root)))


if __name__ == "__main__":
    unittest.main()
