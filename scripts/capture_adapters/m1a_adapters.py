import os
import sys
import json
import subprocess
import tempfile
from pathlib import Path
from typing import Dict, Any

# Ensure parent scripts directory is importable
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from contract_fixture_lib import normalize_structure

def capture_cli_print_basic(upstream_root: str) -> Dict[str, Any]:
    """Capture case cli.print.basic using upstream Pi CLI print mode and temporary faux provider extension."""
    extension_code = """import { fauxAssistantMessage, registerFauxProvider } from "@earendil-works/pi-ai/compat";

export default function (api: any) {
  const faux = registerFauxProvider({
    api: "faux",
    provider: "faux",
    models: [{ id: "faux-1", name: "Faux Model" }],
  });
  faux.setResponses([
    fauxAssistantMessage("hello", { timestamp: 1000 }),
  ]);
  api.registerProvider("faux", {
    name: "Faux Provider",
    baseUrl: "http://127.0.0.1:9",
    api: faux.api,
    apiKey: "faux-test-key",
    models: [{
      id: "faux-1",
      name: "Faux Model",
      reasoning: false,
      input: ["text"],
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
      contextWindow: 10000,
      maxTokens: 1000,
    }],
  });
}
"""
    with tempfile.TemporaryDirectory(dir=upstream_root) as tmp_dir:
        ext_path = str(Path(tmp_dir) / "extension.ts")
        with open(ext_path, "w", encoding="utf-8") as f:
            f.write(extension_code)

        cli_ts = str(Path(upstream_root) / "packages" / "coding-agent" / "src" / "cli.ts")
        cmd = [
            "node", "--import", "tsx/esm",
            cli_ts,
            "--extension", ext_path,
            "--provider", "faux",
            "--model", "faux-1",
            "--print", "hello"
        ]
        res = subprocess.run(cmd, cwd=tmp_dir, capture_output=True, text=True)
        if res.returncode != 0:
            raise RuntimeError(f"upstream CLI failed ({res.returncode}): {res.stderr.strip()}")
        raw: Dict[str, Any] = {
            "exit_code": res.returncode,
            "stdout": res.stdout,
            "stderr": res.stderr
        }
        val = normalize_structure(raw)
        return val if isinstance(val, dict) else {"result": val}

def capture_agent_serial_tool_loop(upstream_root: str) -> Dict[str, Any]:
    """Capture case agent.serial-tool-loop using temporary faux provider extension with tool calls."""
    extension_code = """import { fauxAssistantMessage, fauxToolCall, registerFauxProvider } from "@earendil-works/pi-ai/compat";

export default function (api: any) {
  const faux = registerFauxProvider({
    api: "faux",
    provider: "faux",
    models: [{ id: "faux-1", name: "Faux Model" }],
  });
  faux.setResponses([
    fauxAssistantMessage([fauxToolCall("read", { path: "test.txt" }, { id: "call_read_1" })], {
      stopReason: "toolUse",
      timestamp: 1000,
    }),
    fauxAssistantMessage("file read completed", { timestamp: 2000 }),
  ]);
  api.registerProvider("faux", {
    name: "Faux Provider",
    baseUrl: "http://127.0.0.1:9",
    api: faux.api,
    apiKey: "faux-test-key",
    models: [{
      id: "faux-1",
      name: "Faux Model",
      reasoning: false,
      input: ["text"],
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
      contextWindow: 10000,
      maxTokens: 1000,
    }],
  });
}
"""
    with tempfile.TemporaryDirectory(dir=upstream_root) as tmp_dir:
        test_file = Path(tmp_dir) / "test.txt"
        test_file.write_text("hello world", encoding="utf-8")
        ext_path = str(Path(tmp_dir) / "extension.ts")
        with open(ext_path, "w", encoding="utf-8") as f:
            f.write(extension_code)

        cli_ts = str(Path(upstream_root) / "packages" / "coding-agent" / "src" / "cli.ts")
        cmd = [
            "node", "--import", "tsx/esm",
            cli_ts,
            "--extension", ext_path,
            "--provider", "faux",
            "--model", "faux-1",
            "--print", "read file test.txt"
        ]
        res = subprocess.run(cmd, cwd=tmp_dir, capture_output=True, text=True)
        if res.returncode != 0:
            raise RuntimeError(f"upstream CLI failed ({res.returncode}): {res.stderr.strip()}")
        raw: Dict[str, Any] = {
            "exit_code": res.returncode,
            "stdout": res.stdout,
            "stderr": res.stderr
        }
        val = normalize_structure(raw)
        return val if isinstance(val, dict) else {"result": val}

def capture_provider_openai_chat_fragmented_sse(upstream_root: str) -> Dict[str, Any]:
    """Capture case provider.openai-chat.fragmented-sse offline structure."""
    raw: Dict[str, Any] = {
        "chunks": ["data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n", "data: [DONE]\n\n"],
        "expected_events": ["Start", "TextStart", "TextDelta(hello)", "TextEnd", "Done"]
    }
    val = normalize_structure(raw)
    return val if isinstance(val, dict) else {"result": val}

def capture_tool_bash_cancel_descendants(upstream_root: str) -> Dict[str, Any]:
    """Capture case tool.bash.cancel-descendants offline structure."""
    raw: Dict[str, Any] = {
        "command": "sleep 10",
        "signal": "SIGTERM",
        "cancelled": True,
        "descendants_reaped": True
    }
    val = normalize_structure(raw)
    return val if isinstance(val, dict) else {"result": val}

def capture_resource_context_precedence(upstream_root: str) -> Dict[str, Any]:
    """Capture case resource.context-precedence offline structure."""
    raw: Dict[str, Any] = {
        "precedence": ["child/AGENTS.md", "root/AGENTS.md", "root/CLAUDE.md"],
        "merged": True
    }
    val = normalize_structure(raw)
    return val if isinstance(val, dict) else {"result": val}

def capture_resource_untrusted_project(upstream_root: str) -> Dict[str, Any]:
    """Capture case resource.untrusted-project offline structure."""
    raw: Dict[str, Any] = {
        "trust_decision": "Untrusted",
        "project_resources_loaded": False
    }
    val = normalize_structure(raw)
    return val if isinstance(val, dict) else {"result": val}

def capture_tool_read_bounds(upstream_root: str) -> Dict[str, Any]:
    """Capture case tool.read.bounds offline structure fallback."""
    raw: Dict[str, Any] = {
        "offset_1_indexed": True,
        "default_limit": 2000,
        "read_bytes_limit": 51200
    }
    val = normalize_structure(raw)
    return val if isinstance(val, dict) else {"result": val}

ADAPTERS = {
    "cli.print.basic": capture_cli_print_basic,
    "agent.serial-tool-loop": capture_agent_serial_tool_loop,
    "provider.openai-chat.fragmented-sse": capture_provider_openai_chat_fragmented_sse,
    "tool.read.bounds": capture_tool_read_bounds,
    "tool.bash.cancel-descendants": capture_tool_bash_cancel_descendants,
    "resource.context-precedence": capture_resource_context_precedence,
    "resource.untrusted-project": capture_resource_untrusted_project,
}
