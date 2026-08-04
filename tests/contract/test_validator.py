import unittest
import sys
import os

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(__file__))), 'scripts'))

from contract_fixture_lib import validate_manifest  # type: ignore

class TestValidator(unittest.TestCase):
    def setUp(self):
        # Base valid manifest structure
        self.valid_manifest = {
            "schemaVersion": 1,
            "reference": {
                "package": "@earendil-works/pi-coding-agent",
                "version": "0.82.1",
                "commit": "2efa728d2ee90ef597626e96b1e28ef2b279f07c",
                "lockfileSha256": "472f0726dc79f3b38df58d8a8bce96bf56fbf993a134b49aabc54947b8461e59"
            },
            "captureEnvironment": {
                "captureStatus": "pending",
                "host": None,
                "nodeVersion": None,
                "bunVersion": None,
                "digest": None
            },
            "requiredCaseIds": {
                "M0": [],
                "M1a": ["case_1"],
                "M1": [],
                "M2": [],
                "M3": []
            },
            "cases": {
                "case_1": {
                    "captured": False,
                    "description": "Case 1 description",
                    "oracle": "upstream-pi",
                    "normalizationAllowlist": []
                }
            }
        }

    def test_valid_manifest_passes(self):
        errors = validate_manifest(self.valid_manifest)
        self.assertEqual(errors, [])

    def test_missing_schema_version(self):
        manifest = dict(self.valid_manifest)
        del manifest["schemaVersion"]
        errors = validate_manifest(manifest)
        self.assertIn("Missing schemaVersion", errors)

    def test_invalid_schema_version(self):
        manifest = dict(self.valid_manifest)
        manifest["schemaVersion"] = 2
        errors = validate_manifest(manifest)
        self.assertIn("Invalid schemaVersion: 2", errors)

    def test_placeholder_commit_sha_rejected(self):
        manifest = dict(self.valid_manifest)
        manifest["reference"] = dict(manifest["reference"])
        
        # All zeros SHA
        manifest["reference"]["commit"] = "0000000000000000000000000000000000000000"
        errors = validate_manifest(manifest)
        self.assertTrue(any("Placeholder reference commit SHA rejected" in e for e in errors))

        # Mock sequence SHA
        manifest["reference"]["commit"] = "1234567890123456789012345678901234567890"
        errors = validate_manifest(manifest)
        self.assertTrue(any("Placeholder reference commit SHA rejected" in e for e in errors))

    def test_malformed_commit_sha_rejected(self):
        manifest = dict(self.valid_manifest)
        manifest["reference"] = dict(manifest["reference"])
        
        manifest["reference"]["commit"] = "not-a-sha"
        errors = validate_manifest(manifest)
        self.assertTrue(any("Malformed reference commit SHA" in e for e in errors))

    def test_placeholder_lockfile_sha_rejected(self):
        manifest = dict(self.valid_manifest)
        manifest["reference"] = dict(manifest["reference"])
        
        # All zeros SHA-256
        manifest["reference"]["lockfileSha256"] = "0" * 64
        errors = validate_manifest(manifest)
        self.assertTrue(any("Placeholder reference lockfileSha256 rejected" in e for e in errors))

    def test_malformed_lockfile_sha_rejected(self):
        manifest = dict(self.valid_manifest)
        manifest["reference"] = dict(manifest["reference"])
        
        manifest["reference"]["lockfileSha256"] = "too-short"
        errors = validate_manifest(manifest)
        self.assertTrue(any("Malformed reference lockfileSha256" in e for e in errors))

    def test_capture_environment_status_completed_requires_digest(self):
        manifest = dict(self.valid_manifest)
        manifest["captureEnvironment"] = dict(manifest["captureEnvironment"])
        manifest["captureEnvironment"]["captureStatus"] = "completed"
        errors = validate_manifest(manifest)
        self.assertTrue(any("digest is required" in e for e in errors))
        manifest["captureEnvironment"]["digest"] = "sha256:" + "a" * 64
        self.assertFalse(any("captureEnvironment.captureStatus" in e for e in validate_manifest(manifest)))

    def test_capture_environment_with_invented_digest(self):
        manifest = dict(self.valid_manifest)
        manifest["captureEnvironment"] = dict(manifest["captureEnvironment"])
        manifest["captureEnvironment"]["digest"] = "some-sha256-value"
        errors = validate_manifest(manifest)
        self.assertTrue(any("captureEnvironment digest must be null or empty during pending capture" in e for e in errors))

    def test_missing_milestone_key(self):
        manifest = dict(self.valid_manifest)
        manifest["requiredCaseIds"] = {
            "M0": [],
            "M1a": ["case_1"],
            "M1": [],
            "M2": []
        }
        errors = validate_manifest(manifest)
        self.assertTrue(any("Missing required milestone key" in e for e in errors))

    def test_duplicate_required_case_ids(self):
        manifest = dict(self.valid_manifest)
        manifest["requiredCaseIds"] = {
            "M1a": ["case_1", "case_1"]
        }
        errors = validate_manifest(manifest)
        self.assertIn("Duplicate required case ID: case_1", errors)

    def test_missing_case_definition(self):
        manifest = dict(self.valid_manifest)
        manifest["requiredCaseIds"] = {
            "M1a": ["case_1", "case_missing"]
        }
        errors = validate_manifest(manifest)
        self.assertIn("Case 'case_missing' required by milestone M1a is missing from cases catalog", errors)

    def test_unknown_case_definition(self):
        manifest = dict(self.valid_manifest)
        manifest["cases"] = dict(manifest["cases"])
        manifest["cases"]["case_unknown"] = {
            "captured": False,
            "description": "Unknown case description",
            "oracle": "upstream-pi"
        }
        errors = validate_manifest(manifest)
        self.assertIn("Case 'case_unknown' in catalog is not associated with any milestone in requiredCaseIds", errors)

    def test_canonical_m1a_check_mismatch_fails(self):
        # Create a manifest that matches reference version/package but has incorrect M1a cases
        manifest = dict(self.valid_manifest)
        manifest["requiredCaseIds"] = dict(manifest["requiredCaseIds"])
        manifest["requiredCaseIds"]["M1a"] = ["case_1"] * 15 # More than 5 but wrong values
        errors = validate_manifest(manifest)
        self.assertTrue(any("requiredCaseIds.M1a does not match the canonical set" in e for e in errors))

    def test_unknown_top_level_key_rejected(self):
        manifest = dict(self.valid_manifest)
        manifest["unexpected_key"] = "foo"
        errors = validate_manifest(manifest)
        self.assertTrue(any("Unexpected top-level keys in manifest" in e for e in errors))

    def test_invalid_oracle(self):
        manifest = dict(self.valid_manifest)
        manifest["cases"] = {
            "case_1": {
                "captured": False,
                "description": "Case 1 description",
                "oracle": "invalid-oracle",
                "normalizationAllowlist": []
            }
        }
        errors = validate_manifest(manifest)
        self.assertIn("Case case_1 has invalid oracle: invalid-oracle (must be 'upstream-pi' or 'pi-rs-invariant')", errors)

    def test_milestone_uncaptured_checks(self):
        # general validation allows pending capture
        errors_gen = validate_manifest(self.valid_manifest)
        self.assertEqual(errors_gen, [])

        # M0 validation allows pending capture (since M0 has no cases in self.valid_manifest)
        errors_m0 = validate_manifest(self.valid_manifest, milestone="M0")
        self.assertEqual(errors_m0, [])

        # M1a validation fails since case_1 is not captured
        errors_m1a = validate_manifest(self.valid_manifest, milestone="M1a")
        self.assertIn("Case 'case_1' required for milestone M1a is not captured (pending)", errors_m1a)

        # M1a validation passes if the case is captured
        manifest_captured = dict(self.valid_manifest)
        manifest_captured["cases"] = {
            "case_1": {
                "captured": True,
                "description": "Case 1 description",
                "oracle": "upstream-pi",
                "normalizationAllowlist": []
            }
        }
        errors_captured = validate_manifest(manifest_captured, milestone="M1a")
        self.assertEqual(errors_captured, [])

if __name__ == "__main__":
    unittest.main()
