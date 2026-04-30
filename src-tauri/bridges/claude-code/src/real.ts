import { realpath } from "fs/promises";
import path from "path";
import type { HookInput, PreToolUseHookInput, Query } from "@anthropic-ai/claude-agent-sdk";

type SendFn = (msg: unknown) => void;

export interface ReviewParams {
  lane_id: string;
  system_prompt: string;
  diff: string;
  output_schema: string;
  cwd: string;
}

function notification(method: string, params: unknown): unknown {
  return { jsonrpc: "2.0", method, params };
}

const ALLOWED_TOOLS = ["Read", "Glob", "Grep"] as const;
const DISALLOWED_TOOLS = [
  "Agent",
  "Task",
  "Bash",
  "Edit",
  "Write",
  "NotebookEdit",
  "Monitor",
  "TodoWrite",
  "WebFetch",
  "WebSearch",
] as const;

type ActiveQueryHandle = {
  interrupt: () => Promise<void>;
};

let activeAbortController: AbortController | null = null;
let activeQuery: ActiveQueryHandle | null = null;

export function cancelActiveSession(): void {
  activeAbortController?.abort();
  if (activeQuery) {
    void activeQuery.interrupt().catch(() => {});
  }
  activeAbortController = null;
  activeQuery = null;
}

export function runRealReview(
  params: ReviewParams,
  send: SendFn,
  onSettled: () => void,
): void {
  const { lane_id, system_prompt, diff, output_schema, cwd } = params;
  const resolvedCwd = path.resolve(cwd);
  const abortController = new AbortController();
  activeAbortController = abortController;
  const rootPathPromise = canonicalizePath(resolvedCwd);
  let session: Query | null = null;

  void (async () => {
    try {
      if (!process.env.ANTHROPIC_API_KEY) {
        send(
          notification("review.error", {
            lane_id,
            error: "ANTHROPIC_API_KEY environment variable is not set.",
          }),
        );
        return;
      }

      let outputSchema: Record<string, unknown>;
      try {
        outputSchema = asRecord(JSON.parse(output_schema));
      } catch {
        send(
          notification("review.error", {
            lane_id,
            error: "Failed to parse output_schema JSON.",
          }),
        );
        return;
      }

      const sdk = await import("@anthropic-ai/claude-agent-sdk");
      const prompt = `${system_prompt}\n\n<diff>\n${diff}\n</diff>`;
      const rootPath = await rootPathPromise;

      session = sdk.query({
        prompt,
        options: {
          abortController,
          allowedTools: [...ALLOWED_TOOLS],
          canUseTool: async (toolName, input) => {
            sendDeniedToolRequest(
              send,
              lane_id,
              toolName,
              input,
              "Tool is not approved for read-only review mode.",
            );
            return {
              behavior: "deny",
              message: "SignalPR only allows read-only review tools in Claude Code.",
            };
          },
          cwd: resolvedCwd,
          disallowedTools: [...DISALLOWED_TOOLS],
          hooks: {
            PreToolUse: [
              {
                matcher: "Read|Glob|Grep",
                hooks: [
                  async (input: HookInput) => {
                    if (!isPreToolUseInput(input)) {
                      return {};
                    }
                    const decision = await validateReadOnlyPathAccess(
                      lane_id,
                      rootPath,
                      input,
                      send,
                    );
                    return {
                      hookSpecificOutput: {
                        hookEventName: "PreToolUse",
                        permissionDecision: decision.allowed ? "allow" : "deny",
                        permissionDecisionReason: decision.reason,
                      },
                    };
                  },
                ],
              },
              {
                matcher: "Agent|Task|Bash|Edit|Write|NotebookEdit|Monitor|TodoWrite|WebFetch|WebSearch",
                hooks: [
                  async (input: HookInput) => {
                    if (!isPreToolUseInput(input)) {
                      return {};
                    }
                    sendDeniedToolRequest(
                      send,
                      lane_id,
                      input.tool_name,
                      asRecord(input.tool_input),
                      "Denied by read-only review policy.",
                    );
                    return {
                      hookSpecificOutput: {
                        hookEventName: "PreToolUse",
                        permissionDecision: "deny",
                        permissionDecisionReason:
                          "SignalPR runs Claude Code in read-only review mode.",
                      },
                    };
                  },
                ],
              },
            ],
          },
          includePartialMessages: true,
          outputFormat: {
            type: "json_schema",
            schema: outputSchema,
          },
          permissionMode: "bypassPermissions",
          persistSession: false,
          settingSources: [],
        },
      });

      activeQuery = session;

      for await (const message of session) {
        if (abortController.signal.aborted) {
          break;
        }

        if (message.type === "stream_event") {
          const streamEvent = message.event;
          if (streamEvent.type === "content_block_delta" && streamEvent.delta?.type === "text_delta") {
            const chunk = streamEvent.delta.text ?? "";
            if (chunk) {
              send(notification("review.delta", { lane_id, chunk }));
            }
          }
          continue;
        }

        if (message.type !== "result") {
          continue;
        }

        if (message.subtype === "success" && message.structured_output) {
          send(
            notification("review.completed", {
              lane_id,
              output: message.structured_output,
              session_id: message.session_id ?? null,
              cost_usd: message.total_cost_usd ?? null,
              checkpoint_id: null,
            }),
          );
          return;
        }

        const messageText =
          message.subtype === "success"
            ? message.result
            : message.errors.join("; ") || "Claude Code did not return a structured review payload.";
        send(
          notification("review.error", {
            lane_id,
            error: `${message.subtype}: ${messageText}`,
          }),
        );
      }
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : String(err);
      send(
        notification("review.error", {
          lane_id,
          error: abortController.signal.aborted ? "Review cancelled." : message,
        }),
      );
    } finally {
      if (activeAbortController === abortController) {
        activeAbortController = null;
      }
      if (session && activeQuery === session) {
        void activeQuery.interrupt().catch(() => {});
        activeQuery = null;
      }
      onSettled();
    }
  })();
}

async function validateReadOnlyPathAccess(
  laneId: string,
  rootPath: string,
  input: PreToolUseHookInput,
  send: SendFn,
): Promise<{ allowed: boolean; reason: string }> {
  const toolInput = asRecord(input.tool_input);
  const rawPath = extractToolPath(input.tool_name, toolInput);
  if (!rawPath) {
    return { allowed: true, reason: "Read-only tool path validated." };
  }

  const candidatePath = path.isAbsolute(rawPath)
    ? rawPath
    : path.resolve(rootPath, rawPath);
  const canonicalPath = await canonicalizePath(candidatePath);
  if (canonicalPath === rootPath || canonicalPath.startsWith(`${rootPath}${path.sep}`)) {
    return { allowed: true, reason: "Read-only tool path validated." };
  }

  sendDeniedToolRequest(
    send,
    laneId,
    input.tool_name,
    toolInput,
    `Path '${rawPath}' resolves outside the workspace root.`,
  );
  return {
    allowed: false,
    reason: "Path resolves outside the workspace root.",
  };
}

function extractToolPath(
  toolName: string,
  toolInput: Record<string, unknown>,
): string | null {
  const candidate =
    typeof toolInput.file_path === "string"
      ? toolInput.file_path
      : typeof toolInput.path === "string"
        ? toolInput.path
        : null;

  if (!candidate) {
    return toolName === "Glob" || toolName === "Grep" ? "." : null;
  }

  return candidate;
}

async function canonicalizePath(targetPath: string): Promise<string> {
  try {
    return await realpath(targetPath);
  } catch {
    const parent = path.dirname(targetPath);
    const canonicalParent = await realpath(parent);
    return path.join(canonicalParent, path.basename(targetPath));
  }
}

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object"
    ? (value as Record<string, unknown>)
    : {};
}

function isPreToolUseInput(input: HookInput): input is PreToolUseHookInput {
  return input.hook_event_name === "PreToolUse";
}

function sendDeniedToolRequest(
  send: SendFn,
  laneId: string,
  toolName: string,
  toolInput: Record<string, unknown>,
  reason: string,
): void {
  send(
    notification("review.permission_requested", {
      lane_id: laneId,
      tool_name: toolName,
      tool_input: toolInput,
      reason,
      action: "denied",
    }),
  );
}
