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

def _repair_gaxios_metadata(upstream_root: str) -> None:
    """Repair the known npm-installed metadata corruption before upstream startup."""
    with tempfile.TemporaryDirectory(prefix="gaxios-repair-") as temp_dir:
        packed = subprocess.run(
            ["npm", "pack", "gaxios@7.1.4", "--pack-destination", temp_dir],
            cwd=upstream_root,
            capture_output=True,
            text=True,
            check=True,
        )
        archive = next(Path(temp_dir).glob("gaxios-7.1.4.tgz"), None)
        if archive is None:
            raise RuntimeError(f"npm pack gaxios returned no archive: {packed.stdout}")
        subprocess.run(["tar", "-xzf", str(archive), "-C", temp_dir], check=True)
        target = Path(upstream_root) / "node_modules/gaxios/package.json"
        target.write_bytes((Path(temp_dir) / "package/package.json").read_bytes())
        json.loads(target.read_text(encoding="utf-8"))


def capture_cli_print_basic(upstream_root: str) -> Dict[str, Any]:
    """Capture case cli.print.basic using upstream Pi CLI print mode."""
    _repair_gaxios_metadata(upstream_root)
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
    _repair_gaxios_metadata(upstream_root)
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
    raise NotImplementedError("real upstream capture required: provider.openai-chat.fragmented-sse")

def capture_tool_read_bounds(upstream_root: str) -> Dict[str, Any]:
    """Capture case tool.read.bounds by invoking upstream createReadToolDefinition on disposable fixture."""
    lines = [f"line {i}" for i in range(1, 21)]
    fixture_content = "\n".join(lines)
    with tempfile.TemporaryDirectory(prefix="capture-read-", dir=upstream_root) as temp_dir:
        fixture_path = Path(temp_dir) / "fixture.txt"
        script_path = Path(temp_dir) / "capture-read.ts"
        fixture_path.write_text(fixture_content, encoding="utf-8")
        script_path.write_text(
            f"""import {{ createReadToolDefinition }} from "../packages/coding-agent/src/core/tools/read.ts";

const tool = createReadToolDefinition({json.dumps(temp_dir)});
const path = {json.dumps(str(fixture_path))};
const success = await tool.execute("read-1", {{ path, offset: 5, limit: 3 }}, undefined, undefined, undefined);
let error_case;
try {{
  await tool.execute("read-2", {{ path, offset: 100, limit: 5 }}, undefined, undefined, undefined);
}} catch (error) {{
  error_case = {{ error: String(error) }};
}}
console.log(JSON.stringify({{ success, error_case }}));
""",
            encoding="utf-8",
        )
        res = subprocess.run(
            ["node", "--import", "tsx/esm", str(script_path)],
            cwd=upstream_root,
            capture_output=True,
            text=True,
        )
        if res.returncode != 0:
            raise RuntimeError(
                f"upstream tool.read.bounds execution failed ({res.returncode}): {res.stderr.strip()}"
            )
        try:
            return normalize_structure(json.loads(res.stdout.strip()))
        except json.JSONDecodeError as exc:
            raise RuntimeError(f"upstream tool.read.bounds returned invalid JSON: {res.stdout!r}") from exc

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
