import { useState } from "react";
import { useNavigate } from "react-router";
import { ArrowLeft } from "lucide-react";
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
  const navigate = useNavigate();
  const [activeTab, setActiveTab] = useState<Tab>("general");

  return (
    <div className="flex flex-col h-screen bg-zinc-950 text-zinc-100">
      <header className="flex items-center justify-between px-6 py-4 border-b border-zinc-800">
        <div className="flex items-center gap-3">
          <button
            onClick={() => navigate("/")}
            className="text-zinc-400 hover:text-zinc-100 transition-colors"
          >
            <ArrowLeft className="w-5 h-5" />
          </button>
          <h1 className="text-lg font-semibold">Settings</h1>
        </div>
      </header>

      <div className="flex border-b border-zinc-800 px-6">
        {TABS.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={
              activeTab === tab.id
                ? "px-4 py-2 rounded-t-lg bg-zinc-900 text-emerald-400 border-b-2 border-emerald-500"
                : "px-4 py-2 rounded-t-lg bg-zinc-800 text-zinc-400 hover:text-zinc-200"
            }
          >
            {tab.label}
          </button>
        ))}
      </div>

      <div className="flex-1 overflow-y-auto p-6">
        {activeTab === "general" && <GeneralPanel />}
        {activeTab === "presets" && <PresetPanel />}
        {activeTab === "agents" && <AgentPanel />}
        {activeTab === "channels" && <ChannelPanel />}
        {activeTab === "integrations" && <IntegrationsPanel />}
      </div>
    </div>
  );
}
