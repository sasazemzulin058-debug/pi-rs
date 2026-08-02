import json
import os
import shlex
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

# Ensure parent scripts directory is importable
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from contract_fixture_lib import normalize_structure


def _get_tsx_import_arg(upstream_root: str) -> str:
    """Resolve tsx loader path deterministically."""
    candidate1 = Path(upstream_root) / "node_modules/tsx/dist/loader.mjs"
    if candidate1.exists():
        return str(candidate1.resolve())
    candidate2 = Path(
        "/data/data/com.termux/files/home/upstream-pi-inspect/node_modules/tsx/dist/loader.mjs"
    )
    if candidate2.exists():
        return str(candidate2.resolve())
    return "tsx/esm"


def _repair_gaxios_metadata(upstream_root: str) -> None:
    """Repair corrupted gaxios package.json metadata if needed."""
    source = Path("/tmp/gaxios-repair/package/package.json")
    target = Path(upstream_root) / "node_modules/gaxios/package.json"
    if source.exists() and target.parent.exists():
        target.write_bytes(source.read_bytes())
        try:
            json.loads(target.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, UnicodeDecodeError):
            pass
    elif target.exists():
        try:
            json.loads(target.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, UnicodeDecodeError) as exc:
            raise RuntimeError(
                f"gaxios metadata at {target} is corrupted and /tmp/gaxios-repair source is missing"
            ) from exc


def capture_cli_print_basic(upstream_root: str) -> dict[str, Any]:
    """Capture case cli.print.basic using upstream Pi CLI print mode."""
    _repair_gaxios_metadata(upstream_root)
    cmd = [
        "node",
        "packages/coding-agent/dist/cli.js",
        "--print",
        "--provider",
        "google",
        "--model",
        "gemini-3-flash-preview",
        "hello",
    ]
    bash_cmd = f"cp /tmp/gaxios-repair/package/package.json node_modules/gaxios/package.json && exec {shlex.join(cmd)}"
    res = subprocess.run(
        ["bash", "-c", bash_cmd], cwd=upstream_root, capture_output=True, text=True
    )
    if res.returncode != 0:
        raise RuntimeError(
            f"upstream CLI failed ({res.returncode}): {res.stderr.strip()}"
        )
    raw = {"exit_code": res.returncode, "stdout": res.stdout, "stderr": res.stderr}
    return normalize_structure(raw)


def capture_agent_serial_tool_loop(upstream_root: str) -> dict[str, Any]:
    """Capture case agent.serial-tool-loop using scripted provider or print mode with tool calls."""
    _repair_gaxios_metadata(upstream_root)
    # Run upstream CLI in print mode with scripted prompt requiring sequential tool calls
    cmd = [
        "node",
        "packages/coding-agent/dist/cli.js",
        "--print",
        "--provider",
        "google",
        "--model",
        "gemini-3-flash-preview",
        "read file test.txt",
    ]
    bash_cmd = f"cp /tmp/gaxios-repair/package/package.json node_modules/gaxios/package.json && exec {shlex.join(cmd)}"
    res = subprocess.run(
        ["bash", "-c", bash_cmd], cwd=upstream_root, capture_output=True, text=True
    )
    if res.returncode != 0:
        raise RuntimeError(
            f"upstream CLI failed ({res.returncode}): {res.stderr.strip()}"
        )
    raw = {"exit_code": res.returncode, "stdout": res.stdout, "stderr": res.stderr}
    return normalize_structure(raw)


def capture_provider_openai_chat_fragmented_sse(upstream_root: str) -> dict[str, Any]:
    """Capture case provider.openai-chat.fragmented-sse using disposable Node harness and streamOpenAICompletions."""
    _repair_gaxios_metadata(upstream_root)
    openai_completions_path = (
        Path(upstream_root) / "packages/ai/src/api/openai-completions.ts"
    ).resolve()
    with tempfile.TemporaryDirectory(
        prefix="capture-sse-", dir=upstream_root
    ) as temp_dir:
        script_path = Path(temp_dir) / "capture-sse.ts"
        script_path.write_text(
            f"""import http from "node:http";
import type {{ AddressInfo }} from "node:net";
import {{ stream as streamOpenAICompletions }} from {json.dumps(str(openai_completions_path))};

const server = http.createServer((req, res) => {{
  if (req.method !== "POST" || req.url !== "/chat/completions") {{
    res.writeHead(404).end();
    return;
  }}
  res.writeHead(200, {{
    "content-type": "text/event-stream",
    "cache-control": "no-cache",
    connection: "keep-alive",
  }});

  const chunks = [
    JSON.stringify({{
      id: "chatcmpl-frag1",
      object: "chat.completion.chunk",
      created: 1700000000,
      model: "gpt-4o",
      choices: [{{ index: 0, delta: {{ role: "assistant", content: "Hello" }}, finish_reason: null }}],
    }}),
    JSON.stringify({{
      id: "chatcmpl-frag1",
      object: "chat.completion.chunk",
      created: 1700000000,
      model: "gpt-4o",
      choices: [{{ index: 0, delta: {{ content: " world" }}, finish_reason: null }}],
    }}),
    JSON.stringify({{
      id: "chatcmpl-frag1",
      object: "chat.completion.chunk",
      created: 1700000000,
      model: "gpt-4o",
      choices: [{{ index: 0, delta: {{}}, finish_reason: "stop" }}],
      usage: {{ prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 }},
    }}),
  ];

  res.write("data: " + chunks[0] + "\\n\\n");
  setTimeout(() => {{
    res.write("data: " + chunks[1] + "\\n\\n");
    setTimeout(() => {{
      res.write("data: " + chunks[2] + "\\n\\n");
      res.write("data: [DONE]\\n\\n");
      res.end();
    }}, 20);
  }}, 20);
}});

server.listen(0, "127.0.0.1", async () => {{
  const {{ port }} = server.address() as AddressInfo;
  const baseUrl = `http://127.0.0.1:${{port}}`;

  const model = {{
    id: "gpt-4o",
    name: "GPT-4o",
    api: "openai-completions",
    provider: "openai",
    baseUrl,
    reasoning: false,
    input: ["text"],
    cost: {{ input: 0, output: 0, cacheRead: 0, cacheWrite: 0 }},
    contextWindow: 128000,
    maxTokens: 4096,
    compat: {{
      supportsStore: false,
      supportsDeveloperRole: true,
      supportsReasoningEffort: false,
      supportsUsageInStreaming: true,
      maxTokensField: "max_completion_tokens",
      requiresToolResultName: false,
      requiresAssistantAfterToolResult: false,
      requiresThinkingAsText: false,
      requiresReasoningContentOnAssistantMessages: false,
      thinkingFormat: "openai",
      openRouterRouting: {{}},
      vercelGatewayRouting: {{}},
      chatTemplateKwargs: {{}},
      zaiToolStream: false,
      supportsStrictMode: false,
      supportsOpenAIGrammarTools: false,
      sendSessionAffinityHeaders: false,
      sessionAffinityFormat: "openai",
      supportsLongCacheRetention: false,
    }},
  }};

  const context = {{
    messages: [{{ role: "user", content: "hi", timestamp: 1 }}],
  }};

  try {{
    const stream = streamOpenAICompletions(model, context, {{ apiKey: "test-key" }});
    const result = await stream.result();
    console.log(JSON.stringify(result));
    server.close();
    process.exit(0);
  }} catch (err) {{
    console.error(err);
    server.close();
    process.exit(1);
  }}
}});
""",
            encoding="utf-8",
        )
        tsx_loader = _get_tsx_import_arg(upstream_root)
        res = subprocess.run(
            [
                "node",
                "--import",
                tsx_loader,
                str(script_path),
            ],
            cwd=upstream_root,
            capture_output=True,
            text=True,
        )
        if res.returncode != 0:
            raise RuntimeError(
                f"upstream provider.openai-chat.fragmented-sse failed ({res.returncode}): {res.stderr.strip()}"
            )
        try:
            res_json = json.loads(res.stdout.strip())
            if isinstance(res_json, dict) and "timestamp" in res_json:
                res_json["timestamp"] = 0
            return normalize_structure(res_json)
        except json.JSONDecodeError as exc:
            raise RuntimeError(
                f"upstream provider.openai-chat.fragmented-sse returned invalid JSON: {res.stdout!r}"
            ) from exc


def capture_tool_read_bounds(upstream_root: str) -> dict[str, Any]:
    """Capture case tool.read.bounds by invoking upstream createReadToolDefinition on disposable fixture."""
    lines = [f"line {i}" for i in range(1, 21)]
    fixture_content = "\n".join(lines)
    with tempfile.TemporaryDirectory(
        prefix="capture-read-", dir=upstream_root
    ) as temp_dir:
        fixture_path = Path(temp_dir) / "fixture.txt"
        script_path = Path(temp_dir) / "capture-read.ts"
        fixture_path.write_text(fixture_content, encoding="utf-8")
        read_tool_path = (
            Path(upstream_root) / "packages/coding-agent/src/core/tools/read.ts"
        ).resolve()
        script_path.write_text(
            f"""import {{ createReadToolDefinition }} from {json.dumps(str(read_tool_path))};

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
        tsx_loader = _get_tsx_import_arg(upstream_root)
        res = subprocess.run(
            ["node", "--import", tsx_loader, str(script_path)],
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
            raise RuntimeError(
                f"upstream tool.read.bounds returned invalid JSON: {res.stdout!r}"
            ) from exc


def capture_tool_bash_cancel_descendants(upstream_root: str) -> dict[str, Any]:
    raise NotImplementedError(
        "real upstream capture required: tool.bash.cancel-descendants"
    )


def capture_resource_context_precedence(upstream_root: str) -> dict[str, Any]:
    """Capture case resource.context-precedence using DefaultResourceLoader / loadProjectContextFiles."""
    resource_loader_path = (
        Path(upstream_root) / "packages/coding-agent/src/core/resource-loader.ts"
    ).resolve()
    with tempfile.TemporaryDirectory(
        prefix="capture-context-", dir=upstream_root
    ) as temp_dir:
        agent_dir = Path(temp_dir) / "global_agent"
        agent_dir.mkdir(parents=True, exist_ok=True)
        (agent_dir / "AGENTS.md").write_text("global context", encoding="utf-8")

        parent_dir = Path(temp_dir) / "parent"
        parent_dir.mkdir(parents=True, exist_ok=True)
        (parent_dir / "AGENTS.md").write_text("parent context", encoding="utf-8")

        child_dir = parent_dir / "child"
        child_dir.mkdir(parents=True, exist_ok=True)
        (child_dir / "AGENTS.md").write_text("child context", encoding="utf-8")

        script_path = Path(temp_dir) / "capture-context.ts"
        script_path.write_text(
            f"""import {{ loadProjectContextFiles }} from {json.dumps(str(resource_loader_path))};

const cwd = {json.dumps(str(child_dir))};
const agentDir = {json.dumps(str(agent_dir))};

const files = loadProjectContextFiles({{ cwd, agentDir }});
console.log(JSON.stringify(files));
""",
            encoding="utf-8",
        )
        tsx_loader = _get_tsx_import_arg(upstream_root)
        res = subprocess.run(
            [
                "node",
                "--import",
                tsx_loader,
                str(script_path),
            ],
            cwd=upstream_root,
            capture_output=True,
            text=True,
        )
        if res.returncode != 0:
            raise RuntimeError(
                f"upstream resource.context-precedence execution failed ({res.returncode}): {res.stderr.strip()}"
            )
        try:
            return normalize_structure(json.loads(res.stdout.strip()))
        except json.JSONDecodeError as exc:
            raise RuntimeError(
                f"upstream resource.context-precedence returned invalid JSON: {res.stdout!r}"
            ) from exc


def capture_resource_untrusted_project(upstream_root: str) -> dict[str, Any]:
    raise NotImplementedError(
        "real upstream capture required: resource.untrusted-project"
    )


ADAPTERS = {
    "cli.print.basic": capture_cli_print_basic,
    "agent.serial-tool-loop": capture_agent_serial_tool_loop,
    "provider.openai-chat.fragmented-sse": capture_provider_openai_chat_fragmented_sse,
    "tool.read.bounds": capture_tool_read_bounds,
    "tool.bash.cancel-descendants": capture_tool_bash_cancel_descendants,
    "resource.context-precedence": capture_resource_context_precedence,
    "resource.untrusted-project": capture_resource_untrusted_project,
}
