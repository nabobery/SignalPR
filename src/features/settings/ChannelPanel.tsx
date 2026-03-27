import { useState, useEffect } from "react";
import {
  configureChannel,
  removeChannel,
  getChannelStatus,
  hasChannelToken,
  startChannelListeners,
  stopChannelListeners,
  parseError,
} from "../../lib/ipc";
import type { ChannelStatus } from "../../lib/types";

interface ChannelSectionProps {
  source: "slack" | "discord";
  label: string;
  placeholder: string;
  description: string;
  configured: boolean;
  status: ChannelStatus | null;
  onConfigure: (source: string, token: string) => Promise<void>;
  onRemove: (source: string) => Promise<void>;
}

function ChannelSection({
  source,
  label,
  placeholder,
  description,
  configured,
  status,
  onConfigure,
  onRemove,
}: ChannelSectionProps) {
  const [token, setToken] = useState("");
  const [saving, setSaving] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleConfigure = async () => {
    if (!token.trim()) return;
    setSaving(true);
    setError(null);
    try {
      await onConfigure(source, token);
      setToken("");
    } catch (err) {
      setError(parseError(err).message);
    } finally {
      setSaving(false);
    }
  };

  const handleRemove = async () => {
    setRemoving(true);
    setError(null);
    try {
      await onRemove(source);
    } catch (err) {
      setError(parseError(err).message);
    } finally {
      setRemoving(false);
    }
  };

  const connected = status?.connected ?? false;

  return (
    <div className="bg-zinc-900 rounded-lg border border-zinc-800 p-4">
      <div className="flex items-center justify-between mb-3">
        <div className="flex items-center gap-2">
          <div
            className={
              connected ? "w-2 h-2 rounded-full bg-emerald-500" : "w-2 h-2 rounded-full bg-zinc-600"
            }
          />
          <h3 className="text-sm font-semibold text-zinc-100">{label}</h3>
          {configured && (
            <span className="text-xs text-zinc-500 ml-1">
              {connected ? "Connected" : "Configured"}
            </span>
          )}
        </div>
        {configured && (
          <button
            onClick={handleRemove}
            disabled={removing}
            className="px-3 py-1 text-red-400 border border-red-800 rounded hover:bg-red-900/30 text-xs disabled:opacity-40"
          >
            {removing ? "Removing..." : "Remove"}
          </button>
        )}
      </div>

      {status?.message && <p className="text-xs text-zinc-400 mb-3">{status.message}</p>}

      <p className="text-xs text-zinc-500 mb-3">{description}</p>

      <div className="flex gap-2">
        <input
          type="password"
          value={token}
          onChange={(e) => setToken(e.target.value)}
          placeholder={placeholder}
          className="flex-1 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-zinc-100 text-sm focus:outline-none focus:border-zinc-500 placeholder-zinc-600"
        />
        <button
          onClick={handleConfigure}
          disabled={!token.trim() || saving}
          className="px-4 py-2 bg-emerald-600 text-white rounded-lg hover:bg-emerald-500 text-sm disabled:opacity-40 disabled:cursor-not-allowed"
        >
          {saving ? "Saving..." : configured ? "Update" : "Configure"}
        </button>
      </div>

      {error && <p className="text-red-400 text-xs mt-2">{error}</p>}
    </div>
  );
}

export function ChannelPanel() {
  const [slackConfigured, setSlackConfigured] = useState(false);
  const [discordConfigured, setDiscordConfigured] = useState(false);
  const [statuses, setStatuses] = useState<ChannelStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [toggling, setToggling] = useState(false);

  const loadState = async () => {
    try {
      const [slackHas, discordHas, channelStatuses] = await Promise.all([
        hasChannelToken("slack"),
        hasChannelToken("discord"),
        getChannelStatus(),
      ]);
      setSlackConfigured(slackHas);
      setDiscordConfigured(discordHas);
      setStatuses(channelStatuses);
    } catch {
      // Statuses may fail if backend module is not yet ready
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadState();
  }, []);

  const anyConfigured = slackConfigured || discordConfigured;
  const anyConnected = statuses.some((s) => s.connected);

  const handleConfigure = async (source: string, token: string) => {
    await configureChannel(source, token);
    // Auto-start listeners after configuring a token
    await startChannelListeners();
    await loadState();
  };

  const handleRemove = async (source: string) => {
    await removeChannel(source);
    await loadState();
  };

  const handleToggle = async () => {
    setToggling(true);
    try {
      if (anyConnected) {
        await stopChannelListeners();
      } else {
        await startChannelListeners();
      }
      // Give listeners a moment to connect before refreshing status
      await new Promise((r) => setTimeout(r, 500));
      await loadState();
    } catch {
      // Ignore toggle errors
    } finally {
      setToggling(false);
    }
  };

  const slackStatus = statuses.find((s) => s.source === "slack") ?? null;
  const discordStatus = statuses.find((s) => s.source === "discord") ?? null;

  if (loading) {
    return (
      <div className="text-zinc-500 py-12 text-center">
        <p className="text-sm">Loading channel configuration...</p>
      </div>
    );
  }

  return (
    <div className="space-y-4 max-w-2xl">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-lg font-semibold text-zinc-100 mb-1">Notification Channels</h2>
          <p className="text-sm text-zinc-400">
            Connect Slack or Discord to receive PR review requests via DMs and bot mentions.
          </p>
        </div>
        {anyConfigured && (
          <button
            onClick={handleToggle}
            disabled={toggling}
            className={`px-4 py-2 rounded-lg text-sm font-medium transition-colors disabled:opacity-40 ${
              anyConnected
                ? "bg-zinc-700 text-zinc-200 hover:bg-zinc-600"
                : "bg-emerald-600 text-white hover:bg-emerald-500"
            }`}
          >
            {toggling ? "..." : anyConnected ? "Disconnect" : "Connect"}
          </button>
        )}
      </div>

      <ChannelSection
        source="slack"
        label="Slack"
        placeholder="xapp-..."
        description="Uses Socket Mode for real-time DM and @mention listening. Requires an app-level token (xapp-...) with connections:write scope."
        configured={slackConfigured}
        status={slackStatus}
        onConfigure={handleConfigure}
        onRemove={handleRemove}
      />

      <ChannelSection
        source="discord"
        label="Discord"
        placeholder="Bot token..."
        description="Connects to Discord Gateway for real-time DM and @mention listening. Requires a bot token with Message Content intent enabled."
        configured={discordConfigured}
        status={discordStatus}
        onConfigure={handleConfigure}
        onRemove={handleRemove}
      />
    </div>
  );
}
