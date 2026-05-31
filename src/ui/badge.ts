export type BadgeStyle = {
  text: string;
  bg: string;
  border: string;
};

export const queueLabels: Record<string, string> = {
  needs_your_review: "Needs your review",
  updated_since_review: "Updated since review",
  review_requested: "Review requested",
  attention_needed: "Attention needed",
  in_progress: "In progress",
  ready_to_submit: "Ready to submit",
  waiting_on_author: "Waiting on author",
  submitted_recently: "Submitted recently",
};

const queueStyles: Record<string, BadgeStyle> = {
  needs_your_review: {
    text: "text-[--color-state-action]",
    bg: "bg-[--color-state-action-bg]",
    border: "border-[--color-state-action]/25",
  },
  updated_since_review: {
    text: "text-[--color-state-action]",
    bg: "bg-[--color-state-action-bg]",
    border: "border-[--color-state-action]/25",
  },
  review_requested: {
    text: "text-[--color-state-action]",
    bg: "bg-[--color-state-action-bg]",
    border: "border-[--color-state-action]/25",
  },
  attention_needed: {
    text: "text-[--color-state-alert]",
    bg: "bg-[--color-state-alert-bg]",
    border: "border-[--color-state-alert]/25",
  },
  in_progress: {
    text: "text-[--color-state-progress]",
    bg: "bg-[--color-state-progress-bg]",
    border: "border-[--color-state-progress]/25",
  },
  ready_to_submit: {
    text: "text-[--color-state-ready]",
    bg: "bg-[--color-state-ready-bg]",
    border: "border-[--color-state-ready]/25",
  },
  waiting_on_author: {
    text: "text-[--color-state-waiting]",
    bg: "bg-[--color-state-waiting-bg]",
    border: "border-[--color-state-waiting]/25",
  },
  submitted_recently: {
    text: "text-[--color-state-done]",
    bg: "bg-[--color-state-done-bg]",
    border: "border-[--color-state-done]/25",
  },
};

export const statusLabels: Record<string, string> = {
  running_agents: "Analyzing",
  cleaning: "Cleaning",
  created: "Queued",
};

const statusStyles: Record<string, string> = {
  ready: "text-[--color-state-ready]",
  submitted: "text-[--color-state-action]",
  failed: "text-[--color-state-alert]",
  running_agents: "text-[--color-state-progress]",
  cleaning: "text-[--color-state-progress]",
  created: "text-[--color-text-tertiary]",
};

export const severityStyles: Record<string, BadgeStyle> = {
  blocker: {
    text: "text-[--color-sev-blocker]",
    bg: "bg-[--color-sev-blocker-bg]",
    border: "border-[--color-sev-blocker]/25",
  },
  critical: {
    text: "text-[--color-sev-critical]",
    bg: "bg-[--color-sev-critical-bg]",
    border: "border-[--color-sev-critical]/25",
  },
  warning: {
    text: "text-[--color-sev-warning]",
    bg: "bg-[--color-sev-warning-bg]",
    border: "border-[--color-sev-warning]/25",
  },
  info: {
    text: "text-[--color-sev-info]",
    bg: "bg-[--color-sev-info-bg]",
    border: "border-[--color-sev-info]/25",
  },
  nitpick: {
    text: "text-[--color-sev-nitpick]",
    bg: "bg-[--color-sev-nitpick-bg]",
    border: "border-[--color-sev-nitpick]/25",
  },
};

export function queueBadge(state: string): BadgeStyle {
  return queueStyles[state] ?? queueStyles.submitted_recently;
}

export function queueBadgeLabel(state: string): string {
  return queueLabels[state] ?? state;
}

export function statusTextClass(status: string): string {
  return statusStyles[status] ?? "text-[--color-text-tertiary]";
}

export function statusLabel(status: string): string {
  return statusLabels[status] ?? status;
}

export function severityBadge(severity: string): BadgeStyle {
  return severityStyles[severity] ?? severityStyles.info;
}
