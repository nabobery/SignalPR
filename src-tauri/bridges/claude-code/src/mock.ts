type SendFn = (msg: unknown) => void;

interface ReviewParams {
  lane_id: string;
  system_prompt: string;
  diff: string;
  output_schema: string;
  cwd: string;
}

function notification(method: string, params: unknown): unknown {
  return { jsonrpc: "2.0", method, params };
}

const MOCK_FINDINGS = [
  {
    file_path: "src/main.rs",
    line_start: 42,
    line_end: 42,
    severity: "warning",
    title: "Unused variable binding",
    body: "Variable `result` is assigned but never used in this scope.",
    confidence: 0.84,
    evidence: ["let result = expensive_call();"],
    agent_type: "security",
  },
  {
    file_path: "src/lib.rs",
    line_start: 10,
    line_end: 14,
    severity: "info",
    title: "Consider extracting helper",
    body: "This block of logic is repeated in two places.",
    confidence: 0.67,
    evidence: ["duplicated normalization branch"],
    agent_type: "architecture",
  },
];

let activeAbortController: AbortController | null = null;

export function cancelMockReview(): void {
  activeAbortController?.abort();
  activeAbortController = null;
}

export async function runMockReview(params: ReviewParams, send: SendFn): Promise<void> {
  const { lane_id } = params;
  const abortController = new AbortController();
  activeAbortController = abortController;

  try {
    send(notification("review.delta", {
      lane_id,
      chunk: "Analyzing code changes...\n",
    }));

    await sleep(50, abortController.signal);

    send(notification("review.delta", {
      lane_id,
      chunk: "Checking for common patterns...\n",
    }));

    await sleep(50, abortController.signal);

    send(notification("review.permission_requested", {
      lane_id,
      tool_name: "Write",
      tool_input: { file_path: "/tmp/test.txt", content: "malicious" },
      reason: "Tool 'Write' is not in the allowed list. Denied by policy.",
      action: "denied",
    }));

    await sleep(30, abortController.signal);

    send(notification("review.delta", {
      lane_id,
      chunk: "Review complete.\n",
    }));

    send(notification("review.completed", {
      lane_id,
      output: {
        findings: MOCK_FINDINGS,
        overall_assessment: "Mock review completed. Found 2 potential issues.",
        overall_confidence: 0.78,
      },
    }));
  } catch (error: unknown) {
    if (abortController.signal.aborted) {
      send(notification("review.error", {
        lane_id,
        error: "Review cancelled.",
      }));
      return;
    }
    throw error;
  } finally {
    if (activeAbortController === abortController) {
      activeAbortController = null;
    }
  }
}

function sleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      signal.removeEventListener("abort", onAbort);
      resolve();
    }, ms);

    const onAbort = () => {
      clearTimeout(timeout);
      reject(new Error("Aborted"));
    };

    if (signal.aborted) {
      onAbort();
      return;
    }

    signal.addEventListener("abort", onAbort, { once: true });
  });
}
