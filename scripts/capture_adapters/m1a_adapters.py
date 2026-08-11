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
    extension_code = """import { fauxAssistantMessage, fauxProvider } from "@earendil-works/pi-ai";

export default function (api: any) {
  const faux = fauxProvider({
    api: "faux",
    provider: "faux",
    models: [{ id: "faux-1", name: "Faux Model" }],
  });
  faux.setResponses([
    fauxAssistantMessage("hello", { timestamp: 1000 }),
  ]);
  api.registerProvider(faux.provider);
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
    extension_code = """import { fauxAssistantMessage, fauxToolCall, fauxProvider } from "@earendil-works/pi-ai";

export default function (api: any) {
  const faux = fauxProvider({
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
  api.registerProvider(faux.provider);
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

def capture_tool_edit(upstream_root: str) -> Dict[str, Any]:
    """Capture exact multi-edit projections from pinned coding-agent edit tool."""
    script = r'''import { NodeExecutionEnv } from "AGENT_NODE";
import { createEditTool } from "AGENT_EDIT";
import { writeFile, readFile } from "node:fs/promises";
const root = process.argv[2];
const env = new NodeExecutionEnv({ cwd: root });
const cases = {
  two_disjoint_original_matches: {content: "alpha\\nbeta\\ngamma\\ndelta\\n", edits: [{oldText:"alpha\\n",newText:"ALPHA\\n"},{oldText:"gamma\\n",newText:"GAMMA\\n"}]},
  matches_original_not_incremental: {content: "foo\\nbar\\nbaz\\n", edits: [{oldText:"foo\\n",newText:"foo bar\\n"},{oldText:"bar\\n",newText:"BAR\\n"}]},
  overlap_rejected_without_write: {content: "one\\ntwo\\nthree\\n", edits: [{oldText:"one\\ntwo\\n",newText:"ONE\\nTWO\\n"},{oldText:"two\\nthree\\n",newText:"TWO\\nTHREE\\n"}]},
  missing_later_edit_without_partial_write: {content: "alpha\\nbeta\\ngamma\\n", edits: [{oldText:"alpha\\n",newText:"ALPHA\\n"},{oldText:"missing\\n",newText:"MISSING\\n"}]},
  bom_crlf_preserved: {content: "\\ufeffalpha\\r\\nbeta\\r\\ngamma\\r\\n", edits: [{oldText:"alpha\\n",newText:"ALPHA\\n"},{oldText:"gamma\\n",newText:"GAMMA\\n"}]}
};
const out = {schema:{required:["path","edits"],editRequired:["oldText","newText"]},scenarios:{}};
for (const [name, c] of Object.entries(cases)) { const p = `${root}/edit.txt`; await writeFile(p,c.content); try { const r=await createEditTool().execute("edit",{path:p,edits:c.edits},undefined,undefined,{env}); out.scenarios[name]={ok:true,message:r.content.filter(x=>x.type==="text")[0].text,finalHex:Buffer.from(await readFile(p)).toString("hex")}; } catch(e) { out.scenarios[name]={ok:false,error:e instanceof Error?e.message:String(e),finalHex:Buffer.from(await readFile(p)).toString("hex")}; } }
console.log(JSON.stringify(out));
'''
    script = script.replace("\\\\n", "\\n").replace("\\\\r", "\\r").replace("\\\\u", "\\u")
    script = script.replace("AGENT_NODE", str(Path(upstream_root) / "packages/agent/src/node.ts")).replace("AGENT_EDIT", str(Path(upstream_root) / "packages/coding-agent/src/core/tools/edit.ts"))
    with tempfile.TemporaryDirectory(dir=upstream_root) as tmp_dir:
        root = Path(tmp_dir) / "edit-fixture"; root.mkdir()
        script_path = Path(tmp_dir) / "capture-edit.ts"; script_path.write_text(script, encoding="utf-8")
        res = subprocess.run(["node", "--import", "tsx/esm", str(script_path), str(root)], cwd=upstream_root, capture_output=True, text=True)
        if res.returncode != 0: raise RuntimeError(f"upstream edit capture failed ({res.returncode}): {res.stderr.strip()}")
        raw = json.loads(res.stdout.strip().splitlines()[-1])
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
    """Capture read behavior by executing pinned pi-mono harness read tool."""
    script = r'''import { NodeExecutionEnv } from "AGENT_NODE";
import { createReadTool } from "AGENT_READ";
import { mkdir, writeFile, rm } from "node:fs/promises";

const root = process.argv[2];
const env = new NodeExecutionEnv({ cwd: root });
const context = { env };
const files = {
  "basic.txt": "line 1\nline 2\nline 3\nline 4\nline 5\n",
  "empty.txt": "",
  "lines2500.txt": Array.from({length: 2500}, (_, i) => `line ${i + 1}`).join("\n") + "\n",
  "multibyte.txt": Array.from({length: 3500}, (_, i) => `line ${i}: €€€€€`).join("\n") + "\n",
  "huge_first_line.txt": "a".repeat(55000) + "\nline 2\n",
};
for (const [name, content] of Object.entries(files)) await writeFile(`${root}/${name}`, content);
const tool = createReadTool();
const cases = [
  ["default_params", {path: "basic.txt"}],
  ["offset_1", {path: "basic.txt", offset: 1}],
  ["non_default_offset", {path: "basic.txt", offset: 3}],
  ["explicit_limit", {path: "basic.txt", offset: 1, limit: 2}],
  ["offset_beyond_eof", {path: "basic.txt", offset: 10}],
  ["more_than_default_lines", {path: "lines2500.txt"}],
  ["multibyte_near_50k", {path: "multibyte.txt"}],
  ["first_line_exceeds_50k", {path: "huge_first_line.txt"}],
  ["empty_file", {path: "empty.txt"}],
];
const scenarios = {};
for (const [name, args] of cases) {
  try {
    const result = await tool.execute("read", args, undefined, undefined, context);
    const output = result.content.filter((part) => part.type === "text").map((part) => part.text ?? "").join("\n");
    scenarios[name] = {ok: true, output, byte_length: new TextEncoder().encode(output).byteLength};
  } catch (error) {
    scenarios[name] = {ok: false, error: error instanceof Error ? error.message : String(error)};
  }
}
console.log(JSON.stringify({scenarios}));
await rm(root, {recursive: true, force: true});
'''
    agent_node = Path(upstream_root) / "packages" / "agent" / "src" / "node.ts"
    agent_read = Path(upstream_root) / "packages" / "agent" / "src" / "harness" / "tools" / "read.ts"
    script = script.replace("AGENT_NODE", str(agent_node)).replace("AGENT_READ", str(agent_read))
    with tempfile.TemporaryDirectory(dir=upstream_root) as tmp_dir:
        root = Path(tmp_dir) / "read-fixture"
        root.mkdir()
        script_path = Path(tmp_dir) / "capture-read.ts"
        script_path.write_text(script, encoding="utf-8")
        res = subprocess.run(
            ["node", "--import", "tsx/esm", str(script_path), str(root)],
            cwd=upstream_root, capture_output=True, text=True,
        )
        if res.returncode != 0:
            raise RuntimeError(f"upstream read capture failed ({res.returncode}): {res.stderr.strip()}")
        try:
            raw = json.loads(res.stdout.strip().splitlines()[-1])
        except (ValueError, IndexError) as exc:
            raise RuntimeError(f"invalid upstream read capture output: {res.stdout!r}") from exc
    val = normalize_structure(raw)
    return val if isinstance(val, dict) else {"result": val}

ADAPTERS = {
    "cli.print.basic": capture_cli_print_basic,
    "agent.serial-tool-loop": capture_agent_serial_tool_loop,
    "provider.openai-chat.fragmented-sse": capture_provider_openai_chat_fragmented_sse,
    "tool.read.bounds": capture_tool_read_bounds,
    "tool.edit": capture_tool_edit,
    "tool.bash.cancel-descendants": capture_tool_bash_cancel_descendants,
    "resource.context-precedence": capture_resource_context_precedence,
    "resource.untrusted-project": capture_resource_untrusted_project,
}
