import os
import sys
import unittest

# Adjust path to import from scripts/
sys.path.insert(
    0,
    os.path.join(
        os.path.dirname(os.path.dirname(os.path.dirname(__file__))), "scripts"
    ),
)

from contract_fixture_lib import load_manifest, validate_manifest


class TestFixtureManifest(unittest.TestCase):
    def test_load_and_validate(self):
        try:
            manifest = load_manifest()
        except FileNotFoundError as e:
            self.fail(f"Manifest file missing: {e}")

        errors = validate_manifest(manifest)
        self.assertEqual(
            errors, [], "Manifest contains validation errors:\n" + "\n".join(errors)
        )

    def test_m0_validation(self):
        manifest = load_manifest()
        errors = validate_manifest(manifest, milestone="M0")
        self.assertEqual(
            errors, [], "M0 manifest validation contains errors:\n" + "\n".join(errors)
        )

    def test_m1a_cases_captured_status(self):
        manifest = load_manifest()
        req_cases = manifest.get("requiredCaseIds", {})
        cases = manifest.get("cases", {})

        self.assertIn("M1a", req_cases)
        m1a_cases = req_cases["M1a"]
        self.assertEqual(len(m1a_cases), 13)

        # Check if capture is pending or completed based on captureEnvironment.captureStatus
        status = manifest.get("captureEnvironment", {}).get("captureStatus")
        is_completed = status == "completed"

        for case_id in m1a_cases:
            self.assertIn(case_id, cases)
            case_info = cases[case_id]
            expected_captured = True if is_completed else False
            self.assertEqual(
                case_info.get("captured"),
                expected_captured,
                f"Case {case_id} captured status ({case_info.get('captured')}) does not match manifest captureStatus '{status}'",
            )


if __name__ == "__main__":
    unittest.main()
