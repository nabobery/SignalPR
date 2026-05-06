import { describe, expect, it } from "vitest";
import { isGitHubMetadata, isGitLabMetadata } from "./types";

describe("platform metadata type guards", () => {
  it("accepts valid GitHub metadata", () => {
    const candidate = {
      platform: "github",
      pr_body: null,
      head_sha: "abc",
      base_sha: "def",
      base_ref: "main",
      head_ref: "feature",
      draft: false,
      labels: [],
      requested_reviewers: ["alice"],
      requested_teams: [],
      review_state_summary: [],
      linked_issue_numbers: [],
      text_issue_refs: [],
    };
    expect(isGitHubMetadata(candidate)).toBe(true);
    expect(isGitLabMetadata(candidate)).toBe(false);
  });

  it("accepts valid GitLab metadata", () => {
    const candidate = {
      platform: "gitlab",
      mr_body: null,
      head_sha: "abc",
      base_sha: "def",
      base_ref: "main",
      head_ref: "feature",
      draft: false,
      labels: [],
      reviewers: ["alice"],
      approval_status: null,
      closes_issues: [101],
    };
    expect(isGitLabMetadata(candidate)).toBe(true);
    expect(isGitHubMetadata(candidate)).toBe(false);
  });

  it("rejects malformed metadata payloads", () => {
    expect(isGitHubMetadata(null)).toBe(false);
    expect(isGitLabMetadata(undefined)).toBe(false);
    expect(
      isGitHubMetadata({
        platform: "github",
        head_sha: "abc",
        base_sha: "def",
      }),
    ).toBe(false);
    expect(
      isGitLabMetadata({
        platform: "gitlab",
        head_sha: "abc",
        base_sha: "def",
      }),
    ).toBe(false);
  });
});
