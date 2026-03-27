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
    <div className="bg-zinc-900 rounded-lg border border-zinc-800 p-4 space-y-4">
      <div className="space-y-2">
        <label className="block text-sm font-medium text-zinc-300">Name</label>
        <input
          type="text"
          value={name}
          onChange={(e) => handleNameChange(e.target.value)}
          placeholder="e.g. Accessibility"
          disabled={!!initial}
          className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-zinc-100 placeholder-zinc-500 disabled:opacity-50"
        />
      </div>

      <div className="space-y-2">
        <label className="block text-sm font-medium text-zinc-300">Agent Type</label>
        <input
          type="text"
          value={agentType}
          onChange={(e) => setAgentType(e.target.value)}
          placeholder="e.g. accessibility"
          className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-zinc-100 placeholder-zinc-500"
        />
        <p className="text-xs text-zinc-500">Used as the lane identifier in review results</p>
      </div>

      <div className="space-y-2">
        <label className="block text-sm font-medium text-zinc-300">System Prompt</label>
        <textarea
          value={systemPrompt}
          onChange={(e) => setSystemPrompt(e.target.value)}
          placeholder="Describe what this agent should focus on when reviewing code..."
          rows={6}
          className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-zinc-100 placeholder-zinc-500 min-h-[120px] resize-y"
        />
      </div>

      <div className="space-y-2">
        <label className="block text-sm font-medium text-zinc-300">Provider (optional)</label>
        <select
          value={provider}
          onChange={(e) => setProvider(e.target.value)}
          className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-zinc-100"
        >
          <option value="">Auto (use default)</option>
          <option value="codex">Codex</option>
          <option value="claude">Claude</option>
        </select>
      </div>

      {error && <p className="text-sm text-red-400">{error}</p>}

      <div className="flex gap-2 pt-2">
        <button
          onClick={handleSubmit}
          disabled={saving}
          className="px-4 py-2 bg-emerald-600 hover:bg-emerald-500 text-white rounded-lg text-sm font-medium transition-colors disabled:opacity-50"
        >
          {saving ? "Saving..." : "Save Agent"}
        </button>
        <button
          onClick={onCancel}
          className="px-4 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded-lg text-sm font-medium transition-colors"
        >
          Cancel
        </button>
      </div>
    </div>
  );
}
