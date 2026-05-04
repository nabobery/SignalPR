import { NavLink, Outlet } from "react-router";
import { Inbox, Settings } from "lucide-react";

export function AppShell() {
  return (
    <div className="h-screen bg-zinc-950 text-zinc-100 flex flex-col">
      <header className="flex items-center gap-4 px-4 py-2.5 border-b border-zinc-800 shrink-0">
        <span className="text-sm font-bold tracking-tight text-zinc-100">SignalPR</span>
        <nav className="flex items-center gap-1 ml-2">
          <NavLink
            to="/"
            end
            className={({ isActive }) =>
              `flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-medium transition-colors ${isActive ? "bg-zinc-800 text-zinc-100" : "text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800/50"}`
            }
          >
            <Inbox className="w-3.5 h-3.5" />
            Inbox
          </NavLink>
          <NavLink
            to="/settings"
            className={({ isActive }) =>
              `flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-medium transition-colors ${isActive ? "bg-zinc-800 text-zinc-100" : "text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800/50"}`
            }
          >
            <Settings className="w-3.5 h-3.5" />
            Settings
          </NavLink>
        </nav>
      </header>
      <Outlet />
    </div>
  );
}
