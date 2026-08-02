import unittest

from m1a_adapters import (
    ADAPTERS,
    _normalize_capture,
    _parse_strict_jsonl,
    capture_agent_retry_auto_compaction,
    capture_agent_serial_tool_loop,
    capture_cli_json_events,
    capture_cli_print_basic,
    capture_provider_openai_chat_fragmented_sse,
    capture_resource_context_precedence,
    capture_resource_untrusted_project,
    capture_tool_bash_cancel_descendants,
    capture_tool_edit,
    capture_tool_read_bounds,
)


class CaptureAdapterContracts(unittest.TestCase):
    def test_registry_entries(self):
        expected = {
            "cli.print.basic": capture_cli_print_basic,
            "agent.serial-tool-loop": capture_agent_serial_tool_loop,
            "provider.openai-chat.fragmented-sse": capture_provider_openai_chat_fragmented_sse,
            "tool.edit": capture_tool_edit,
            "tool.read.bounds": capture_tool_read_bounds,
            "tool.bash.cancel-descendants": capture_tool_bash_cancel_descendants,
            "resource.context-precedence": capture_resource_context_precedence,
            "resource.untrusted-project": capture_resource_untrusted_project,
            "cli.json-events": capture_cli_json_events,
            "agent.retry-auto-compaction": capture_agent_retry_auto_compaction,
        }
        self.assertEqual(ADAPTERS, expected)

    def test_strict_jsonl(self):
        self.assertEqual(
            _parse_strict_jsonl('{"type":"a"}\n{"type":"b"}\n', "", 0)[1]["type"], "b"
        )
        for stdout, stderr, code in [
            ("", "", 0),
            ('{"ok":true}\n', "diagnostic", 0),
            ("nope\n", "", 0),
            ('{"ok":true}\n', "", 1),
        ]:
            with self.assertRaises(RuntimeError):
                _parse_strict_jsonl(stdout, stderr, code)

    def test_normalization_preserves_order_and_references(self):
        value = {
            "events": [
                {"entryId": "a", "timestamp": 1700000000000},
                {"entryId": "a", "timestamp": "2024-01-01T00:00:00Z"},
                {"entryId": "b"},
            ],
            "path": "/tmp/case/x",
            "text": "unchanged",
        }
        normalized = _normalize_capture(value, "/tmp/case", "/opt/upstream")
        self.assertEqual(
            [e["entryId"] for e in normalized["events"]],
            ["entry-1", "entry-1", "entry-2"],
        )
        self.assertEqual(
            [e.get("timestamp") for e in normalized["events"]], [0, 0, None]
        )
        self.assertEqual(
            normalized["events"][0]["text"]
            if "text" in normalized["events"][0]
            else normalized["text"],
            "unchanged",
        )
        self.assertEqual(normalized["path"], "__CAPTURE_ROOT__/x")

    def test_normalization_separates_session_and_entry_ids(self):
        normalized = _normalize_capture(
            {"sessionId": "same", "entryId": "same"}, "/tmp/case", "/opt/upstream"
        )
        self.assertEqual(normalized, {"sessionId": "session-1", "entryId": "entry-1"})


if __name__ == "__main__":
    unittest.main()
