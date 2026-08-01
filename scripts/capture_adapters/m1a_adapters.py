import os
import sys
import json
import subprocess
import tempfile
from typing import Dict, Any

# Ensure parent scripts directory is importable
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from contract_fixture_lib import normalize_structure

def capture_cli_print_basic(upstream_root: str) -> Dict[str, Any]:
    """Capture case cli.print.basic using upstream Pi CLI print mode."""
    cmd = ["bun", "run", "packages/coding-agent/src/cli.ts", "--print", "hello"]
    res = subprocess.run(cmd, cwd=upstream_root, capture_output=True, text=True, check=True)
    raw = {
        "exit_code": res.returncode,
        "stdout": res.stdout,
        "stderr": res.stderr
    }
    return normalize_structure(raw)

def capture_agent_serial_tool_loop(upstream_root: str) -> Dict[str, Any]:
    """Capture case agent.serial-tool-loop using scripted provider or print mode with tool calls."""
    # Run upstream CLI in print mode with scripted prompt requiring sequential tool calls
    cmd = ["bun", "run", "packages/coding-agent/src/cli.ts", "--print", "read file test.txt"]
    res = subprocess.run(cmd, cwd=upstream_root, capture_output=True, text=True)
    raw = {
        "exit_code": res.returncode,
        "stdout": res.stdout,
        "stderr": res.stderr
    }
    return normalize_structure(raw)

def capture_provider_openai_chat_fragmented_sse(upstream_root: str) -> Dict[str, Any]:
    raise NotImplementedError("real upstream capture required: provider.openai-chat.fragmented-sse")

def capture_tool_read_bounds(upstream_root: str) -> Dict[str, Any]:
    raise NotImplementedError("real upstream capture required: tool.read.bounds")

def capture_tool_bash_cancel_descendants(upstream_root: str) -> Dict[str, Any]:
    raise NotImplementedError("real upstream capture required: tool.bash.cancel-descendants")

def capture_resource_context_precedence(upstream_root: str) -> Dict[str, Any]:
    raise NotImplementedError("real upstream capture required: resource.context-precedence")

def capture_resource_untrusted_project(upstream_root: str) -> Dict[str, Any]:
    raise NotImplementedError("real upstream capture required: resource.untrusted-project")

ADAPTERS = {
    "cli.print.basic": capture_cli_print_basic,
    "agent.serial-tool-loop": capture_agent_serial_tool_loop,
    "provider.openai-chat.fragmented-sse": capture_provider_openai_chat_fragmented_sse,
    "tool.read.bounds": capture_tool_read_bounds,
    "tool.bash.cancel-descendants": capture_tool_bash_cancel_descendants,
    "resource.context-precedence": capture_resource_context_precedence,
    "resource.untrusted-project": capture_resource_untrusted_project,
}
