import { createContext, useContext } from "react";
import type {
  Finding,
  FindingCluster,
  LaneSnapshot,
  RunScorecard,
  ReviewDeltaSnapshot,
} from "./types";

export interface ReviewState {
  runId: string;
  status: string;
  prTitle: string;
  prNumber: number;
  prUrl: string;
  diffText: string | null;
  changedFiles: string[];
  findings: Finding[];
  errorMessage: string | null;
  laneStatuses: LaneSnapshot[];
  clusters: FindingCluster[];
  selectedFile: string | null;
  focusedFindingId: string | null;
  sessionDecisions: Record<string, "accept" | "skip">;
  baselineRunId: string | null;
  metrics: RunScorecard | null;
  delta: ReviewDeltaSnapshot | null;
}

export interface ReviewContextType {
  state: ReviewState;
  setSelectedFile: (file: string | null) => void;
  setSessionDecision: (findingId: string, decision: "accept" | "skip" | null) => void;
  refreshSnapshot: () => Promise<void>;
  revealFinding: (findingId: string) => void;
}

export const ReviewContext = createContext<ReviewContextType | null>(null);

export function useReviewContext() {
  const ctx = useContext(ReviewContext);
  if (!ctx) throw new Error("useReviewContext must be used within ReviewContext.Provider");
  return ctx;
}
