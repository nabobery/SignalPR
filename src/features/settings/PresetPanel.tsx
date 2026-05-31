import { useState, useEffect } from "react";
import { Loader2, Info } from "lucide-react";
import { getPreferences, parseError } from "../../lib/ipc";
import type { PreferenceSummary } from "../../lib/types";

const EXAMPLE_YAML = `# .signalpr.yml
extends: default

max_surface_findings: 25
similarity_threshold: 0.8
preferred_provider: auto
drop_nitpicks: true
min_confidence: 0.6
lane_timeout_secs: 90

agents:
  - type: security
    enabled: true
  - type: logic
    enabled: true
  - type: style
    enabled: false`;

export function PresetPanel() {
  const [preferences, setPreferences] = useState<PreferenceSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const prefs = await getPreferences();
        if (!cancelled) setPreferences(prefs);
      } catch (err) {
        if (!cancelled) setError(parseError(err).message);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div className="space-y-6 max-w-2xl">
      <div className="bg-[--color-surface] rounded-lg border border-[--color-border-subtle] p-4">
        <h3 className="text-sm font-medium text-[--color-text-primary] mb-2">
          Repository Configuration
        </h3>
        <p className="text-[--color-text-secondary] text-sm mb-4">
          SignalPR loads its configuration from a{" "}
          <code className="text-[--color-accent] bg-[--color-elevated] px-1 rounded">
            .signalpr.yml
          </code>{" "}
          file in the root of your repository. This lets each project define its own review
          settings, agent selection, and thresholds.
        </p>
        <pre className="bg-[--color-base] border border-[--color-border-subtle] rounded-lg p-4 text-xs text-[--color-text-secondary] overflow-x-auto">
          {EXAMPLE_YAML}
        </pre>
      </div>

      <div className="bg-[--color-surface] rounded-lg border border-[--color-border-subtle] p-4">
        <h3 className="text-sm font-medium text-[--color-text-primary] mb-2">
          Learned Preferences
        </h3>

        <div className="flex items-start gap-2 mb-4 text-[--color-text-secondary] text-sm">
          <Info className="w-4 h-4 mt-0.5 shrink-0 text-[--color-accent]" />
          <span>
            Preferences are learned automatically from your review decisions. Over time, SignalPR
            adapts to surface findings you care about and suppress ones you typically reject.
          </span>
        </div>

        {loading ? (
          <div className="flex items-center py-4">
            <Loader2 className="w-4 h-4 animate-spin text-[--color-text-secondary]" />
            <span className="ml-2 text-[--color-text-secondary] text-sm">
              Loading preferences...
            </span>
          </div>
        ) : error ? (
          <p className="text-[--color-sev-blocker] text-sm">{error}</p>
        ) : preferences.length === 0 ? (
          <p className="text-[--color-text-tertiary] text-sm">
            No learned preferences yet. Complete some reviews and preferences will appear here.
          </p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-[--color-border] text-[--color-text-secondary]">
                  <th className="text-left py-2 pr-4 font-medium">Agent Type</th>
                  <th className="text-left py-2 pr-4 font-medium">Category</th>
                  <th className="text-right py-2 pr-4 font-medium">Accept Rate</th>
                  <th className="text-right py-2 font-medium">Total Decisions</th>
                </tr>
              </thead>
              <tbody>
                {preferences.map((pref) => (
                  <tr key={pref.id} className="border-b border-[--color-border-subtle]">
                    <td className="py-2 pr-4 text-[--color-text-primary]">{pref.agent_type}</td>
                    <td className="py-2 pr-4 text-[--color-text-secondary]">
                      {pref.category_tag ?? "-"}
                    </td>
                    <td className="py-2 pr-4 text-right text-[--color-text-primary]">
                      {(pref.accept_rate * 100).toFixed(1)}%
                    </td>
                    <td className="py-2 text-right text-[--color-text-primary]">
                      {pref.total_decisions}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}
