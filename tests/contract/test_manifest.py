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
        
    def test_m1a_cases_are_pending(self):
        manifest = load_manifest()
        req_cases = manifest.get("requiredCaseIds", {})
        cases = manifest.get("cases", {})
        
        self.assertIn("M1a", req_cases)
        m1a_cases = req_cases["M1a"]
        self.assertEqual(len(m1a_cases), 13)
        
        for case_id in m1a_cases:
            self.assertIn(case_id, cases)
            case_info = cases[case_id]
            self.assertFalse(case_info.get("captured"), f"Case {case_id} should be pending/uncaptured in M0 baseline")

if __name__ == '__main__':
    unittest.main()
