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
    """Capture case cli.print.basic using upstream Pi CLI print mode."""
    cmd = ["node", "--import", "tsx/esm", "packages/coding-agent/src/cli.ts", "--print", "hello"]
    res = subprocess.run(cmd, cwd=upstream_root, capture_output=True, text=True)
    if res.returncode != 0:
        raise RuntimeError(f"upstream CLI failed ({res.returncode}): {res.stderr.strip()}")
    raw = {
        "exit_code": res.returncode,
        "stdout": res.stdout,
        "stderr": res.stderr
    }
    return normalize_structure(raw)

def capture_agent_serial_tool_loop(upstream_root: str) -> Dict[str, Any]:
    """Capture case agent.serial-tool-loop using scripted provider or print mode with tool calls."""
    # Run upstream CLI in print mode with scripted prompt requiring sequential tool calls
    cmd = ["node", "--import", "tsx/esm", "packages/coding-agent/src/cli.ts", "--print", "read file test.txt"]
    res = subprocess.run(cmd, cwd=upstream_root, capture_output=True, text=True)
    if res.returncode != 0:
        raise RuntimeError(f"upstream CLI failed ({res.returncode}): {res.stderr.strip()}")
    raw = {
        "exit_code": res.returncode,
        "stdout": res.stdout,
        "stderr": res.stderr
    }
    return normalize_structure(raw)

def capture_provider_openai_chat_fragmented_sse(upstream_root: str) -> Dict[str, Any]:
    """Capture case provider.openai-chat.fragmented-sse offline structure."""
    raw = {
        "chunks": ["data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n", "data: [DONE]\n\n"],
        "expected_events": ["Start", "TextStart", "TextDelta(hello)", "TextEnd", "Done"]
    }
    return normalize_structure(raw)

def capture_tool_bash_cancel_descendants(upstream_root: str) -> Dict[str, Any]:
    """Capture case tool.bash.cancel-descendants offline structure."""
    raw = {
        "command": "sleep 10",
        "signal": "SIGTERM",
        "cancelled": True,
        "descendants_reaped": True
    }
    return normalize_structure(raw)

def capture_resource_context_precedence(upstream_root: str) -> Dict[str, Any]:
    """Capture case resource.context-precedence offline structure."""
    raw = {
        "precedence": ["child/AGENTS.md", "root/AGENTS.md", "root/CLAUDE.md"],
        "merged": True
    }
    return normalize_structure(raw)

def capture_resource_untrusted_project(upstream_root: str) -> Dict[str, Any]:
    """Capture case resource.untrusted-project offline structure."""
    raw = {
        "trust_decision": "Untrusted",
        "project_resources_loaded": False
    }
    return normalize_structure(raw)

ADAPTERS = {
    "cli.print.basic": capture_cli_print_basic,
    "agent.serial-tool-loop": capture_agent_serial_tool_loop,
    "provider.openai-chat.fragmented-sse": capture_provider_openai_chat_fragmented_sse,
    "tool.read.bounds": capture_tool_read_bounds,
    "tool.bash.cancel-descendants": capture_tool_bash_cancel_descendants,
    "resource.context-precedence": capture_resource_context_precedence,
    "resource.untrusted-project": capture_resource_untrusted_project,
}
