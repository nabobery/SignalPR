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
            ? "bg-[--color-accent-subtle] text-[--color-accent]"
            : "text-[--color-text-tertiary] hover:text-[--color-text-secondary] hover:bg-[--color-elevated]"
        }`
      }
    >
      <Icon className="w-4 h-4" />
    </NavLink>
  );
}

export function AppShell() {
  return (
    <div className="h-screen bg-[--color-base] text-[--color-text-primary] flex">
      {/* Vertical sidebar */}
      <nav className="w-[52px] shrink-0 flex flex-col items-center py-3 gap-1 border-r border-[--color-border-subtle] bg-[--color-surface]">
        {/* Logo mark */}
        <div className="flex items-center justify-center w-9 h-9 mb-2" aria-label="SignalPR">
          <svg width="20" height="20" viewBox="0 0 20 20" fill="none">
            <path
              d="M10 2L17 6.5V13.5L10 18L3 13.5V6.5L10 2Z"
              fill="none"
              stroke="oklch(70% 0.155 168)"
              strokeWidth="1.5"
              strokeLinejoin="round"
            />
            <path
              d="M10 6L14 8.5V13L10 15.5L6 13V8.5L10 6Z"
              fill="oklch(70% 0.155 168)"
              fillOpacity="0.25"
              stroke="oklch(70% 0.155 168)"
              strokeWidth="1"
              strokeLinejoin="round"
            />
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
