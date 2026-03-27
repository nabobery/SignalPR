import { createContext, useContext } from "react";
import type { Finding } from "./types";

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
  selectedFile: string | null;
}

export interface ReviewContextType {
  state: ReviewState;
  setSelectedFile: (file: string | null) => void;
  refreshSnapshot: () => Promise<void>;
}

export const ReviewContext = createContext<ReviewContextType | null>(null);

export function useReviewContext() {
  const ctx = useContext(ReviewContext);
  if (!ctx) throw new Error("useReviewContext must be used within ReviewContext.Provider");
  return ctx;
}
