import { invoke } from "@tauri-apps/api/core";
import type { ToolStatus, PrIntakeResult, ReviewSnapshot, ReviewRun } from "./types";

export async function inspectEnvironment(): Promise<ToolStatus[]> {
  return invoke("inspect_environment");
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

export async function submitReview(runId: string, action: string): Promise<void> {
  return invoke("submit_review", { runId, action });
}

export async function getIncompleteReviews(): Promise<ReviewRun[]> {
  return invoke("get_incomplete_reviews");
}

export async function resumeReview(runId: string): Promise<string> {
  return invoke("resume_review", { runId });
}
