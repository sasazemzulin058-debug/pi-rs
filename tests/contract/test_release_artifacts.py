import hashlib
import json
import subprocess
import tarfile
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TARGETS = ("aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu")


class TestReleaseArtifacts(unittest.TestCase):
    def test_generate_sbom_and_verify_relative_dist(self):
        with tempfile.TemporaryDirectory() as tmp:
            dist = Path(tmp) / "dist"
            dist.mkdir()
            for target in TARGETS:
                sbom = dist / f"release-sbom-{target}.spdx.json"
                sbom.write_text('{"spdxVersion":"SPDX-2.3","packages":[{"name":"pi-rs"}]}')
                archive = dist / f"pi-rs-{target}.tar.gz"
                with tarfile.open(archive, "w:gz") as out:
                    for name, data, mode in ((f"pi-rs-{target}/pi-rs", b"bin", 0o755), (f"pi-rs-{target}/README.md", b"readme", 0o644), (f"pi-rs-{target}/LICENSE", b"license", 0o644)):
                        info = tarfile.TarInfo(name); info.size = len(data); info.mode = mode; out.addfile(info, __import__("io").BytesIO(data))
            artifacts = {}
            for path in sorted(dist.iterdir()):
                artifacts[path.name] = {"sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "size": path.stat().st_size}
            (dist / "release-manifest.json").write_text(json.dumps({"version": "v1.2.0", "artifacts": artifacts}))
            artifacts["release-manifest.json"] = {"sha256": hashlib.sha256((dist / "release-manifest.json").read_bytes()).hexdigest(), "size": (dist / "release-manifest.json").stat().st_size}
            (dist / "SHA256SUMS").write_text("\n".join(f"{info['sha256']}  {name}" for name, info in sorted(artifacts.items())))
            result = subprocess.run([str(ROOT / "scripts/verify-release-artifacts"), str(dist)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
