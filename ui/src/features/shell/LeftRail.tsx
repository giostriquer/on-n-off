import { Rocker } from "@/features/agents/Rocker";
import type { CatalogCounts, Screen } from "$lib/catalog";

type LeftRailProps = {
  screen: Screen;
  counts: CatalogCounts;
  masterOn: boolean;
  masterNote: string;
  busy?: boolean;
  masterDisabled?: boolean;
  showMasterCut?: boolean;
  onScreen: (next: Screen) => void;
  onMaster: (enabled: boolean) => void;
};

export function LeftRail({
  screen,
  counts,
  masterOn,
  masterNote,
  busy = false,
  masterDisabled = false,
  showMasterCut = false,
  onScreen,
  onMaster,
}: LeftRailProps) {
  const items = [
    { id: "overview" as const, label: "Overview", count: "" },
    { id: "plugins" as const, label: "Plugins", count: `${counts.plugins.on}/${counts.plugins.total}` },
    { id: "skills" as const, label: "Skills", count: `${counts.skills.on}/${counts.skills.total}` },
    { id: "mcp" as const, label: "MCP servers", count: `${counts.mcp.on}/${counts.mcp.total}` },
    { id: "usage" as const, label: "Usage", count: "" },
    { id: "config" as const, label: "Agent config", count: "" },
  ];

  return (
    <nav
      className="flex h-full w-[186px] shrink-0 flex-col gap-0.5 border-r border-[var(--hair)] bg-[var(--plate)] px-2.5 py-3"
      aria-label="Section"
    >
      {items.map((item) => {
        const active = screen === item.id;
        return (
          <button
            key={item.id}
            type="button"
            className={`flex h-[34px] items-center gap-2.5 rounded-none border-0 px-2 text-left text-[11.5px] font-semibold tracking-[0.04em] uppercase ${
              active ? "bg-[var(--well)] text-[var(--silkscreen)]" : "bg-transparent text-[var(--mute)]"
            }`}
            aria-current={active ? "page" : undefined}
            onClick={() => onScreen(item.id)}
          >
            <span className={`h-[18px] w-[3px] shrink-0 ${active ? "bg-[var(--fill)]" : "bg-transparent"}`} />
            <span className="flex-1">{item.label}</span>
            {item.count ? <span className="font-mono text-[11px] opacity-75">{item.count}</span> : null}
          </button>
        );
      })}
      <div className="flex-1" />
      {showMasterCut ? (
        <div className="flex flex-col gap-1.5 border border-dashed border-[var(--hair)] p-2.5">
          <span className="text-[9.5px] font-semibold tracking-[0.05em] text-[var(--mute)] uppercase">Master cut</span>
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
    </nav>
  );
}
