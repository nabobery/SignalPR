import { runMockReview, runMockReviewInteractive, cancelMockReview, resolveMockPermission } from "./mock.js";
import { runRealReview, cancelActiveSession } from "./real.js";
import os from "os";
import { createRequire } from "module";

type SendFn = (msg: unknown) => void;

let sdkVersion = "unknown";
try {
  const require = createRequire(import.meta.url);
  const pkg = require("@anthropic-ai/claude-agent-sdk/package.json");
  sdkVersion = pkg.version ?? "unknown";
} catch {
  // SDK not available in mock-only builds
}

let reviewInFlight = false;

export async function handleRequest(
  method: string,
  params: unknown,
  _id: unknown,
  send: SendFn,
  isMock: boolean,
): Promise<unknown> {
  switch (method) {
    case "health.check":
      if (!isMock && process.env.ANTHROPIC_API_KEY) {
        const sdk = await import("@anthropic-ai/claude-agent-sdk");
        const warmQuery = sdk.query({
          prompt: "Initialize the Claude Code review session.",
          options: {
            allowedTools: ["Read"],
            cwd: process.cwd(),
            permissionMode: "plan",
            persistSession: false,
            settingSources: [],
          },
        });
        for await (const message of warmQuery) {
          if (message.type === "system" && message.subtype === "init") {
            await warmQuery.interrupt();
            break;
          }
          if (message.type === "result") {
            break;
          }
        }
      }
      return {
        status: "ok",
        bridge_version: "0.1.0",
        mode: isMock ? "mock" : "real",
        sdk_version: sdkVersion,
        env: {
          CLAUDE_CONFIG_DIR: process.env.CLAUDE_CONFIG_DIR ?? null,
          CLAUDE_CODE_TMPDIR: process.env.CLAUDE_CODE_TMPDIR ?? null,
          CLAUDE_CODE_SKIP_PROMPT_HISTORY: process.env.CLAUDE_CODE_SKIP_PROMPT_HISTORY ?? null,
        },
        runtime: {
          homedir: os.homedir(),
          tmpdir: os.tmpdir(),
          cwd: process.cwd(),
          platform: os.platform(),
        },
      };

    case "review.start": {
      const p = params as {
        lane_id: string;
        system_prompt: string;
        diff: string;
        output_schema: string;
        cwd: string;
      };
      if (!p || !p.lane_id || !p.diff || !p.output_schema || !p.cwd) {
        throw new Error("Missing required params: lane_id, diff, output_schema, cwd");
      }
      if (reviewInFlight) {
        throw new Error("Bridge already has an active review.");
      }
      reviewInFlight = true;
      const onSettled = () => {
        reviewInFlight = false;
      };
      setTimeout(() => {
        if (isMock) {
          void runMockReview(p, send).finally(onSettled);
        } else {
          runRealReview(p, send, onSettled);
        }
      }, 0);
      return { started: true };
    }

    case "review.start_interactive": {
      const p = params as {
        lane_id: string;
        system_prompt: string;
        diff: string;
        output_schema: string;
        cwd: string;
      };
      if (!p || !p.lane_id || !p.diff || !p.output_schema || !p.cwd) {
        throw new Error("Missing required params: lane_id, diff, output_schema, cwd");
      }
      if (reviewInFlight) {
        throw new Error("Bridge already has an active review.");
      }
      reviewInFlight = true;
      const onSettledInteractive = () => {
        reviewInFlight = false;
      };
      setTimeout(() => {
        if (isMock) {
          void runMockReviewInteractive(p, send).finally(onSettledInteractive);
        } else {
          runRealReview(p, send, onSettledInteractive);
        }
      }, 0);
      return { started: true, mode: "interactive" };
    }

    case "review.resolve_permission": {
      const rp = params as { request_id: string; approved: boolean };
      if (!rp || !rp.request_id) {
        throw new Error("Missing required param: request_id");
      }
      if (isMock) {
        const resolved = resolveMockPermission(rp.request_id, !!rp.approved);
        return { resolved };
      }
      return { resolved: false };
    }

    case "review.cancel":
      cancelMockReview();
      cancelActiveSession();
      reviewInFlight = false;
      return { cancelled: true };

    case "bridge.shutdown":
      setTimeout(() => process.exit(0), 50);
      return { shutting_down: true };

    default:
      throw new Error(`Unknown method: ${method}`);
  }
}
