import unittest
import sys
import os
import json
import tempfile
import shutil
import subprocess

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(__file__))), 'scripts'))

from contract_fixture_lib import normalize_structure, compare_structures, canonical_json_sha256, validate_expected_envelope, validate_manifest  # type: ignore

class TestComparator(unittest.TestCase):
    def test_invariant_envelope_and_digest_helpers(self):
        value = {"case_id": "x", "oracle": "pi-rs-invariant", "expected": {"ok": True}}
        self.assertIsNone(validate_expected_envelope(value, "x", "pi-rs-invariant"))
        self.assertEqual(canonical_json_sha256(value), canonical_json_sha256(json.loads(json.dumps(value))))
        self.assertIsNotNone(validate_expected_envelope({**value, "extra": 1}, "x", "pi-rs-invariant"))
    def setUp(self):
        # Base expected structure
        self.expected = {
            "uuid": "1a2b3c4d-5e6f-7a8b-9c0d-1e2f3a4b5c6d",
            "timestamp": "2026-07-27T12:34:56.789Z",
            "parent_id": "00000000-1111-2222-3333-444455556666",
            "messages": [
                {
                    "role": "user",
                    "content": "Hello"
                },
                {
                    "role": "assistant",
                    "stop_reason": "stop",
                    "content": "Hi",
                    "tool_calls": [
                        {"id": "call_1", "type": "function", "function": {"name": "read"}},
                        {"id": "call_2", "type": "function", "function": {"name": "bash"}}
                    ]
                }
            ]
        }

    def test_normalization_allowlist_filtering(self):
        obj = {
            "uuid": "1a2b3c4d-5e6f-7a8b-9c0d-1e2f3a4b5c6d",
            "timestamp": "2026-07-27T12:34:56.789Z",
        }
        # When allowlist includes /uuid, /uuid is normalized but /timestamp is untouched
        norm = normalize_structure(obj, allowlist=["/uuid"])
        self.assertEqual(norm["uuid"], "00000000-0000-0000-0000-000000000000")
        self.assertEqual(norm["timestamp"], "2026-07-27T12:34:56.789Z")

    def test_invariant_cli_completeness_and_digest_gate(self):
        root = os.path.dirname(os.path.dirname(os.path.dirname(__file__)))
        with open(os.path.join(root, "fixtures/upstream-pi/manifest.json"), encoding="utf-8") as f:
            manifest = json.load(f)
        invariant = [cid for cid in manifest["requiredCaseIds"]["M1a"] if manifest["cases"][cid]["oracle"] == "pi-rs-invariant"]
        with tempfile.TemporaryDirectory() as tmpdir:
            for cid in invariant:
                with open(os.path.join(root, "fixtures/upstream-pi", f"{cid}.expected.json"), encoding="utf-8") as f:
                    expected = json.load(f)
                with open(os.path.join(tmpdir, f"{cid}.actual.json"), "w", encoding="utf-8") as f:
                    json.dump(expected["expected"], f)
            command = ["python3", os.path.join(root, "scripts/compare-contract-fixtures"), "--milestone", "M1a", "--invariant-only", "--actual", tmpdir]
            self.assertEqual(subprocess.run(command, capture_output=True, text=True).returncode, 0)
            os.remove(os.path.join(tmpdir, f"{invariant[0]}.actual.json"))
            failed = subprocess.run(command, capture_output=True, text=True)
            self.assertNotEqual(failed.returncode, 0)
            self.assertIn("actual output file missing", failed.stdout)

    def test_normalization_specific_keys(self):
        # Verify that specific keys are normalized
        obj = {
            "timestamp": "2026-07-27T12:34:56.789Z",
            "uuid": "1a2b3c4d-5e6f-7a8b-9c0d-1e2f3a4b5c6d",
            "temp_path": "/data/data/com.termux/files/usr/tmp/test.txt",
            # This should NOT be normalized globally as it is not a targeted key
            "content": "This contains a path /tmp/test.txt and date 2026-07-27T12:34:56Z and UUID 1a2b3c4d-5e6f-7a8b-9c0d-1e2f3a4b5c6d"
        }

        norm = normalize_structure(obj)
        self.assertEqual(norm["timestamp"], "1970-01-01T00:00:00.000Z")
        self.assertEqual(norm["uuid"], "00000000-0000-0000-0000-000000000000")
        self.assertEqual(norm["temp_path"], "__TMPDIR__")

        # Verify global content remains unchanged
        self.assertEqual(norm["content"], obj["content"])

    def test_comparison_matches_with_normalization(self):
        actual = json.loads(json.dumps(self.expected))
        # Modify normalized metadata
        actual["uuid"] = "9f8e7d6c-5b4a-3f2e-1d0c-9b8a7f6e5d4c"
        actual["timestamp"] = "1970-01-01T00:00:00.000Z"

        diff = compare_structures(self.expected, actual)
        self.assertIsNone(diff)

    def test_mutation_role_fails(self):
        actual = json.loads(json.dumps(self.expected))
        actual["messages"][0]["role"] = "assistant"

        diff = compare_structures(self.expected, actual)
        self.assertIsNotNone(diff)
        self.assertIn("JSON-pointer '/messages/0/role'", diff)

    def test_mutation_stop_reason_fails(self):
        actual = json.loads(json.dumps(self.expected))
        actual["messages"][1]["stop_reason"] = "length"

        diff = compare_structures(self.expected, actual)
        self.assertIsNotNone(diff)
        self.assertIn("JSON-pointer '/messages/1/stop_reason'", diff)

    def test_mutation_tool_order_fails(self):
        actual = json.loads(json.dumps(self.expected))
        actual["messages"][1]["tool_calls"] = [
            {"id": "call_2", "type": "function", "function": {"name": "bash"}},
            {"id": "call_1", "type": "function", "function": {"name": "read"}}
        ]

        diff = compare_structures(self.expected, actual)
        self.assertIsNotNone(diff)
        self.assertTrue(
            "JSON-pointer '/messages/1/tool_calls/0/id'" in diff or
            "JSON-pointer '/messages/1/tool_calls/0/function/name'" in diff or
            "JSON-pointer '/messages/1/tool_calls/1/id'" in diff or
            "JSON-pointer '/messages/1/tool_calls/1/function/name'" in diff
        )

    def test_mutation_session_parent_id_fails(self):
        actual = json.loads(json.dumps(self.expected))
        actual["parent_id"] = "99999999-9999-9999-9999-999999999999"

        diff = compare_structures(self.expected, actual)
        self.assertIsNotNone(diff)
        self.assertIn("JSON-pointer '/parent_id'", diff)

    def test_comparator_fails_on_stale_corpus_digest(self):
        root = os.path.dirname(os.path.dirname(os.path.dirname(__file__)))
        fixtures_dir = os.path.join(root, "fixtures", "upstream-pi")
        with tempfile.TemporaryDirectory() as tmpdir:
            # Copy fixtures to tmpdir
            shutil.copytree(fixtures_dir, os.path.join(tmpdir, "fixtures"))
            tmp_fixtures = os.path.join(tmpdir, "fixtures")

            # Mutate corpusDigest in tmp manifest
            man_path = os.path.join(tmp_fixtures, "manifest.json")
            with open(man_path, "r", encoding="utf-8") as f:
                man_data = json.load(f)
            man_data["corpusDigest"]["M1a"] = "sha256:" + "0" * 64
            with open(man_path, "w", encoding="utf-8") as f:
                json.dump(man_data, f)

            actual_dir = os.path.join(tmpdir, "actual")
            os.makedirs(actual_dir)

            errors = validate_manifest(man_data, milestone="M1a", fixtures_dir=tmp_fixtures)
            self.assertTrue(any("Corpus digest mismatch" in e for e in errors))

if __name__ == "__main__":
    unittest.main()
