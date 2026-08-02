import json
import os
import re
import shlex
import subprocess
import sys
import tempfile
from contextlib import suppress
from pathlib import Path
from typing import Any, cast

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


def _parse_strict_jsonl(
    stdout: str, stderr: str, returncode: int
) -> list[dict[str, Any]]:
    """Parse a runner protocol without hiding diagnostics or malformed lines."""
    if returncode != 0:
        raise RuntimeError(f"runner exited with status {returncode}; stderr={stderr!r}")
    if stderr:
        raise RuntimeError(f"runner wrote stderr: {stderr!r}")
    if not stdout.strip():
        raise RuntimeError("runner produced empty stdout")
    records: list[dict[str, Any]] = []
    for number, line in enumerate(stdout.splitlines(), 1):
        try:
            value = json.loads(line)
        except json.JSONDecodeError as exc:
            raise RuntimeError(f"runner stdout line {number} is not JSON") from exc
        if not isinstance(value, dict):
            raise RuntimeError(f"runner stdout line {number} is not a JSON object")
        records.append(value)
    return records


def _normalize_capture(value: Any, disposable_root: str, upstream_root: str) -> Any:
    """Normalize only instability owned by the disposable capture runner."""
    ids_by_kind: dict[str, dict[str, str]] = {"session": {}, "entry": {}}
    counters = {"session": 0, "entry": 0}

    def walk(item: Any) -> Any:
        if isinstance(item, str):
            item = item.replace(disposable_root, "__CAPTURE_ROOT__").replace(
                upstream_root, "__UPSTREAM_ROOT__"
            )
            if re.fullmatch(r"(?:\d{4}-\d{2}-\d{2}T[^ ]+|\d{10,13})", item):
                return "0"
            return item
        if isinstance(item, list):
            return [walk(v) for v in item]
        if isinstance(item, dict):
            result = {}
            for key, val in item.items():
                if key in {
                    "timestamp",
                    "createdAt",
                    "updatedAt",
                    "created_at",
                } and isinstance(val, (int, float, str)):
                    result[key] = 0
                    continue
                if key in {
                    "sessionId",
                    "session_id",
                    "entryId",
                    "entry_id",
                } and isinstance(val, str):
                    kind = "session" if "session" in key.lower() else "entry"
                    ids = ids_by_kind[kind]
                    if val not in ids:
                        counters[kind] += 1
                        ids[val] = f"{kind}-{counters[kind]}"
                    result[key] = ids[val]
                else:
                    result[key] = walk(val)
            return result
        return item

    return cast(Any, walk(value))


def _unsupported_capture(case: str, upstream_root: str) -> dict[str, Any]:
    """Do not publish a fixture until the pinned production seam is executable."""
    raise RuntimeError(
        f"{case}: production capture seam not verified in this environment; pinned root={upstream_root}"
    )


def capture_cli_json_events(upstream_root: str) -> dict[str, Any]:
    """Capture JSONL from pinned upstream's production print-mode seam."""
    loader = _get_tsx_import_arg(upstream_root)
    if loader == "tsx/esm":
        raise RuntimeError(
            f"cli.json-events: pinned tsx loader not found under {upstream_root}"
        )
    runner = r"""import { createAssistantMessageEventStream } from "__UPSTREAM__/node_modules/@earendil-works/pi-ai/dist/index.js";
import { AuthStorage } from "__UPSTREAM__/packages/coding-agent/src/core/auth-storage.ts";
import { ModelRuntime } from "__UPSTREAM__/packages/coding-agent/src/core/model-runtime.ts";
import { runPrintMode } from "__UPSTREAM__/packages/coding-agent/src/modes/print-mode.ts";
import { createAgentSessionRuntime, createAgentSessionServices, createAgentSessionFromServices } from "__UPSTREAM__/packages/coding-agent/src/core/agent-session-runtime.ts";
import { SessionManager } from "__UPSTREAM__/packages/coding-agent/src/core/session-manager.ts";
const cwd = process.cwd();
const agentDir = __AGENT_DIR__;
const model = { id: "capture-model", name: "Capture Model", api: "anthropic-messages", provider: "capture", baseUrl: "http://127.0.0.1:1", reasoning: false, input: ["text"], cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 }, contextWindow: 128000, maxTokens: 256 };
const sessionManager = SessionManager.inMemory();
const modelRuntime = await ModelRuntime.create({
  credentials: AuthStorage.inMemory({ capture: { type: "api_key", key: "capture-key" } }),
  modelsPath: null,
  allowModelNetwork: false,
});
const make = async ({ sessionManager, sessionStartEvent, cwd }: any) => {
  const services = await createAgentSessionServices({ cwd, agentDir, modelRuntime });
  const result = await createAgentSessionFromServices({ services, sessionManager, model, sessionStartEvent });
  result.session.agent.streamFunction = () => { const stream = createAssistantMessageEventStream(); queueMicrotask(() => { const message = { role: "assistant", content: [{ type: "text", text: "fixed capture" }], api: model.api, provider: model.provider, model: model.id, usage: { input: 1, output: 2, cacheRead: 0, cacheWrite: 0, totalTokens: 3, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } }, stopReason: "stop", timestamp: 0 }; stream.push({ type: "start", partial: { ...message, content: [], stopReason: "pending" } }); stream.push({ type: "text_start", contentIndex: 0, partial: { ...message, content: [{ type: "text", text: "" }], stopReason: "pending" } }); stream.push({ type: "text_delta", contentIndex: 0, delta: "fixed capture", partial: { ...message, content: [{ type: "text", text: "fixed capture" }], stopReason: "pending" } }); stream.push({ type: "text_end", contentIndex: 0, content: "fixed capture", partial: { ...message, content: [{ type: "text", text: "fixed capture" }], stopReason: "pending" } }); stream.push({ type: "done", reason: "stop", message }); }); return stream; };
  return result;
};
const runtimeHost = await createAgentSessionRuntime(make, { cwd, agentDir, sessionManager });
process.exitCode = await runPrintMode(runtimeHost, { mode: "json", initialMessage: "capture" });
"""
    with tempfile.TemporaryDirectory(prefix="pi-json-capture-") as temp:
        temp_root = Path(temp).resolve()
        agent_dir = temp_root / "agent-dir"
        agent_dir.mkdir()
        path = temp_root / "runner.mts"
        path.write_text(
            runner.replace("__UPSTREAM__", str(Path(upstream_root).resolve())).replace(
                "__AGENT_DIR__", json.dumps(str(agent_dir))
            ),
            encoding="utf-8",
        )
        res = subprocess.run(
            ["node", "--import", loader, str(path)],
            cwd=upstream_root,
            capture_output=True,
            text=True,
        )
    records = _parse_strict_jsonl(res.stdout, res.stderr, res.returncode)
    return cast(
        dict[str, Any],
        _normalize_capture(
            {"exit_code": res.returncode, "events": records},
            str(temp_root),
            upstream_root,
        ),
    )


def capture_agent_retry_auto_compaction(upstream_root: str) -> dict[str, Any]:
    return _unsupported_capture("agent.retry-auto-compaction", upstream_root)


def _repair_gaxios_metadata(upstream_root: str) -> None:
    """Repair corrupted gaxios package.json metadata if needed."""
    source = Path("/tmp/gaxios-repair/package/package.json")
    target = Path(upstream_root) / "node_modules/gaxios/package.json"
    if source.exists() and target.parent.exists():
        target.write_bytes(source.read_bytes())
        with suppress(json.JSONDecodeError, UnicodeDecodeError):
            json.loads(target.read_text(encoding="utf-8"))
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
    return cast(dict[str, Any], normalize_structure(raw))


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
    return cast(dict[str, Any], normalize_structure(raw))


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
            return cast(dict[str, Any], normalize_structure(res_json))
        except json.JSONDecodeError as exc:
            raise RuntimeError(
                f"upstream provider.openai-chat.fragmented-sse returned invalid JSON: {res.stdout!r}"
            ) from exc


def capture_tool_edit(upstream_root: str) -> dict[str, Any]:
    """Capture the pinned upstream edit definition against disposable files."""
    with tempfile.TemporaryDirectory(
        prefix="capture-edit-", dir=upstream_root
    ) as temp_dir:
        script_path = Path(temp_dir) / "capture-edit.ts"
        edit_path = (
            Path(upstream_root) / "packages/coding-agent/src/core/tools/edit.ts"
        ).resolve()
        script_path.write_text(
            f"""import {{ readFile, writeFile }} from "node:fs/promises";
import {{ createEditToolDefinition }} from {json.dumps(str(edit_path))};

const root = {json.dumps(temp_dir)};
const tool = createEditToolDefinition(root);
const fs = await import("node:fs/promises");
const cases = [];
async function run(name, file, edits, initial) {{
  const path = `${{root}}/${{file}}`;
  if (initial !== undefined) await writeFile(path, initial, "utf8");
  try {{
    const result = await tool.execute("capture", {{ path: file, edits }});
    cases.push({{ name, ok: true, result, bytes: Array.from(await readFile(path)) }});
  }} catch (error) {{
    cases.push({{ name, ok: false, error: error instanceof Error ? error.message : String(error), bytes: await fs.readFile(path).then(b => Array.from(b)).catch(() => null) }});
  }}
}}
await run("multiple-disjoint", "multiple.txt", [
  {{ oldText: "alpha", newText: "A" }}, {{ oldText: "gamma", newText: "G" }}
], "alpha\\nbeta\\ngamma\\n");
await run("duplicate-oldText", "duplicate.txt", [{{ oldText: "x", newText: "y" }}], "x\\nx\\n");
await run("overlapping", "overlap.txt", [{{ oldText: "abcd", newText: "A" }}, {{ oldText: "bc", newText: "B" }}], "abcd\\n");
await run("missing-file", "missing.txt", [{{ oldText: "x", newText: "y" }}]);
await run("bom-crlf", "bom-crlf.txt", [{{ oldText: "one", newText: "ONE" }}], "\\ufeffone\\r\\ntwo\\r\\n");
console.log(JSON.stringify({{ cases }}));
""",
            encoding="utf-8",
        )
        res = subprocess.run(
            ["node", "--import", _get_tsx_import_arg(upstream_root), str(script_path)],
            cwd=upstream_root,
            capture_output=True,
            text=True,
        )
        if res.returncode != 0:
            raise RuntimeError(
                f"upstream tool.edit execution failed ({res.returncode}): {res.stderr.strip()}"
            )
        try:
            normalized = normalize_structure(json.loads(res.stdout.strip()))
            if not isinstance(normalized, dict):
                raise ValueError("upstream tool.edit output must be a JSON object")
            return cast(dict[str, Any], normalized)
        except (json.JSONDecodeError, ValueError) as exc:
            raise RuntimeError(
                f"upstream tool.edit returned invalid JSON: {res.stdout!r}"
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
            return cast(
                dict[str, Any], normalize_structure(json.loads(res.stdout.strip()))
            )
        except json.JSONDecodeError as exc:
            raise RuntimeError(
                f"upstream tool.read.bounds returned invalid JSON: {res.stdout!r}"
            ) from exc


def capture_tool_bash_cancel_descendants(upstream_root: str) -> dict[str, Any]:
    """Capture case tool.bash.cancel-descendants using upstream createBashToolDefinition."""
    bash_tool_path = (
        Path(upstream_root) / "packages/coding-agent/src/core/tools/bash.ts"
    ).resolve()
    with tempfile.TemporaryDirectory(
        prefix="capture-bash-", dir=upstream_root
    ) as temp_dir:
        pid_file = Path(temp_dir) / "descendant.pid"
        script_path = Path(temp_dir) / "capture-bash.ts"
        script_path.write_text(
            f"""import {{ createBashToolDefinition }} from {json.dumps(str(bash_tool_path))};

const tool = createBashToolDefinition({json.dumps(temp_dir)});
const pidFile = {json.dumps(str(pid_file))};

const abortController = new AbortController();

const execPromise = tool.execute(
  "bash-1",
  {{ command: `sleep 100 & echo $! > "${{pidFile}}" && wait` }},
  abortController.signal,
  undefined,
  undefined
);

// Wait for descendant pid file to be written
let pid = "";
for (let i = 0; i < 50; i++) {{
  try {{
    const fs = await import("fs");
    if (fs.existsSync(pidFile)) {{
      pid = fs.readFileSync(pidFile, "utf-8").trim();
      if (pid) break;
    }}
  }} catch (_) {{}}
  await new Promise((r) => setTimeout(r, 100));
}}

abortController.abort();

let res;
try {{
  res = await execPromise;
}} catch (err) {{
  res = {{ error: String(err) }};
}}

// Check if descendant process is dead
let isAlive = false;
if (pid) {{
  try {{
    process.kill(Number(pid), 0);
    isAlive = true;
  }} catch (_) {{
    isAlive = false;
  }}
}}

console.log(JSON.stringify({{ res, descendant_pid: pid, descendant_alive: isAlive }}));
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
                f"upstream tool.bash.cancel-descendants execution failed ({res.returncode}): {res.stderr.strip()}"
            )
        try:
            return cast(
                dict[str, Any], normalize_structure(json.loads(res.stdout.strip()))
            )
        except json.JSONDecodeError as exc:
            raise RuntimeError(
                f"upstream tool.bash.cancel-descendants returned invalid JSON: {res.stdout!r}"
            ) from exc


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
            return cast(
                dict[str, Any], normalize_structure(json.loads(res.stdout.strip()))
            )
        except json.JSONDecodeError as exc:
            raise RuntimeError(
                f"upstream resource.context-precedence returned invalid JSON: {res.stdout!r}"
            ) from exc


def capture_resource_untrusted_project(upstream_root: str) -> dict[str, Any]:
    """Capture case resource.untrusted-project using project trust modules."""
    project_trust_path = (
        Path(upstream_root) / "packages/coding-agent/src/core/project-trust.ts"
    ).resolve()
    trust_manager_path = (
        Path(upstream_root) / "packages/coding-agent/src/core/trust-manager.ts"
    ).resolve()
    with tempfile.TemporaryDirectory(
        prefix="capture-trust-", dir=upstream_root
    ) as temp_dir:
        agent_dir = Path(temp_dir) / "agent_dir"
        untrusted_dir = Path(temp_dir) / "untrusted_proj"
        untrusted_dir.mkdir(parents=True, exist_ok=True)
        (untrusted_dir / ".pi").mkdir(parents=True, exist_ok=True)
        (untrusted_dir / ".pi" / "settings.json").write_text("{}", encoding="utf-8")

        script_path = Path(temp_dir) / "capture-trust.ts"
        script_path.write_text(
            f"""import {{ resolveProjectTrusted }} from {json.dumps(str(project_trust_path))};
import {{ ProjectTrustStore, hasTrustRequiringProjectResources }} from {json.dumps(str(trust_manager_path))};

const agentDir = {json.dumps(str(agent_dir))};
const untrustedDir = {json.dumps(str(untrusted_dir))};

const trustStore = new ProjectTrustStore(agentDir);
const hasResources = hasTrustRequiringProjectResources(untrustedDir);

const initialDecision = trustStore.get(untrustedDir);
const initialResolved = await resolveProjectTrusted({{
  cwd: untrustedDir,
  trustStore,
  defaultProjectTrust: "never",
  projectTrustContext: {{ hasUI: false, ui: {{ select: async () => undefined }} as any }},
}});

trustStore.set(untrustedDir, true);
const updatedDecision = trustStore.get(untrustedDir);
const updatedResolved = await resolveProjectTrusted({{
  cwd: untrustedDir,
  trustStore,
  projectTrustContext: {{ hasUI: false, ui: {{ select: async () => undefined }} as any }},
}});

console.log(JSON.stringify({{
  has_resources: hasResources,
  initial_decision: initialDecision,
  initial_resolved: initialResolved,
  updated_decision: updatedDecision,
  updated_resolved: updatedResolved,
}}));
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
                f"upstream resource.untrusted-project execution failed ({res.returncode}): {res.stderr.strip()}"
            )
        try:
            return cast(
                dict[str, Any], normalize_structure(json.loads(res.stdout.strip()))
            )
        except json.JSONDecodeError as exc:
            raise RuntimeError(
                f"upstream resource.untrusted-project returned invalid JSON: {res.stdout!r}"
            ) from exc


ADAPTERS = {
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
