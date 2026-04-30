import { useState, useEffect, useRef } from "react";
import { Loader2, KeyRound, Trash2, CheckCircle2 } from "lucide-react";
import {
  getSettings,
  updateSetting,
  parseError,
  getProviderCredentialStatuses,
  storeProviderSecret,
  deleteProviderSecret,
} from "../../lib/ipc";
import type { CredentialStatus } from "../../lib/types";

interface SettingsForm {
  max_surface_findings: string;
  similarity_threshold: string;
  preferred_provider: string;
  drop_nitpicks: string;
  min_confidence: string;
  lane_timeout_secs: string;
}

const DEFAULTS: SettingsForm = {
  max_surface_findings: "20",
  similarity_threshold: "0.85",
  preferred_provider: "auto",
  drop_nitpicks: "false",
  min_confidence: "0.5",
  lane_timeout_secs: "120",
};

export function GeneralPanel() {
  const [form, setForm] = useState<SettingsForm>(DEFAULTS);
  const [initial, setInitial] = useState<SettingsForm>(DEFAULTS);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const settings = await getSettings();
        if (cancelled) return;
        const loaded: SettingsForm = {
          max_surface_findings: settings.max_surface_findings ?? DEFAULTS.max_surface_findings,
          similarity_threshold: settings.similarity_threshold ?? DEFAULTS.similarity_threshold,
          preferred_provider: settings.preferred_provider ?? DEFAULTS.preferred_provider,
          drop_nitpicks: settings.drop_nitpicks ?? DEFAULTS.drop_nitpicks,
          min_confidence: settings.min_confidence ?? DEFAULTS.min_confidence,
          lane_timeout_secs: settings.lane_timeout_secs ?? DEFAULTS.lane_timeout_secs,
        };
        setForm(loaded);
        setInitial(loaded);
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

  const handleChange = (key: keyof SettingsForm, value: string) => {
    setForm((prev) => ({ ...prev, [key]: value }));
  };

  const handleSave = async () => {
    setError(null);
    setSaving(true);
    try {
      const entries = Object.entries(form) as [keyof SettingsForm, string][];
      for (const [key, value] of entries) {
        if (value !== initial[key]) {
          await updateSetting(key, value);
        }
      }
      setInitial({ ...form });
      setSaved(true);
      if (timerRef.current) clearTimeout(timerRef.current);
      timerRef.current = setTimeout(() => setSaved(false), 2000);
    } catch (err) {
      setError(parseError(err).message);
    } finally {
      setSaving(false);
    }
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12">
        <Loader2 className="w-5 h-5 animate-spin text-zinc-400" />
        <span className="ml-2 text-zinc-400">Loading settings...</span>
      </div>
    );
  }

  return (
    <div className="space-y-6 max-w-xl">
      {error && <p className="text-red-400 text-sm">{error}</p>}

      <div>
        <label className="text-zinc-400 text-xs block mb-1">Max Surface Findings</label>
        <input
          type="number"
          min={1}
          value={form.max_surface_findings}
          onChange={(e) => handleChange("max_surface_findings", e.target.value)}
          className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-zinc-100 focus:outline-none focus:ring-2 focus:ring-emerald-500"
        />
        <p className="text-zinc-500 text-xs mt-1">
          Maximum number of findings shown to the reviewer.
        </p>
      </div>

      <div>
        <label className="text-zinc-400 text-xs block mb-1">Similarity Threshold (0-1)</label>
        <input
          type="number"
          min={0}
          max={1}
          step={0.05}
          value={form.similarity_threshold}
          onChange={(e) => handleChange("similarity_threshold", e.target.value)}
          className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-zinc-100 focus:outline-none focus:ring-2 focus:ring-emerald-500"
        />
        <p className="text-zinc-500 text-xs mt-1">
          Threshold for clustering similar findings together.
        </p>
      </div>

      <div>
        <label className="text-zinc-400 text-xs block mb-1">Preferred Provider</label>
        <select
          value={form.preferred_provider}
          onChange={(e) => handleChange("preferred_provider", e.target.value)}
          className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-zinc-100 focus:outline-none focus:ring-2 focus:ring-emerald-500"
        >
          <option value="auto">Auto</option>
          <option value="codex">Codex</option>
          <option value="claude">Claude</option>
          <option value="copilot">Copilot</option>
          <option value="opencode">OpenCode</option>
          <option value="gemini">Gemini</option>
          <option value="cursor">Cursor</option>
          <option value="pi">PI</option>
          <option value="claude_code">Claude Code (opt-in, read-only)</option>
        </select>
        <p className="text-zinc-500 text-xs mt-1">Which AI provider to use for analysis lanes.</p>
        {form.preferred_provider === "gemini" && (
          <div className="text-amber-400 text-xs mt-2 space-y-1">
            <p>
              Gemini authenticates via API key only. Set{" "}
              <code className="text-amber-300">GEMINI_API_KEY</code> (AI Studio) or a Vertex{" "}
              <code className="text-amber-300">GOOGLE_*</code> env var, or store the key in Provider
              Credentials below.
            </p>
            <p>
              Google account OAuth is disabled to avoid third-party access risks described in{" "}
              <a
                href="https://github.com/google-gemini/gemini-cli/blob/main/docs/resources/tos-privacy.md"
                target="_blank"
                rel="noopener noreferrer"
                className="underline hover:text-amber-300"
              >
                Gemini CLI's ToS notice
              </a>
              . See the{" "}
              <a
                href="https://github.com/google-gemini/gemini-cli/blob/main/docs/get-started/authentication.md"
                target="_blank"
                rel="noopener noreferrer"
                className="underline hover:text-amber-300"
              >
                authentication guide
              </a>{" "}
              for setup. Review lanes run in plan mode with deny-by-default tool permissions.
            </p>
          </div>
        )}
        {form.preferred_provider === "cursor" && (
          <div className="text-amber-400 text-xs mt-2 space-y-1">
            <p>
              Cursor authenticates via API key. Generate one from the Cursor Dashboard (Cloud Agents
              → User API Keys) and export <code className="text-amber-300">CURSOR_API_KEY</code>, or
              store it in Provider Credentials below.
            </p>
            <p>
              Install the Cursor CLI with{" "}
              <code className="text-amber-300">curl https://cursor.com/install -fsS | bash</code>.
              SignalPR embeds Cursor via <code className="text-amber-300">agent acp</code> (an
              advanced, hidden subcommand) and speaks the Agent Client Protocol over stdio. Review
              lanes run in ask mode with deny-by-default tool permissions and filesystem reads
              sandboxed to the PR worktree.
            </p>
            <p>
              Docs:{" "}
              <a
                href="https://cursor.com/docs/cli/acp"
                target="_blank"
                rel="noopener noreferrer"
                className="underline hover:text-amber-300"
              >
                Cursor ACP
              </a>
              {" · "}
              <a
                href="https://cursor.com/docs/cli/reference/authentication"
                target="_blank"
                rel="noopener noreferrer"
                className="underline hover:text-amber-300"
              >
                Authentication
              </a>
            </p>
          </div>
        )}
        {form.preferred_provider === "pi" && (
          <div className="text-amber-400 text-xs mt-2 space-y-1">
            <p>
              PI authenticates through its own configuration. Install the CLI with{" "}
              <code className="text-amber-300">npm i -g @mariozechner/pi-coding-agent</code> and
              configure your API keys in PI's settings before launching SignalPR.
            </p>
            <p>
              SignalPR embeds PI via <code className="text-amber-300">pi --mode rpc</code> with all
              tools disabled (<code className="text-amber-300">--no-tools --no-session</code>). The
              agent runs in read-only mode with no filesystem access.
            </p>
          </div>
        )}
      </div>

      <div className="flex items-center gap-3">
        <input
          type="checkbox"
          id="drop_nitpicks"
          checked={form.drop_nitpicks === "true"}
          onChange={(e) => handleChange("drop_nitpicks", e.target.checked ? "true" : "false")}
          className="w-4 h-4 rounded bg-zinc-800 border-zinc-700 text-emerald-500 focus:ring-emerald-500 focus:ring-offset-0 accent-emerald-500"
        />
        <label htmlFor="drop_nitpicks" className="text-zinc-300 text-sm">
          Drop nitpick-level findings automatically
        </label>
      </div>

      <div>
        <label className="text-zinc-400 text-xs block mb-1">Min Confidence (0-1)</label>
        <input
          type="number"
          min={0}
          max={1}
          step={0.05}
          value={form.min_confidence}
          onChange={(e) => handleChange("min_confidence", e.target.value)}
          className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-zinc-100 focus:outline-none focus:ring-2 focus:ring-emerald-500"
        />
        <p className="text-zinc-500 text-xs mt-1">Minimum confidence score to surface a finding.</p>
      </div>

      <div>
        <label className="text-zinc-400 text-xs block mb-1">Lane Timeout (seconds)</label>
        <input
          type="number"
          min={10}
          value={form.lane_timeout_secs}
          onChange={(e) => handleChange("lane_timeout_secs", e.target.value)}
          className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-zinc-100 focus:outline-none focus:ring-2 focus:ring-emerald-500"
        />
        <p className="text-zinc-500 text-xs mt-1">
          How long each analysis lane runs before timing out.
        </p>
      </div>

      <div className="flex items-center gap-3 pt-2">
        <button
          onClick={handleSave}
          disabled={saving}
          className="px-4 py-2 bg-emerald-600 text-white rounded-lg hover:bg-emerald-500 disabled:opacity-50 flex items-center gap-2"
        >
          {saving && <Loader2 className="w-3 h-3 animate-spin" />}
          Save Settings
        </button>
        {saved && <span className="text-emerald-400 text-sm">Saved!</span>}
      </div>

      <ProviderCredentialsSection />
    </div>
  );
}

const CREDENTIAL_LABELS: Record<string, { label: string; placeholder: string }> = {
  anthropic_api_key: { label: "Anthropic API Key", placeholder: "sk-ant-..." },
  gemini_api_key: { label: "Gemini API Key", placeholder: "AIza..." },
  google_api_key: { label: "Google API Key", placeholder: "AIza..." },
  cursor_api_key: { label: "Cursor API Key", placeholder: "cur_..." },
  opencode_server_password: { label: "OpenCode Server Password", placeholder: "password" },
};

function fieldToProviderAndField(field: string): { providerId: string; field: string } {
  switch (field) {
    case "anthropic_api_key":
      return { providerId: "claude", field: "api_key" };
    case "gemini_api_key":
      return { providerId: "gemini", field: "api_key" };
    case "google_api_key":
      return { providerId: "gemini", field: "google_api_key" };
    case "cursor_api_key":
      return { providerId: "cursor", field: "api_key" };
    case "opencode_server_password":
      return { providerId: "opencode", field: "server_password" };
    default:
      return { providerId: "unknown", field: "unknown" };
  }
}

function ProviderCredentialsSection() {
  const [statuses, setStatuses] = useState<CredentialStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [secretInputs, setSecretInputs] = useState<Record<string, string>>({});
  const [busyField, setBusyField] = useState<string | null>(null);
  const [credError, setCredError] = useState<string | null>(null);

  const loadStatuses = async () => {
    try {
      const s = await getProviderCredentialStatuses();
      setStatuses(s);
    } catch (err) {
      setCredError(parseError(err).message);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadStatuses();
  }, []);

  const handleStore = async (field: string) => {
    const value = secretInputs[field];
    if (!value?.trim()) return;
    const { providerId, field: fieldName } = fieldToProviderAndField(field);
    setBusyField(field);
    setCredError(null);
    try {
      await storeProviderSecret(providerId, fieldName, value.trim());
      setSecretInputs((prev) => ({ ...prev, [field]: "" }));
      await loadStatuses();
    } catch (err) {
      setCredError(parseError(err).message);
    } finally {
      setBusyField(null);
    }
  };

  const handleDelete = async (field: string) => {
    const { providerId, field: fieldName } = fieldToProviderAndField(field);
    setBusyField(field);
    setCredError(null);
    try {
      await deleteProviderSecret(providerId, fieldName);
      await loadStatuses();
    } catch (err) {
      setCredError(parseError(err).message);
    } finally {
      setBusyField(null);
    }
  };

  if (loading) return null;

  return (
    <div className="border-t border-zinc-800 pt-6 mt-6">
      <h3 className="text-zinc-200 text-sm font-medium mb-3 flex items-center gap-2">
        <KeyRound className="w-4 h-4" />
        Provider Credentials
      </h3>
      <p className="text-zinc-500 text-xs mb-4">
        Store API keys in your OS keychain. Environment variables take precedence over stored keys.
      </p>

      {credError && <p className="text-red-400 text-xs mb-3">{credError}</p>}

      <div className="space-y-3">
        {statuses.map((cred) => {
          const meta = CREDENTIAL_LABELS[cred.field] ?? {
            label: cred.field,
            placeholder: "",
          };
          const isBusy = busyField === cred.field;

          return (
            <div key={cred.field} className="flex items-center gap-2">
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 mb-1">
                  <span className="text-zinc-300 text-xs">{meta.label}</span>
                  {cred.source !== "none" && (
                    <span className="inline-flex items-center gap-1 text-emerald-400 text-xs">
                      <CheckCircle2 className="w-3 h-3" />
                      {cred.source}
                    </span>
                  )}
                </div>
                {cred.source === "none" && (
                  <input
                    type="password"
                    placeholder={meta.placeholder}
                    value={secretInputs[cred.field] ?? ""}
                    onChange={(e) =>
                      setSecretInputs((prev) => ({
                        ...prev,
                        [cred.field]: e.target.value,
                      }))
                    }
                    className="w-full bg-zinc-800 border border-zinc-700 rounded px-2 py-1 text-zinc-100 text-xs focus:outline-none focus:ring-1 focus:ring-emerald-500"
                  />
                )}
              </div>
              {cred.source === "none" ? (
                <button
                  onClick={() => handleStore(cred.field)}
                  disabled={isBusy || !secretInputs[cred.field]?.trim()}
                  className="px-2 py-1 text-xs bg-emerald-700 text-white rounded hover:bg-emerald-600 disabled:opacity-50"
                >
                  {isBusy ? <Loader2 className="w-3 h-3 animate-spin" /> : "Save"}
                </button>
              ) : cred.source === "keychain" ? (
                <button
                  onClick={() => handleDelete(cred.field)}
                  disabled={isBusy}
                  className="px-2 py-1 text-xs text-red-400 hover:text-red-300"
                  title="Remove from keychain"
                >
                  {isBusy ? (
                    <Loader2 className="w-3 h-3 animate-spin" />
                  ) : (
                    <Trash2 className="w-3 h-3" />
                  )}
                </button>
              ) : null}
            </div>
          );
        })}
      </div>
    </div>
  );
}
