import { useState, useEffect } from "react";
import type { AgentDefinition } from "../../lib/types";

interface AgentFormProps {
  initial?: AgentDefinition;
  onSave: (
    name: string,
    systemPrompt: string,
    agentType: string,
    provider?: string,
  ) => Promise<void>;
  onCancel: () => void;
}

export function AgentForm({ initial, onSave, onCancel }: AgentFormProps) {
  const [name, setName] = useState(initial?.name ?? "");
  const [agentType, setAgentType] = useState(initial?.agent_type ?? "");
  const [systemPrompt, setSystemPrompt] = useState(initial?.system_prompt ?? "");
  const [provider, setProvider] = useState<string>(initial?.provider ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Auto-suggest agent_type from name when creating new agent
  useEffect(() => {
    if (!initial && name && !agentType) {
      setAgentType(name.toLowerCase().replace(/\s+/g, "_"));
    }
  }, [name, initial, agentType]);

  const handleNameChange = (value: string) => {
    setName(value);
    if (!initial) {
      setAgentType(value.toLowerCase().replace(/\s+/g, "_"));
    }
  };

  const handleSubmit = async () => {
    if (!name.trim()) {
      setError("Name is required");
      return;
    }
    if (!agentType.trim()) {
      setError("Agent type is required");
      return;
    }
    if (!systemPrompt.trim()) {
      setError("System prompt is required");
      return;
    }
    setError(null);
    setSaving(true);
    try {
      await onSave(name.trim(), systemPrompt.trim(), agentType.trim(), provider || undefined);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="bg-(--color-surface) rounded-lg border border-(--color-border-subtle) p-4 space-y-4">
      <div className="space-y-2">
        <label className="block text-sm font-medium text-(--color-text-secondary)">Name</label>
        <input
          type="text"
          value={name}
          onChange={(e) => handleNameChange(e.target.value)}
          placeholder="e.g. Accessibility"
          disabled={!!initial}
          className="w-full bg-(--color-elevated) border border-(--color-border) rounded-lg px-3 py-2 text-(--color-text-primary) placeholder-zinc-500 disabled:opacity-50"
        />
      </div>

      <div className="space-y-2">
        <label className="block text-sm font-medium text-(--color-text-secondary)">
          Agent Type
        </label>
        <input
          type="text"
          value={agentType}
          onChange={(e) => setAgentType(e.target.value)}
          placeholder="e.g. accessibility"
          className="w-full bg-(--color-elevated) border border-(--color-border) rounded-lg px-3 py-2 text-(--color-text-primary) placeholder-zinc-500"
        />
        <p className="text-xs text-(--color-text-tertiary)">
          Used as the lane identifier in review results
        </p>
      </div>

      <div className="space-y-2">
        <label className="block text-sm font-medium text-(--color-text-secondary)">
          System Prompt
        </label>
        <textarea
          value={systemPrompt}
          onChange={(e) => setSystemPrompt(e.target.value)}
          placeholder="Describe what this agent should focus on when reviewing code..."
          rows={6}
          className="w-full bg-(--color-elevated) border border-(--color-border) rounded-lg px-3 py-2 text-(--color-text-primary) placeholder-zinc-500 min-h-[120px] resize-y"
        />
      </div>

      <div className="space-y-2">
        <label className="block text-sm font-medium text-(--color-text-secondary)">
          Provider (optional)
        </label>
        <select
          value={provider}
          onChange={(e) => setProvider(e.target.value)}
          className="w-full bg-(--color-elevated) border border-(--color-border) rounded-lg px-3 py-2 text-(--color-text-primary)"
        >
          <option value="">Auto (use default)</option>
          <option value="codex">Codex</option>
          <option value="claude">Claude</option>
          <option value="copilot">Copilot</option>
          <option value="opencode">OpenCode</option>
        </select>
      </div>

      {error && <p className="text-sm text-(--color-sev-blocker)">{error}</p>}

      <div className="flex gap-2 pt-2">
        <button
          onClick={handleSubmit}
          disabled={saving}
          className="px-4 py-2 bg-(--color-accent) hover:bg-(--color-accent-hover) text-white rounded-lg text-sm font-medium transition-colors disabled:opacity-50"
        >
          {saving ? "Saving..." : "Save Agent"}
        </button>
        <button
          onClick={onCancel}
          className="px-4 py-2 bg-(--color-elevated) hover:bg-(--color-elevated) text-(--color-text-secondary) rounded-lg text-sm font-medium transition-colors"
        >
          Cancel
        </button>
      </div>
    </div>
  );
}
