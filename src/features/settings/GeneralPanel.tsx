import { useState, useEffect, useMemo, useRef } from "react";
import { Loader2, KeyRound, Trash2, CheckCircle2, ExternalLink, RefreshCw } from "lucide-react";
import {
  getSettings,
  updateSetting,
  parseError,
  getProviderCredentialStatuses,
  getProviderControlPlane,
  getProviderSetupCatalog,
  storeProviderSecret,
  deleteProviderSecret,
  probeProviderSetup,
} from "../../lib/ipc";
import type {
  CredentialStatus,
  ProviderControlPlaneSnapshot,
  ProviderSetupCatalogEntry,
  ProviderSetupCatalogSnapshot,
} from "../../lib/types";

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
  const [providerControl, setProviderControl] = useState<ProviderControlPlaneSnapshot | null>(null);
  const [providerCatalog, setProviderCatalog] = useState<ProviderSetupCatalogSnapshot | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const loadProviderControl = async () => {
    const control = await getProviderControlPlane();
    setProviderControl(control);
    return control;
  };

  const loadProviderCatalog = async () => {
    const catalog = await getProviderSetupCatalog();
    setProviderCatalog(catalog);
    return catalog;
  };

  const refreshProviderSignals = async () => {
    await loadProviderControl();
    void loadProviderCatalog().catch((err) => {
      setError((current) => current ?? parseError(err).message);
    });
  };

  useEffect(() => {
    let cancelled = false;
    const loadCatalogInBackground = async () => {
      try {
        const catalog = await getProviderSetupCatalog();
        if (!cancelled) setProviderCatalog(catalog);
      } catch (err) {
        if (!cancelled) {
          setError((current) => current ?? parseError(err).message);
        }
      }
    };
    (async () => {
      try {
        const [settings, control] = await Promise.all([getSettings(), getProviderControlPlane()]);
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
        setProviderControl(control);
      } catch (err) {
        if (!cancelled) setError(parseError(err).message);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    void loadCatalogInBackground();
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
      await refreshProviderSignals();
      setSaved(true);
      if (timerRef.current) clearTimeout(timerRef.current);
      timerRef.current = setTimeout(() => setSaved(false), 2000);
    } catch (err) {
      setError(parseError(err).message);
    } finally {
      setSaving(false);
    }
  };

  const selectedProviderCatalogEntry = useMemo(
    () =>
      providerCatalog?.providers.find(
        (provider) => provider.provider_id === form.preferred_provider,
      ) ?? null,
    [providerCatalog, form.preferred_provider],
  );

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
        {providerControl && (
          <ProviderControlSection
            control={providerControl}
            selectedProvider={form.preferred_provider}
          />
        )}
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
        {selectedProviderCatalogEntry && (
          <SelectedProviderSetupDetails provider={selectedProviderCatalogEntry} />
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

      <ProviderCredentialsSection onCredentialsChanged={refreshProviderSignals} />
      {providerCatalog && <ProviderCatalogSection catalog={providerCatalog} />}
    </div>
  );
}

function ProviderControlSection({
  control,
  selectedProvider,
}: {
  control: ProviderControlPlaneSnapshot;
  selectedProvider: string;
}) {
  return (
    <div className="mb-3 rounded-lg border border-zinc-800 bg-zinc-900/40 p-3 space-y-3">
      {control.recommended_provider_id && (
        <div className="rounded-md border border-emerald-900/40 bg-emerald-950/10 px-3 py-2">
          <div className="text-xs text-emerald-300">Recommended default</div>
          <p className="mt-1 text-sm text-zinc-100">{control.recommendation_reason}</p>
        </div>
      )}

      <div className="overflow-x-auto">
        <table className="w-full text-xs">
          <thead>
            <tr className="border-b border-zinc-800 text-zinc-500">
              <th className="text-left py-1.5 pr-3 font-medium">Provider</th>
              <th className="text-left py-1.5 px-2 font-medium">Status</th>
              <th className="text-left py-1.5 px-2 font-medium">Auth</th>
              <th className="text-left py-1.5 px-2 font-medium">Trust</th>
              <th className="text-left py-1.5 px-2 font-medium">Latency</th>
              <th className="text-left py-1.5 px-2 font-medium">Cost</th>
              <th className="text-left py-1.5 px-2 font-medium">Fit</th>
            </tr>
          </thead>
          <tbody>
            {control.providers.map((provider) => (
              <tr key={provider.provider_id} className="border-b border-zinc-800/40 align-top">
                <td className="py-2 pr-3">
                  <div className="text-zinc-200">{provider.display_name}</div>
                  <div className="mt-1 flex gap-1.5 flex-wrap">
                    {provider.provider_id === selectedProvider && (
                      <span className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-300">
                        selected
                      </span>
                    )}
                    {provider.recommended_default && (
                      <span className="rounded bg-emerald-900/30 px-1.5 py-0.5 text-[10px] text-emerald-300">
                        recommended
                      </span>
                    )}
                    {!provider.capabilities.in_auto_fallback && (
                      <span className="rounded bg-amber-900/30 px-1.5 py-0.5 text-[10px] text-amber-300">
                        opt-in
                      </span>
                    )}
                  </div>
                </td>
                <td className="py-2 px-2">
                  <div className="text-zinc-300">{provider.status}</div>
                  <div className="mt-1 text-[11px] text-zinc-500">{provider.status_reason}</div>
                </td>
                <td className="py-2 px-2 text-zinc-400">{provider.credential_source ?? "none"}</td>
                <td className="py-2 px-2 text-zinc-400">
                  {percentMaybe(provider.recent_metrics.avg_accept_rate)}
                </td>
                <td className="py-2 px-2 text-zinc-400">
                  {secondsMaybe(provider.recent_metrics.avg_latency_ms)}
                </td>
                <td className="py-2 px-2 text-zinc-400">
                  {costMaybe(provider.recent_metrics.avg_cost_usd)}
                </td>
                <td className="py-2 pl-2">
                  <div className="flex gap-1.5 flex-wrap">
                    {provider.capabilities.fit_tags.map((tag) => (
                      <span
                        key={tag}
                        className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-300"
                      >
                        {tag.split("_").join(" ")}
                      </span>
                    ))}
                  </div>
                  {provider.warnings.slice(0, 1).map((warning) => (
                    <div key={warning} className="mt-1 text-[11px] text-amber-300">
                      {warning}
                    </div>
                  ))}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function percentMaybe(value: number | null) {
  return value == null ? "—" : `${Math.round(value * 100)}%`;
}

function secondsMaybe(value: number | null) {
  return value == null ? "—" : `${(value / 1000).toFixed(1)}s`;
}

function costMaybe(value: number | null) {
  return value == null ? "—" : `$${value.toFixed(2)}`;
}

function humanizeToken(value: string) {
  return value.split("_").join(" ");
}

function SelectedProviderSetupDetails({ provider }: { provider: ProviderSetupCatalogEntry }) {
  const registry = provider.registry;

  return (
    <div className="mt-2 rounded-lg border border-zinc-800 bg-zinc-900/40 p-3 text-xs space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        <span className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-300">
          {humanizeToken(provider.provider_family)}
        </span>
        <span className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-300">
          {humanizeToken(provider.setup_state)}
        </span>
        <span className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-300">
          {humanizeToken(provider.capabilities.permission_model)}
        </span>
        {provider.currently_runnable && (
          <span className="rounded bg-emerald-900/30 px-1.5 py-0.5 text-[10px] text-emerald-300">
            runnable
          </span>
        )}
        {provider.execution_supported && !provider.currently_runnable && (
          <span className="rounded bg-amber-900/30 px-1.5 py-0.5 text-[10px] text-amber-300">
            setup needed
          </span>
        )}
        {provider.execution_supported && !provider.release_gate_passed && (
          <span className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-300">
            validation pending
          </span>
        )}
      </div>
      <p className="text-zinc-300">{provider.readiness_reason}</p>
      {provider.warnings.length > 0 && (
        <div className="space-y-1">
          {provider.warnings.map((warning) => (
            <p key={warning} className="text-amber-300">
              {warning}
            </p>
          ))}
        </div>
      )}
      {registry?.install_command && (
        <p className="text-zinc-400">
          Install: <code className="text-zinc-200">{registry.install_command}</code>
        </p>
      )}
      {provider.capabilities.supported_session_modes.length > 0 && (
        <p className="text-zinc-400">
          Modes: {provider.capabilities.supported_session_modes.join(", ")}
        </p>
      )}
      {registry?.config_options.length ? (
        <div className="space-y-1">
          <div className="text-zinc-500">ACP config options</div>
          <div className="flex flex-wrap gap-1.5">
            {registry.config_options.map((option) => (
              <span
                key={option.id}
                className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-300"
              >
                {option.name}
              </span>
            ))}
          </div>
        </div>
      ) : null}
      {registry?.setup_notes.length ? (
        <div className="space-y-1 text-zinc-400">
          {registry.setup_notes.map((note) => (
            <p key={note}>{note}</p>
          ))}
        </div>
      ) : null}
      <div className="flex flex-wrap gap-3 text-zinc-400">
        {registry?.docs_url && (
          <a
            href={registry.docs_url}
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex items-center gap-1 hover:text-zinc-200"
          >
            Docs <ExternalLink className="w-3 h-3" />
          </a>
        )}
        {registry?.auth_docs_url && (
          <a
            href={registry.auth_docs_url}
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex items-center gap-1 hover:text-zinc-200"
          >
            Auth <ExternalLink className="w-3 h-3" />
          </a>
        )}
      </div>
    </div>
  );
}

function ProviderCatalogSection({ catalog }: { catalog: ProviderSetupCatalogSnapshot }) {
  const [busyProviderId, setBusyProviderId] = useState<string | null>(null);
  const [probeMessages, setProbeMessages] = useState<Record<string, string>>({});
  const acpProviders = useMemo(
    () =>
      catalog.providers.filter(
        (provider) =>
          provider.registry !== null || provider.capabilities.install_source === "acp_registry",
      ),
    [catalog.providers],
  );

  const handleProbe = async (providerId: string) => {
    setBusyProviderId(providerId);
    try {
      const result = await probeProviderSetup(providerId);
      setProbeMessages((prev) => ({ ...prev, [providerId]: result.reason }));
    } catch (err) {
      setProbeMessages((prev) => ({ ...prev, [providerId]: parseError(err).message }));
    } finally {
      setBusyProviderId(null);
    }
  };

  return (
    <div className="border-t border-zinc-800 pt-6 mt-6">
      <div className="flex items-center justify-between gap-3 mb-3">
        <div>
          <h3 className="text-zinc-200 text-sm font-medium">ACP Provider Catalog</h3>
          <p className="text-zinc-500 text-xs mt-1">
            Registry source: {catalog.registry_source}
            {catalog.registry_fetched_at ? ` • fetched ${catalog.registry_fetched_at}` : ""}
          </p>
        </div>
      </div>
      <div className="space-y-3">
        {acpProviders.map((provider) => {
          const verifyAction = provider.actions.find((action) => action.kind === "verify");
          const verifyDisabled = !verifyAction?.enabled || busyProviderId === provider.provider_id;
          return (
            <div
              key={provider.provider_id}
              className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-3 space-y-2"
            >
              <div className="flex items-start justify-between gap-3">
                <div>
                  <div className="text-sm text-zinc-100">{provider.display_name}</div>
                  <div className="mt-1 flex flex-wrap gap-1.5">
                    <span className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-300">
                      {humanizeToken(provider.setup_state)}
                    </span>
                    <span className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-300">
                      {provider.support_tier}
                    </span>
                    <span className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-300">
                      {humanizeToken(provider.capabilities.transport_family)}
                    </span>
                    {provider.currently_runnable && (
                      <span className="rounded bg-emerald-900/30 px-1.5 py-0.5 text-[10px] text-emerald-300">
                        runnable
                      </span>
                    )}
                    {provider.execution_supported && !provider.release_gate_passed && (
                      <span className="rounded bg-amber-900/30 px-1.5 py-0.5 text-[10px] text-amber-300">
                        validation pending
                      </span>
                    )}
                  </div>
                </div>
                <button
                  onClick={() => handleProbe(provider.provider_id)}
                  disabled={verifyDisabled}
                  className="inline-flex items-center gap-1 rounded-md border border-zinc-700 px-2 py-1 text-xs text-zinc-300 hover:border-zinc-500 disabled:opacity-50"
                >
                  {busyProviderId === provider.provider_id ? (
                    <Loader2 className="w-3 h-3 animate-spin" />
                  ) : (
                    <RefreshCw className="w-3 h-3" />
                  )}
                  Verify
                </button>
              </div>
              <p className="text-xs text-zinc-400">{provider.readiness_reason}</p>
              {provider.registry?.install_command && (
                <p className="text-xs text-zinc-500">
                  Install:{" "}
                  <code className="text-zinc-300">{provider.registry.install_command}</code>
                </p>
              )}
              {provider.registry?.config_options.length ? (
                <div className="flex flex-wrap gap-1.5">
                  {provider.registry.config_options.map((option) => (
                    <span
                      key={`${provider.provider_id}-${option.id}`}
                      className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-300"
                    >
                      {option.name}
                    </span>
                  ))}
                </div>
              ) : null}
              {provider.registry?.setup_notes.length ? (
                <div className="space-y-1 text-xs text-zinc-500">
                  {provider.registry.setup_notes.map((note) => (
                    <p key={note}>{note}</p>
                  ))}
                </div>
              ) : null}
              {provider.warnings.length > 0 && (
                <div className="space-y-1 text-xs text-amber-300">
                  {provider.warnings.map((warning) => (
                    <p key={warning}>{warning}</p>
                  ))}
                </div>
              )}
              <div className="flex flex-wrap gap-3 text-xs text-zinc-400">
                {provider.actions
                  .filter((action) => action.url)
                  .map((action) => (
                    <a
                      key={action.id}
                      href={action.url ?? "#"}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="inline-flex items-center gap-1 hover:text-zinc-200"
                    >
                      {action.label} <ExternalLink className="w-3 h-3" />
                    </a>
                  ))}
              </div>
              {probeMessages[provider.provider_id] && (
                <p className="text-xs text-emerald-300">{probeMessages[provider.provider_id]}</p>
              )}
            </div>
          );
        })}
      </div>
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

function ProviderCredentialsSection({
  onCredentialsChanged,
}: {
  onCredentialsChanged?: () => Promise<void> | void;
}) {
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
      await onCredentialsChanged?.();
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
      await onCredentialsChanged?.();
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
