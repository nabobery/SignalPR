import { invoke } from "@tauri-apps/api/core";
import type { ToolStatus, PrIntakeResult, ReviewSnapshot, ReviewRun, AppError } from "./types";

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
