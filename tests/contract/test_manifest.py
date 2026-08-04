import unittest
import sys
import os

# Adjust path to import from scripts/
sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(__file__))), 'scripts'))

from contract_fixture_lib import load_manifest, validate_manifest

class TestFixtureManifest(unittest.TestCase):
    def test_load_and_validate(self):
        try:
            manifest = load_manifest()
        except FileNotFoundError as e:
            self.fail(f"Manifest file missing: {e}")
            
        errors = validate_manifest(manifest)
        self.assertEqual(errors, [], f"Manifest contains validation errors:\n" + "\n".join(errors))

    def test_m0_validation(self):
        manifest = load_manifest()
        errors = validate_manifest(manifest, milestone="M0")
        self.assertEqual(errors, [], f"M0 manifest validation contains errors:\n" + "\n".join(errors))
        
    def test_m1a_cases_have_truthful_capture_state(self):
        manifest = load_manifest()
        req_cases = manifest.get("requiredCaseIds", {})
        cases = manifest.get("cases", {})
        self.assertIn("M1a", req_cases)
        self.assertEqual(len(req_cases["M1a"]), 13)
        for case_id in req_cases["M1a"]:
            self.assertIn(case_id, cases)
            info = cases[case_id]
            if info.get("oracle") == "pi-rs-invariant":
                self.assertFalse(info.get("captured"), f"Invariant case {case_id} must not claim upstream capture")
            else:
                self.assertTrue(info.get("captured"), f"Upstream case {case_id} must have capture evidence")

if __name__ == '__main__':
    unittest.main()
