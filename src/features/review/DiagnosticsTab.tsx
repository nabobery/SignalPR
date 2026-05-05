import { useCallback, useEffect, useMemo, useState } from "react";
import { Activity, Filter, Loader2 } from "lucide-react";
import { getEventLog, parseError } from "../../lib/ipc";

interface EventEntry {
  timestamp: string;
  event_type: string;
  payload: Record<string, unknown>;
}

export function DiagnosticsTab({ runId }: { runId: string }) {
  const [events, setEvents] = useState<EventEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filterText, setFilterText] = useState("");

  const load = useCallback(async () => {
    try {
      const data = (await getEventLog(runId)) as EventEntry[];
      setEvents(data);
    } catch (err) {
      setError(parseError(err).message);
    } finally {
      setLoading(false);
    }
  }, [runId]);

  useEffect(() => {
    load();
  }, [load]);

  const filtered = useMemo(() => {
    if (!filterText.trim()) return events;
    const lower = filterText.toLowerCase();
    return events.filter(
      (e) =>
        e.event_type.toLowerCase().includes(lower) ||
        JSON.stringify(e.payload).toLowerCase().includes(lower),
    );
  }, [events, filterText]);

  if (loading) {
    return (
      <div className="flex items-center justify-center p-8">
        <Loader2 className="w-5 h-5 animate-spin text-zinc-400" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-4">
        <p className="text-sm text-red-400">{error}</p>
      </div>
    );
  }

  return (
    <div className="overflow-y-auto p-4 space-y-3">
      <div className="flex items-center gap-2">
        <Activity className="w-4 h-4 text-zinc-400" />
        <h3 className="text-sm font-medium text-zinc-200">Event Timeline</h3>
        <span className="text-xs text-zinc-500">({filtered.length} events)</span>
      </div>

      <div className="relative">
        <Filter className="absolute left-2.5 top-2 w-3.5 h-3.5 text-zinc-500" />
        <input
          type="text"
          value={filterText}
          onChange={(e) => setFilterText(e.target.value)}
          placeholder="Filter by event type or payload..."
          className="w-full pl-8 pr-3 py-1.5 text-xs bg-zinc-900 border border-zinc-800 rounded-md text-zinc-300 placeholder:text-zinc-600 focus:outline-none focus:border-zinc-700"
        />
      </div>

      {filtered.length === 0 && (
        <p className="text-xs text-zinc-500 py-4 text-center">
          {events.length === 0 ? "No events recorded for this run." : "No events match filter."}
        </p>
      )}

      <div className="space-y-1">
        {filtered.map((event, i) => (
          <EventRow key={i} event={event} />
        ))}
      </div>
    </div>
  );
}

function EventRow({ event }: { event: EventEntry }) {
  const [expanded, setExpanded] = useState(false);
  const time = new Date(event.timestamp).toLocaleTimeString();

  const payloadPreview = useMemo(() => {
    const keys = Object.keys(event.payload);
    if (keys.length === 0) return "";
    const parts: string[] = [];
    for (const key of keys.slice(0, 3)) {
      const val = event.payload[key];
      if (typeof val === "number" || typeof val === "string") {
        parts.push(`${key}=${val}`);
      }
    }
    return parts.join(", ");
  }, [event.payload]);

  return (
    <div className="border border-zinc-800/50 rounded-md bg-zinc-900/50">
      <button
        onClick={() => setExpanded(!expanded)}
        className="w-full text-left px-3 py-2 flex items-center gap-2 hover:bg-zinc-800/30 transition-colors"
      >
        <span className="text-xs text-zinc-500 shrink-0 font-mono w-20">{time}</span>
        <span className="text-xs font-medium text-zinc-200 shrink-0">{event.event_type}</span>
        {payloadPreview && <span className="text-xs text-zinc-500 truncate">{payloadPreview}</span>}
      </button>
      {expanded && Object.keys(event.payload).length > 0 && (
        <div className="px-3 pb-2 pt-0">
          <pre className="text-xs text-zinc-400 bg-zinc-950 rounded p-2 overflow-x-auto max-h-40">
            {JSON.stringify(event.payload, null, 2)}
          </pre>
        </div>
      )}
    </div>
  );
}
