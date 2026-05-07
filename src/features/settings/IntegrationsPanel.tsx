import { useState, useEffect } from "react";
import { Loader2, CheckCircle2, AlertCircle, KeyRound, Trash2 } from "lucide-react";
import {
  getIntegrationStatuses,
  storeIntegrationSecret,
  deleteIntegrationSecret,
  updateIntegrationSetting,
  parseError,
} from "../../lib/ipc";
import type { IntegrationStatus } from "../../lib/ipc";

export function IntegrationsPanel() {
  const [statuses, setStatuses] = useState<IntegrationStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = async () => {
    try {
      const data = await getIntegrationStatuses();
      setStatuses(data);
    } catch (err) {
      setError(parseError(err).message);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    refresh();
  }, []);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12 text-zinc-400">
        <Loader2 className="w-5 h-5 animate-spin mr-2" />
        Loading integrations…
      </div>
    );
  }

  if (error) {
    return (
      <div className="text-red-400 text-sm p-4 bg-red-950/30 border border-red-800 rounded-lg">
        {error}
      </div>
    );
  }

  const jira = statuses.find((s) => s.id === "jira");
  const linear = statuses.find((s) => s.id === "linear");

  return (
    <div className="space-y-8 max-w-2xl">
      <p className="text-sm text-zinc-400">
        Connect external issue trackers to enrich review context with linked issue data.
      </p>

      {jira && (
        <IntegrationCard
          id="jira"
          label="Jira"
          description="Fetch issue context from Atlassian Jira Cloud."
          enabled={jira.enabled}
          hasSecret={jira.has_secret}
          settings={jira.settings}
          settingsFields={[
            {
              key: "integration_jira_base_url",
              label: "Base URL",
              placeholder: "https://your-team.atlassian.net",
            },
            { key: "integration_jira_email", label: "Email", placeholder: "user@company.com" },
          ]}
          secretLabel="API Token"
          onRefresh={refresh}
          enableKey="integration_jira_enabled"
        />
      )}

      {linear && (
        <IntegrationCard
          id="linear"
          label="Linear"
          description="Fetch issue context from Linear via GraphQL API."
          enabled={linear.enabled}
          hasSecret={linear.has_secret}
          settings={linear.settings}
          settingsFields={[
            {
              key: "integration_linear_workspace",
              label: "Workspace slug",
              placeholder: "my-workspace",
            },
          ]}
          secretLabel="API Key"
          onRefresh={refresh}
          enableKey="integration_linear_enabled"
        />
      )}
    </div>
  );
}

interface SettingsField {
  key: string;
  label: string;
  placeholder: string;
}

interface IntegrationCardProps {
  id: string;
  label: string;
  description: string;
  enabled: boolean;
  hasSecret: boolean;
  settings: Record<string, string>;
  settingsFields: SettingsField[];
  secretLabel: string;
  enableKey: string;
  onRefresh: () => void;
}

function IntegrationCard({
  id,
  label,
  description,
  enabled,
  hasSecret,
  settings,
  settingsFields,
  secretLabel,
  enableKey,
  onRefresh,
}: IntegrationCardProps) {
  const [secretInput, setSecretInput] = useState("");
  const [saving, setSaving] = useState(false);
  const [fieldValues, setFieldValues] = useState<Record<string, string>>({});

  useEffect(() => {
    const initial: Record<string, string> = {};
    for (const f of settingsFields) {
      const settingKey = f.key.replace(`integration_${id}_`, "");
      initial[f.key] = settings[settingKey] || "";
    }
    setFieldValues(initial);
  }, [settings, id, settingsFields]);

  const handleToggle = async () => {
    try {
      await updateIntegrationSetting(enableKey, enabled ? "false" : "true");
      onRefresh();
    } catch {
      // ignore
    }
  };

  const handleSaveSecret = async () => {
    if (!secretInput.trim()) return;
    setSaving(true);
    try {
      await storeIntegrationSecret(id, secretInput);
      setSecretInput("");
      onRefresh();
    } catch {
      // ignore
    } finally {
      setSaving(false);
    }
  };

  const handleDeleteSecret = async () => {
    setSaving(true);
    try {
      await deleteIntegrationSecret(id);
      onRefresh();
    } catch {
      // ignore
    } finally {
      setSaving(false);
    }
  };

  const handleFieldBlur = async (key: string) => {
    const value = fieldValues[key] || "";
    try {
      await updateIntegrationSetting(key, value);
    } catch {
      // ignore
    }
  };

  return (
    <div className="border border-zinc-800 rounded-lg p-5 bg-zinc-900/50">
      <div className="flex items-center justify-between mb-3">
        <div>
          <h3 className="text-base font-medium text-zinc-100">{label}</h3>
          <p className="text-sm text-zinc-500 mt-0.5">{description}</p>
        </div>
        <button
          type="button"
          onClick={handleToggle}
          role="switch"
          aria-checked={enabled}
          aria-label={`${label} integration`}
          className={`relative w-10 h-5 rounded-full transition-colors ${
            enabled ? "bg-emerald-600" : "bg-zinc-700"
          }`}
        >
          <span
            className={`absolute top-0.5 left-0.5 w-4 h-4 bg-white rounded-full transition-transform ${
              enabled ? "translate-x-5" : "translate-x-0"
            }`}
          />
        </button>
      </div>

      {enabled && (
        <div className="space-y-3 mt-4 pt-4 border-t border-zinc-800">
          {settingsFields.map((f) => (
            <div key={f.key}>
              <label className="block text-xs text-zinc-400 mb-1">{f.label}</label>
              <input
                type="text"
                value={fieldValues[f.key] || ""}
                onChange={(e) => setFieldValues({ ...fieldValues, [f.key]: e.target.value })}
                onBlur={() => handleFieldBlur(f.key)}
                placeholder={f.placeholder}
                className="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm text-zinc-200 placeholder:text-zinc-600 focus:outline-none focus:border-emerald-600"
              />
            </div>
          ))}

          <div>
            <label className="block text-xs text-zinc-400 mb-1">{secretLabel}</label>
            <div className="flex items-center gap-2">
              {hasSecret ? (
                <>
                  <span className="flex items-center gap-1 text-xs text-emerald-400">
                    <CheckCircle2 className="w-3.5 h-3.5" />
                    Stored in keychain
                  </span>
                  <button
                    onClick={handleDeleteSecret}
                    disabled={saving}
                    className="ml-auto text-xs text-red-400 hover:text-red-300 flex items-center gap-1"
                  >
                    <Trash2 className="w-3 h-3" />
                    Remove
                  </button>
                </>
              ) : (
                <>
                  <div className="flex items-center gap-1 text-xs text-zinc-500">
                    <AlertCircle className="w-3.5 h-3.5" />
                    Not configured
                  </div>
                </>
              )}
            </div>
            {!hasSecret && (
              <div className="flex items-center gap-2 mt-2">
                <KeyRound className="w-4 h-4 text-zinc-500" />
                <input
                  type="password"
                  value={secretInput}
                  onChange={(e) => setSecretInput(e.target.value)}
                  placeholder={`Enter ${secretLabel.toLowerCase()}…`}
                  className="flex-1 bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm text-zinc-200 placeholder:text-zinc-600 focus:outline-none focus:border-emerald-600"
                />
                <button
                  onClick={handleSaveSecret}
                  disabled={saving || !secretInput.trim()}
                  className="px-3 py-1.5 text-xs font-medium bg-emerald-700 text-emerald-100 rounded hover:bg-emerald-600 disabled:opacity-40"
                >
                  {saving ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : "Save"}
                </button>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
