import { createInterface } from "readline";
import { handleRequest } from "./handler.js";

const isMock = process.argv.includes("--mock");

const rl = createInterface({ input: process.stdin, terminal: false });

function send(msg: unknown): void {
  process.stdout.write(JSON.stringify(msg) + "\n");
}

rl.on("line", async (line) => {
  if (!line.trim()) return;

  let parsed: unknown;
  try {
    parsed = JSON.parse(line);
  } catch {
    send({
      jsonrpc: "2.0",
      id: null,
      error: { code: -32700, message: "Parse error" },
    });
    return;
  }

  const msg = parsed as { jsonrpc?: string; id?: unknown; method?: string; params?: unknown };
  if (msg.jsonrpc !== "2.0" || typeof msg.method !== "string") {
    send({
      jsonrpc: "2.0",
      id: msg.id ?? null,
      error: { code: -32600, message: "Invalid Request" },
    });
    return;
  }

  try {
    const result = await handleRequest(msg.method, msg.params, msg.id, send, isMock);
    if (msg.id !== undefined && msg.id !== null) {
      send({ jsonrpc: "2.0", id: msg.id, result });
    }
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : String(err);
    if (msg.id !== undefined && msg.id !== null) {
      send({
        jsonrpc: "2.0",
        id: msg.id,
        error: { code: -32000, message },
      });
    }
  }
});

rl.on("close", () => {
  process.exit(0);
});
