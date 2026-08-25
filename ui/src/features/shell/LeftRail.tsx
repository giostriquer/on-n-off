import {
  Activity,
  Gauge,
  GitPullRequest,
  LayoutDashboard,
  Moon,
  Package,
  Settings,
  Settings2,
  Sparkles,
  Sun,
  type LucideIcon,
} from "lucide-react";
import { Rocker } from "@/features/agents/Rocker";
import type { CatalogCounts, Screen } from "$lib/catalog";
import type { Theme } from "@/features/session/SessionProvider";

type LeftRailProps = {
  screen: Screen;
  counts: CatalogCounts;
  theme: Theme;
  masterOn: boolean;
  masterNote: string;
  busy?: boolean;
  masterDisabled?: boolean;
  showMasterCut?: boolean;
  onUsageIntent?: () => void;
  onScreen: (next: Screen) => void;
  onThemeChange: (theme: Theme) => void;
  onMaster: (enabled: boolean) => void;
};

type RailButtonProps = {
  id: Screen;
  label: string;
  Icon: LucideIcon | typeof McpGlyph;
  screen: Screen;
  count?: string;
  className?: string;
  onScreen: (next: Screen) => void;
  onIntent?: () => void;
};

/** One nav entry; `onIntent` fires on hover/focus so a heavy screen can preload its chunk. */
function RailButton({ id, label, Icon, screen, count, className, onScreen, onIntent }: RailButtonProps) {
  const active = screen === id;
  return (
    <button
      type="button"
      className={`flex h-[34px] items-center gap-2.5 rounded-none border-0 px-2 text-left text-[11.5px] font-semibold tracking-[0.04em] uppercase transition-colors hover:bg-[var(--well)] hover:text-[var(--silkscreen)] ${
        active ? "bg-[var(--well)] text-[var(--silkscreen)]" : "bg-transparent text-[var(--mute)]"
      } ${className ?? ""}`}
      aria-current={active ? "page" : undefined}
      onMouseEnter={onIntent}
      onFocus={onIntent}
      onClick={() => onScreen(id)}
    >
      <Icon className={`size-[15px] shrink-0 ${active ? "text-[var(--silkscreen)]" : "text-[var(--mute)]"}`} />
      <span className="flex-1 text-left">{label}</span>
      {count ? <span className="font-mono text-[11px] opacity-75">{count}</span> : null}
    </button>
  );
}

function McpGlyph({ className }: { className?: string }) {
  return (
    <span
      className={`inline-flex size-[15px] shrink-0 items-center justify-center text-[8px] font-bold tracking-tight ${className ?? ""}`}
      aria-hidden="true"
    >
      MCP
    </span>
  );
}

export function LeftRail({
  screen,
  counts,
  theme,
  masterOn,
  masterNote,
  busy = false,
  masterDisabled = false,
  showMasterCut = false,
  onUsageIntent,
  onScreen,
  onThemeChange,
  onMaster,
}: LeftRailProps) {
  const items = [
    { id: "overview" as const, label: "Overview", count: "", Icon: LayoutDashboard },
    {
      id: "plugins" as const,
      label: "Plugins",
      count: `${counts.plugins.on}/${counts.plugins.total}`,
      Icon: Package,
    },
    {
      id: "skills" as const,
      label: "Skills",
      count: `${counts.skills.on}/${counts.skills.total}`,
      Icon: Sparkles,
    },
    {
      id: "mcp" as const,
      label: "MCP servers",
      count: `${counts.mcp.on}/${counts.mcp.total}`,
      Icon: McpGlyph,
    },
    { id: "config" as const, label: "Agent config", count: "", Icon: Settings2 },
  ];

  return (
    <nav
      className="app-rail gap-0.5 border-r border-[var(--hair)] bg-[var(--plate)] px-2.5 py-3"
      aria-label="Section"
    >
      {items.map((item) => (
        <RailButton
          key={item.id}
          id={item.id}
          label={item.label}
          Icon={item.Icon}
          screen={screen}
          count={item.count}
          onScreen={onScreen}
        />
      ))}
      <div className="min-h-3.5 flex-1" />
      {showMasterCut ? (
        <div className="mb-2 flex flex-col gap-1.5 border border-dashed border-[var(--hair)] p-2.5">
          <span className="text-[9.5px] font-semibold tracking-[0.05em] text-[var(--mute)] uppercase">
            Master cut
          </span>
          <Rocker
            size="master"
            on={masterOn}
            dangerOff={!masterOn}
            busy={busy}
            disabled={masterDisabled}
            ariaLabel="Master cut"
            onToggle={() => onMaster(!masterOn)}
          />
          <span className="font-mono text-[10.5px] leading-snug text-[var(--mute)]">{masterNote}</span>
        </div>
      ) : null}
      <RailButton
        id="usage"
        label="Usage"
        Icon={Activity}
        screen={screen}
        className="w-full"
        onScreen={onScreen}
        onIntent={onUsageIntent}
      />
      <RailButton id="limits" label="Limits" Icon={Gauge} screen={screen} className="w-full" onScreen={onScreen} />
      <RailButton
        id="github"
        label="Pull requests"
        Icon={GitPullRequest}
        screen={screen}
        className="w-full"
        onScreen={onScreen}
      />
      <RailButton id="settings" label="Settings" Icon={Settings} screen={screen} className="w-full" onScreen={onScreen} />
      <div className="mt-2 flex flex-col gap-1.5 border-t border-[var(--hair)] px-2 pt-2.5 pb-0.5">
        <span className="text-[9.5px] font-semibold tracking-[0.05em] text-[var(--mute)] uppercase">
          Appearance
        </span>
        <div
          className="grid grid-cols-2 overflow-hidden rounded-lg border border-[var(--hair)]"
          role="group"
          aria-label="Theme"
        >
          <button
            type="button"
            className={`inline-flex h-7 items-center justify-center gap-1 border-0 text-[10px] font-semibold tracking-[0.04em] uppercase transition-shadow hover:shadow-[inset_0_0_0_1px_var(--mute)] ${
              theme === "dark"
                ? "bg-[var(--well)] text-[var(--silkscreen)]"
                : "bg-transparent text-[var(--mute)]"
            }`}
            aria-pressed={theme === "dark"}
            onClick={() => onThemeChange("dark")}
          >
            <Moon className="size-[13px] shrink-0" aria-hidden="true" />
            Dark
          </button>
          <button
            type="button"
            className={`inline-flex h-7 items-center justify-center gap-1 border-0 border-l border-[var(--hair)] text-[10px] font-semibold tracking-[0.04em] uppercase transition-shadow hover:shadow-[inset_0_0_0_1px_var(--mute)] ${
              theme === "light"
                ? "bg-[var(--well)] text-[var(--silkscreen)]"
                : "bg-transparent text-[var(--mute)]"
            }`}
            aria-pressed={theme === "light"}
            onClick={() => onThemeChange("light")}
          >
            <Sun className="size-[13px] shrink-0" aria-hidden="true" />
            Light
          </button>
        </div>
      </div>
    </nav>
  );
}
