import ast
import inspect
import textwrap
import unittest
from pathlib import Path
from unittest.mock import patch

import m1a_adapters
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

    def test_cli_json_runner_uses_disposable_agent_dir(self):
        source = textwrap.dedent(inspect.getsource(capture_cli_json_events))
        tree = ast.parse(source)
        self.assertIn(
            'with tempfile.TemporaryDirectory(prefix="pi-json-capture-") as temp:',
            source,
        )
        self.assertIn('agent_dir = temp_root / "agent-dir"', source)
        self.assertIn("agent_dir.mkdir()", source)
        self.assertIn('"__AGENT_DIR__", json.dumps(str(agent_dir))', source)
        self.assertIn("cwd=upstream_root", source)
        self.assertNotIn(".capture-agent", source)

        calls = [
            ast.unparse(node) for node in ast.walk(tree) if isinstance(node, ast.Call)
        ]
        self.assertTrue(any(call.startswith("agent_dir.mkdir") for call in calls))
        self.assertTrue(any("json.dumps(str(agent_dir))" in call for call in calls))
        self.assertTrue(any(call.startswith("path.write_text") for call in calls))
        self.assertTrue(any(call.startswith("subprocess.run") for call in calls))
        make_start = source.index("const make =")
        stream_patch = source.index("result.session.agent.streamFunction", make_start)
        return_result = source.index("return result", stream_patch)
        self.assertLess(make_start, stream_patch)
        self.assertLess(stream_patch, return_result)

    def test_cli_json_runner_subprocess_contract_and_cleanup(self):
        seen = {}

        def fake_run(command, **kwargs):
            runner_path = Path(command[-1])
            seen.update(
                command=command,
                kwargs=kwargs,
                runner=runner_path,
                agent_dir=runner_path.parent / "agent-dir",
            )
            source = runner_path.read_text(encoding="utf-8")
            self.assertIn(
                'from "/opt/pinned-upstream/node_modules/@earendil-works/pi-ai/dist/index.js"',
                source,
            )
            self.assertNotIn(
                'from "@earendil-works/pi-ai"',
                source,
            )
            self.assertIn(
                'AuthStorage.inMemory({ capture: { type: "api_key", key: "capture-key" } })',
                source,
            )
            self.assertIn("modelsPath: null", source)
            self.assertIn("allowModelNetwork: false", source)
            registration = "modelRuntime.registerProvider(model.provider, {\nbaseUrl: model.baseUrl,\napi: model.api,\n});"
            self.assertIn(registration, source)
            create_runtime = source.index(
                "const modelRuntime = await ModelRuntime.create"
            )
            registration_start = source.index(registration)
            make_start = source.index("const make =")
            self.assertLess(create_runtime, registration_start)
            self.assertLess(registration_start, make_start)
            self.assertNotIn("models:", source)
            self.assertNotIn("modelRuntime.getModel(", source)
            self.assertIn("modelRuntime });", source)
            return type(
                "Completed",
                (),
                {
                    "stdout": '{"type":"start"}\n{"type":"done"}\n',
                    "stderr": "",
                    "returncode": 0,
                },
            )()

        with (
            patch.object(
                m1a_adapters, "_get_tsx_import_arg", return_value="fake-loader.mjs"
            ),
            patch.object(m1a_adapters.subprocess, "run", side_effect=fake_run),
        ):
            result = capture_cli_json_events("/opt/pinned-upstream")

        self.assertEqual(
            result, {"exit_code": 0, "events": [{"type": "start"}, {"type": "done"}]}
        )
        self.assertEqual(seen["command"][:3], ["node", "--import", "fake-loader.mjs"])
        self.assertEqual(
            seen["kwargs"],
            {"cwd": "/opt/pinned-upstream", "capture_output": True, "text": True},
        )
        self.assertFalse(seen["runner"].exists())
        self.assertFalse(seen["agent_dir"].exists())

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

    def test_normalization_stabilizes_session_entry_ids_and_order(self):
        normalized = _normalize_capture(
            {
                "events": [
                    {"type": "session_start", "id": "session-random"},
                    {"type": "message_start", "id": "entry-random", "parentId": "session-random"},
                    {"type": "message_end", "id": "entry-random"},
                ]
            },
            "/tmp/case",
            "/opt/upstream",
        )
        self.assertEqual(
            [event["type"] for event in normalized["events"]],
            ["session_start", "message_start", "message_end"],
        )
        self.assertEqual(
            [event["id"] for event in normalized["events"]],
            ["entry-1", "entry-2", "entry-2"],
        )
        self.assertEqual(normalized["events"][1]["parentId"], "entry-1")


if __name__ == "__main__":
    unittest.main()
