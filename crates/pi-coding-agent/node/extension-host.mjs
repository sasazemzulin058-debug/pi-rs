import readline from "node:readline";
import { pathToFileURL } from "node:url";

const entry = process.argv[2];
const send = value => process.stdout.write(JSON.stringify(value) + "\n");
const fail = (code, message) => { send({ kind: "error", code, message }); process.exit(0); };

const rl = readline.createInterface({ input: process.stdin });
const firstLine = await new Promise(resolve => rl.once("line", resolve));
try {
  const handshake = JSON.parse(firstLine);
  if (handshake.kind !== "pi-extension-host" || handshake.version !== 1) throw new Error("unsupported handshake");
} catch (e) { fail("PROTOCOL_ERROR", String(e?.message ?? e)); }
let factory;
try {
  if (!entry.endsWith(".js")) throw new Error("only .js extension entries supported");
  const mod = await import(pathToFileURL(entry).href);
  factory = mod.default;
  if (typeof factory !== "function") throw new Error("default export must be callable ExtensionFactory");
} catch (e) { fail("EXTENSION_INCOMPATIBLE", String(e?.message ?? e)); }
const handlers = [];
const pi = { on(type, handler) {
  if (type !== "tool_call") throw new Error(`unsupported event: ${type}`);
  if (typeof handler !== "function") throw new Error("tool_call handler must be callable");
  handlers.push(handler);
} };
try { await factory(pi); }
catch (e) { fail("EXTENSION_INCOMPATIBLE", String(e?.message ?? e)); }

send({ kind: "pi-extension-host", version: 1 });
rl.on("line", async line => {
  let request;
  try { request = JSON.parse(line); } catch { return fail("PROTOCOL_ERROR", "malformed request"); }
  if (request.kind !== "hook" || typeof request.id !== "number") return send({ kind: "response", id: request.id, error: "invalid request" });
  const event = { type: "tool_call", toolName: request.toolName, toolCallId: request.toolCallId, input: request.input };
  try {
    for (const handler of handlers) {
      const result = await handler(event, {});
      if (result?.block === true) return send({ kind: "response", id: request.id, block: true, reason: result.reason ?? null, input: event.input });
    }
    send({ kind: "response", id: request.id, block: false, input: event.input });
  } catch (e) { send({ kind: "response", id: request.id, error: String(e?.message ?? e) }); }
});
