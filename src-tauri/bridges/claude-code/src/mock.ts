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
let cancelRequestedBeforeStart = false;

const pendingPermissions = new Map<
  string,
  { resolve: (approved: boolean) => void; timer: ReturnType<typeof setTimeout> }
>();

export function cancelMockReview(): void {
  if (activeAbortController) {
    activeAbortController.abort();
    activeAbortController = null;
  } else {
    cancelRequestedBeforeStart = true;
  }
  for (const [, entry] of pendingPermissions) {
    clearTimeout(entry.timer);
    entry.resolve(false);
  }
  pendingPermissions.clear();
}

export function resolveMockPermission(requestId: string, approved: boolean): boolean {
  const entry = pendingPermissions.get(requestId);
  if (!entry) return false;
  clearTimeout(entry.timer);
  pendingPermissions.delete(requestId);
  setTimeout(() => entry.resolve(approved), 0);
  return true;
}

export async function runMockReview(params: ReviewParams, send: SendFn): Promise<void> {
  const { lane_id } = params;
  const abortController = new AbortController();
  activeAbortController = abortController;

  try {
    if (cancelRequestedBeforeStart) {
      cancelRequestedBeforeStart = false;
      send(
        notification("review.error", {
          lane_id,
          error: "Review cancelled.",
        }),
      );
      return;
    }

    send(
      notification("review.delta", {
        lane_id,
        chunk: "Analyzing code changes...\n",
      }),
    );

    await sleep(50, abortController.signal);

    send(
      notification("review.delta", {
        lane_id,
        chunk: "Checking for common patterns...\n",
      }),
    );

    await sleep(50, abortController.signal);

    send(
      notification("review.permission_requested", {
        lane_id,
        tool_name: "Write",
        tool_input: { file_path: "/tmp/test.txt", content: "malicious" },
        reason: "Tool 'Write' is not in the allowed list. Denied by policy.",
        action: "denied",
      }),
    );

    await sleep(30, abortController.signal);

    send(
      notification("review.delta", {
        lane_id,
        chunk: "Review complete.\n",
      }),
    );

    send(
      notification("review.completed", {
        lane_id,
        output: {
          findings: MOCK_FINDINGS,
          overall_assessment: "Mock review completed. Found 2 potential issues.",
          overall_confidence: 0.78,
        },
        session_id: `mock-session-${Date.now()}`,
        cost_usd: 0.0042,
        checkpoint_id: `mock-checkpoint-${Date.now()}`,
      }),
    );
  } catch (error: unknown) {
    if (abortController.signal.aborted) {
      send(
        notification("review.error", {
          lane_id,
          error: "Review cancelled.",
        }),
      );
      return;
    }
    throw error;
  } finally {
    if (activeAbortController === abortController) {
      activeAbortController = null;
    }
  }
}

/**
 * Interactive mock review: emits a permission_requested with action="pending"
 * and waits for resolution before completing.
 */
export async function runMockReviewInteractive(params: ReviewParams, send: SendFn): Promise<void> {
  const { lane_id } = params;
  const abortController = new AbortController();
  activeAbortController = abortController;
  const requestId = `perm-${Date.now()}`;

  try {
    if (cancelRequestedBeforeStart) {
      cancelRequestedBeforeStart = false;
      send(notification("review.error", { lane_id, error: "Review cancelled." }));
      return;
    }

    send(notification("review.delta", { lane_id, chunk: "Analyzing (interactive mode)...\n" }));
    await sleep(50, abortController.signal);

    const permissionPromise = new Promise<boolean>((resolve) => {
      const timer = setTimeout(() => {
        pendingPermissions.delete(requestId);
        resolve(false);
      }, 10000);
      pendingPermissions.set(requestId, { resolve, timer });
    });

    send(
      notification("review.permission_requested", {
        lane_id,
        request_id: requestId,
        tool_name: "Write",
        tool_input: { file_path: "/tmp/test.txt", content: "guarded" },
        reason: "Tool 'Write' requires guarded-write approval.",
        action: "pending",
      }),
    );

    const approved = await permissionPromise;

    if (!approved) {
      send(notification("review.delta", { lane_id, chunk: "Write tool denied.\n" }));
    } else {
      send(
        notification("review.delta", { lane_id, chunk: "Write tool approved. Proceeding...\n" }),
      );
    }

    await sleep(30, abortController.signal);

    send(
      notification("review.completed", {
        lane_id,
        output: {
          findings: MOCK_FINDINGS,
          overall_assessment: "Interactive mock review completed.",
          overall_confidence: 0.8,
        },
        session_id: `mock-interactive-session-${Date.now()}`,
        cost_usd: 0.0058,
        checkpoint_id: `mock-interactive-checkpoint-${Date.now()}`,
      }),
    );
  } catch (error: unknown) {
    if (abortController.signal.aborted) {
      send(notification("review.error", { lane_id, error: "Review cancelled." }));
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
