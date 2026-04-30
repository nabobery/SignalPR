import { invoke } from "@tauri-apps/api/core";
import type {
  ToolStatus,
  PrIntakeResult,
  ReviewSnapshot,
  ReviewRun,
  PreferenceSummary,
  AgentDefinitionsResponse,
  ChannelStatus,
  AppError,
} from "./types";

export function parseError(err: unknown): AppError {
  if (typeof err === "object" && err !== null && "code" in err && "message" in err) {
    return err as AppError;
  }
  if (typeof err === "string") {
    try {
      const parsed = JSON.parse(err);
      if (parsed.code && parsed.message) return parsed;
    } catch {
      // not JSON
    }
    return { code: "unknown", message: err };
  }
  return { code: "unknown", message: String(err) };
}

export async function inspectEnvironment(): Promise<ToolStatus[]> {
  return invoke("inspect_environment");
}

export async function getEnvironmentSummary(): Promise<{
  can_review: boolean;
  can_submit: boolean;
  available_providers: string[];
  warnings: string[];
  tools: ToolStatus[];
}> {
  return invoke("get_environment_summary");
}

export async function openFromUrl(url: string): Promise<PrIntakeResult> {
  return invoke("open_from_url", { url });
}

export async function confirmWorkspace(prId: string, localPath: string): Promise<void> {
  return invoke("confirm_workspace", { prId, localPath });
}

export async function startReview(prId: string): Promise<string> {
  return invoke("start_review", { prId });
}

export async function cancelReview(runId: string): Promise<void> {
  return invoke("cancel_review", { runId });
}

export async function getReviewSnapshot(runId: string): Promise<ReviewSnapshot> {
  return invoke("get_review_snapshot", { runId });
}

export async function updateFinding(
  findingId: string,
  body?: string,
  severity?: string,
  status?: string,
): Promise<void> {
  return invoke("update_finding", { findingId, body, severity, status });
}

export async function submitReview(
  runId: string,
  action: string,
  forceResubmit?: boolean,
): Promise<void> {
  return invoke("submit_review", { runId, action, forceResubmit });
}

export async function getSubmissionHistory(runId: string): Promise<unknown[]> {
  return invoke("get_submission_history", { runId });
}

export async function getIncompleteReviews(): Promise<ReviewRun[]> {
  return invoke("get_incomplete_reviews");
}

export async function resumeReview(runId: string): Promise<string> {
  return invoke("resume_review", { runId });
}

export async function getSettings(): Promise<Record<string, string>> {
  return invoke("get_settings");
}

export async function updateSetting(key: string, value: string): Promise<void> {
  return invoke("update_setting", { key, value });
}

export async function exportDiagnosticBundle(runId: string): Promise<unknown> {
  return invoke("export_diagnostic_bundle", { runId });
}

export async function getEventLog(runId: string): Promise<unknown[]> {
  return invoke("get_event_log", { runId });
}

export async function recordDecision(
  findingId: string,
  decision: string,
  timeToDecisionMs?: number,
): Promise<void> {
  return invoke("record_decision", { findingId, decision, timeToDecisionMs });
}

export async function getPreferences(): Promise<PreferenceSummary[]> {
  return invoke("get_preferences");
}

export async function previewFix(findingId: string): Promise<string> {
  return invoke("preview_fix", { findingId });
}

export async function applyFix(findingId: string): Promise<void> {
  return invoke("apply_fix", { findingId });
}

export async function acceptFix(findingId: string): Promise<void> {
  return invoke("accept_fix", { findingId });
}

export async function rejectFix(findingId: string): Promise<void> {
  return invoke("reject_fix", { findingId });
}

export async function getAgentDefinitions(): Promise<AgentDefinitionsResponse> {
  return invoke("get_agent_definitions");
}

export async function saveAgentDefinition(
  name: string,
  systemPrompt: string,
  agentType: string,
  provider?: string,
): Promise<void> {
  return invoke("save_agent_definition", { name, systemPrompt, agentType, provider });
}

export async function deleteAgentDefinition(name: string): Promise<void> {
  return invoke("delete_agent_definition", { name });
}

export async function configureChannel(source: string, token: string): Promise<void> {
  return invoke("configure_channel", { source, token });
}

export async function removeChannel(source: string): Promise<void> {
  return invoke("remove_channel", { source });
}

export async function getChannelStatus(): Promise<ChannelStatus[]> {
  return invoke("get_channel_status");
}

export async function hasChannelToken(source: string): Promise<boolean> {
  return invoke("has_channel_token", { source });
}

export async function startChannelListeners(): Promise<void> {
  return invoke("start_channel_listeners");
}

export async function stopChannelListeners(): Promise<void> {
  return invoke("stop_channel_listeners");
}

export async function resolveCodexApproval(requestId: unknown, decision: string): Promise<void> {
  return invoke("resolve_codex_approval", { requestId, decision });
}

export async function resolveCopilotPermission(
  sessionId: string,
  eventId: string,
  decision: string,
): Promise<void> {
  return invoke("resolve_copilot_permission", { sessionId, eventId, decision });
}

export async function resolveOpenCodePermission(requestId: string, reply: string): Promise<void> {
  return invoke("resolve_opencode_permission", { requestId, reply });
}

export async function resolveClaudeCodePermission(
  requestId: string,
  approved: boolean,
): Promise<void> {
  return invoke("resolve_claude_code_permission", { requestId, approved });
}
