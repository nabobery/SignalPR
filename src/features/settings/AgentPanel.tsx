import { useState, useEffect, useCallback } from "react";
import { Shield, Layers, Gauge, Sparkles, Trash2, Plus, Pencil } from "lucide-react";
import { getAgentDefinitions, saveAgentDefinition, deleteAgentDefinition } from "../../lib/ipc";
import type { AgentDefinition } from "../../lib/types";
import { AgentForm } from "./AgentForm";

const BUILTIN_ICONS: Record<string, typeof Shield> = {
  security: Shield,
  architecture: Layers,
  performance: Gauge,
};

export function AgentPanel() {
  const [agents, setAgents] = useState<AgentDefinition[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showForm, setShowForm] = useState(false);
  const [editingAgent, setEditingAgent] = useState<AgentDefinition | null>(null);

  const loadAgents = useCallback(async () => {
    try {
      setError(null);
      const response = await getAgentDefinitions();
      setAgents(response.agents);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadAgents();
  }, [loadAgents]);

  const handleSave = async (
    name: string,
    systemPrompt: string,
    agentType: string,
    provider?: string,
  ) => {
    await saveAgentDefinition(name, systemPrompt, agentType, provider);
    setShowForm(false);
    setEditingAgent(null);
    await loadAgents();
  };

  const handleDelete = async (name: string) => {
    try {
      await deleteAgentDefinition(name);
      await loadAgents();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const builtinAgents = agents.filter((a) => a.is_builtin);
  const customAgents = agents.filter((a) => !a.is_builtin);

  if (loading) {
    return (
      <div className="text-zinc-500 py-12 text-center">
        <p className="text-sm">Loading agents...</p>
      </div>
    );
  }

  return (
    <div className="space-y-6 max-w-3xl">
      {error && (
        <div className="bg-red-900/30 border border-red-800 rounded-lg px-4 py-3 text-sm text-red-300">
          {error}
        </div>
      )}

      <div>
        <h2 className="text-sm font-medium text-zinc-400 uppercase tracking-wide mb-3">
          Built-in Agents
        </h2>
        <div className="space-y-3">
          {builtinAgents.map((agent) => {
            const Icon = BUILTIN_ICONS[agent.agent_type] ?? Sparkles;
            return (
              <div
                key={agent.agent_type}
                className="bg-zinc-900 rounded-lg border border-zinc-800 p-4"
              >
                <div className="flex items-center gap-3 mb-2">
                  <Icon className="w-4 h-4 text-zinc-400" />
                  <span className="font-medium text-zinc-200">{agent.name}</span>
                  <span className="bg-zinc-700 text-zinc-400 text-xs px-2 py-1 rounded">
                    built-in
                  </span>
                  <span className="bg-zinc-700 text-zinc-400 text-xs px-2 py-1 rounded">
                    {agent.agent_type}
                  </span>
                </div>
                <p className="text-sm text-zinc-400 pl-7">{agent.system_prompt}</p>
              </div>
            );
          })}
        </div>
      </div>

      <div>
        <div className="flex items-center justify-between mb-3">
          <h2 className="text-sm font-medium text-zinc-400 uppercase tracking-wide">
            Custom Agents
          </h2>
          {!showForm && !editingAgent && (
            <button
              onClick={() => setShowForm(true)}
              className="flex items-center gap-1.5 px-3 py-1.5 bg-emerald-600 hover:bg-emerald-500 text-white rounded-lg text-sm font-medium transition-colors"
            >
              <Plus className="w-3.5 h-3.5" />
              Add Agent
            </button>
          )}
        </div>

        {showForm && (
          <div className="mb-4">
            <AgentForm onSave={handleSave} onCancel={() => setShowForm(false)} />
          </div>
        )}

        {customAgents.length === 0 && !showForm && (
          <p className="text-sm text-zinc-500 py-4 text-center">
            No custom agents yet. Add one to extend your review pipeline.
          </p>
        )}

        <div className="space-y-3">
          {customAgents.map((agent) => (
            <div key={agent.name}>
              {editingAgent?.name === agent.name ? (
                <AgentForm
                  initial={agent}
                  onSave={handleSave}
                  onCancel={() => setEditingAgent(null)}
                />
              ) : (
                <div className="bg-zinc-900 rounded-lg border border-zinc-800 p-4">
                  <div className="flex items-center justify-between mb-2">
                    <div className="flex items-center gap-3">
                      <Sparkles className="w-4 h-4 text-violet-400" />
                      <span className="font-medium text-zinc-200">{agent.name}</span>
                      <span className="bg-violet-900/40 text-violet-300 text-xs px-2 py-1 rounded">
                        {agent.agent_type}
                      </span>
                      {agent.provider && (
                        <span className="bg-zinc-700 text-zinc-400 text-xs px-2 py-1 rounded">
                          {agent.provider}
                        </span>
                      )}
                    </div>
                    <div className="flex items-center gap-2">
                      <button
                        onClick={() => setEditingAgent(agent)}
                        className="text-zinc-400 hover:text-zinc-200 transition-colors"
                        title="Edit agent"
                      >
                        <Pencil className="w-3.5 h-3.5" />
                      </button>
                      <button
                        onClick={() => handleDelete(agent.name)}
                        className="text-red-400 hover:text-red-300 transition-colors"
                        title="Delete agent"
                      >
                        <Trash2 className="w-3.5 h-3.5" />
                      </button>
                    </div>
                  </div>
                  <p className="text-sm text-zinc-400 pl-7 line-clamp-3">{agent.system_prompt}</p>
                </div>
              )}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
