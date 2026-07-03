import { NavLink, Outlet } from "react-router";
import { Inbox, Settings } from "lucide-react";

function NavItem({
  to,
  icon: Icon,
  label,
  end,
}: {
  to: string;
  icon: typeof Inbox;
  label: string;
  end?: boolean;
}) {
  return (
    <NavLink
      to={to}
      end={end}
      title={label}
      aria-label={label}
      className={({ isActive }) =>
        `flex items-center justify-center w-9 h-9 rounded-lg transition-colors ${
          isActive
            ? "bg-(--color-accent-subtle) text-(--color-accent)"
            : "text-(--color-text-tertiary) hover:text-(--color-text-secondary) hover:bg-(--color-elevated)"
        }`
      }
    >
      <Icon className="w-4 h-4" />
    </NavLink>
  );
}

export function AppShell() {
  return (
    <div className="h-screen bg-(--color-base) text-(--color-text-primary) flex">
      {/* Vertical sidebar */}
      <nav className="w-[52px] shrink-0 flex flex-col items-center py-3 gap-1 border-r border-(--color-border-subtle) bg-(--color-surface)">
        {/* Logo mark */}
        <div className="flex items-center justify-center w-9 h-9 mb-2" aria-label="SignalPR">
          <svg width="26" height="26" viewBox="0 0 320 320" fill="none" aria-hidden="true">
            <g strokeLinecap="round" strokeLinejoin="round">
              <path d="M230 80H115A45 45 0 0 0 115 170H175" stroke="#EDEDED" strokeWidth="15" />
              <path d="M115 140H230" stroke="#EDEDED" strokeWidth="15" />
              <path
                d="M115 180H175C206 180 225 200 225 230C225 260 205 270 175 270H70"
                stroke="#22C57E"
                strokeWidth="15"
              />
              <path
                d="M70 220H175C196 220 205 210 205 200C205 189 195 180 175 180"
                stroke="#22C57E"
                strokeWidth="15"
              />
            </g>
            <circle cx="230" cy="80" r="11" fill="#EDEDED" />
            <circle cx="230" cy="140" r="11" fill="#22C57E" />
            <circle cx="70" cy="220" r="11" fill="#22C57E" />
            <circle cx="70" cy="270" r="11" fill="#22C57E" />
          </svg>
        </div>

        {/* Nav items */}
        <NavItem to="/" icon={Inbox} label="Inbox" end />
        <NavItem to="/settings" icon={Settings} label="Settings" />
      </nav>

      {/* Main content */}
      <div className="flex-1 min-w-0 overflow-hidden">
        <Outlet />
      </div>
    </div>
  );
}
