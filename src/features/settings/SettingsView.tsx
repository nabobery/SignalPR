import { useState } from "react";
import { GeneralPanel } from "./GeneralPanel";
import { PresetPanel } from "./PresetPanel";
import { AgentPanel } from "./AgentPanel";
import { ChannelPanel } from "./ChannelPanel";
import { IntegrationsPanel } from "./IntegrationsPanel";

type Tab = "general" | "presets" | "agents" | "channels" | "integrations";

const TABS: { id: Tab; label: string }[] = [
  { id: "general", label: "General" },
  { id: "presets", label: "Presets" },
  { id: "agents", label: "Agents" },
  { id: "channels", label: "Channels" },
  { id: "integrations", label: "Integrations" },
];

export function SettingsView() {
  const [activeTab, setActiveTab] = useState<Tab>("general");

  return (
    <div className="h-full flex bg-(--color-base) text-(--color-text-primary)">
      {/* Vertical tab list */}
      <aside className="w-44 shrink-0 border-r border-(--color-border-subtle) py-5 px-3">
        <h1 className="px-2 mb-4 text-xs font-semibold uppercase tracking-wider text-(--color-text-tertiary)">
          Settings
        </h1>
        <nav className="flex flex-col gap-0.5">
          {TABS.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`w-full text-left px-2.5 py-1.5 rounded-md text-sm transition-colors ${
                activeTab === tab.id
                  ? "bg-(--color-elevated) text-(--color-text-primary) font-medium"
                  : "text-(--color-text-secondary) hover:text-(--color-text-primary) hover:bg-(--color-elevated)/60"
              }`}
            >
              {tab.label}
            </button>
          ))}
        </nav>
      </aside>

      {/* Content panel */}
      <div className="flex-1 min-w-0 overflow-y-auto p-6">
        {activeTab === "general" && <GeneralPanel />}
        {activeTab === "presets" && <PresetPanel />}
        {activeTab === "agents" && <AgentPanel />}
        {activeTab === "channels" && <ChannelPanel />}
        {activeTab === "integrations" && <IntegrationsPanel />}
      </div>
    </div>
  );
}
