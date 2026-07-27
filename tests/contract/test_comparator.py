import unittest
import sys
import os
import json
import tempfile

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(__file__))), 'scripts'))

from contract_fixture_lib import normalize_structure, compare_structures

class TestComparator(unittest.TestCase):
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

    def test_mutations_using_temp_dirs(self):
        # We test saving expected and mutated JSONs to temp files and comparing them
        with tempfile.TemporaryDirectory() as tmpdir:
            exp_file = os.path.join(tmpdir, "case_1.expected.json")
            act_file = os.path.join(tmpdir, "case_1.actual.json")
            
            with open(exp_file, "w", encoding="utf-8") as f:
                json.dump(self.expected, f)
                
            # 1. Matching case
            with open(act_file, "w", encoding="utf-8") as f:
                json.dump(self.expected, f)
            with open(exp_file, "r", encoding="utf-8") as f1, open(act_file, "r", encoding="utf-8") as f2:
                self.assertIsNone(compare_structures(json.load(f1), json.load(f2)))
                
            # 2. Mutated role
            mutated = json.loads(json.dumps(self.expected))
            mutated["messages"][0]["role"] = "assistant"
            with open(act_file, "w", encoding="utf-8") as f:
                json.dump(mutated, f)
            with open(exp_file, "r", encoding="utf-8") as f1, open(act_file, "r", encoding="utf-8") as f2:
                diff = compare_structures(json.load(f1), json.load(f2))
                self.assertIsNotNone(diff)
                self.assertIn("JSON-pointer '/messages/0/role'", diff)

if __name__ == "__main__":
    unittest.main()
