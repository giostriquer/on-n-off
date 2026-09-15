import { Link } from "@tanstack/react-router";
import { Rocker } from "@/features/agents/Rocker";
import { OverviewUsageCard } from "@/features/usage/OverviewUsageCard";
import { copy } from "$lib/copy";
import { driftLine, formatPluginVersion, type CatalogCounts, type DriftRow, type LiveRow } from "$lib/catalog";
import { tripTagClass, type TripEntry } from "$lib/tripLog";

type OverviewProps = {
  counts: CatalogCounts;
  rows: LiveRow[];
  drift?: DriftRow[];
  log: TripEntry[];
  busy?: boolean;
  pluginToggle?: boolean;
  scopeLabel?: string;
  onToggle: (row: LiveRow, enabled: boolean) => void;
  onUpdate?: (pluginId: string) => void;
  cliOk?: boolean;
  usageReady?: boolean;
};

const GAUGE_TO: Record<"plugins" | "skills" | "mcp", "/plugins" | "/skills" | "/mcp"> = {
  plugins: "/plugins",
  skills: "/skills",
  mcp: "/mcp",
};

export function Overview({
  counts,
  rows,
  drift = [],
  log,
  busy = false,
  pluginToggle = false,
  scopeLabel = "all projects",
  onToggle,
  onUpdate,
  cliOk = false,
  usageReady = true,
}: OverviewProps) {
  const gauges = [
    { id: "plugins" as const, label: "Plugins", ...counts.plugins },
    { id: "skills" as const, label: "Skills", ...counts.skills },
    { id: "mcp" as const, label: "MCP servers", ...counts.mcp },
  ];

  return (
    <div className="flex flex-col gap-4 px-5 pt-[18px] pb-[26px]">
      <div className="grid grid-cols-3 gap-3">
        {gauges.map((gauge) => (
          <Link
            key={gauge.id}
            to={GAUGE_TO[gauge.id]}
            className="relative block overflow-hidden rounded-[11px] border border-[var(--hair)] bg-[var(--plate)] p-3.5 text-[var(--silkscreen)] no-underline transition-colors hover:border-[var(--mute)] hover:bg-[var(--well)]"
            aria-label={`${gauge.label}: ${gauge.on} on of ${gauge.total} installed`}
          >
            <div className="text-[10px] font-semibold tracking-[0.03em] text-[var(--mute)] uppercase">
              {gauge.label}
            </div>
            <div className="mt-2.5 flex items-end gap-2">
              <span className="text-[34px] leading-none font-semibold">{gauge.on}</span>
              <span className="font-mono pb-1 text-xs text-[var(--mute)]">on / {gauge.total} installed</span>
            </div>
            <div className="mt-3 flex gap-[3px]" aria-hidden="true">
              {Array.from({ length: Math.max(gauge.total, 1) }, (_, index) => (
                <span
                  key={index}
                  className={`h-1.5 min-w-[2px] flex-1 rounded-[1px] ${
                    index < gauge.on ? "bg-[var(--live)]" : "bg-[var(--mute)] opacity-[0.32]"
                  }`}
                />
              ))}
            </div>
          </Link>
        ))}
      </div>

      <OverviewUsageCard ready={usageReady} />

      {drift.length > 0 ? (
        <section className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]" aria-label={copy.driftTitle}>
          <header className="flex items-center gap-2.5 border-b border-[var(--hair)] px-3 py-[9px]">
            <span className="size-2 shrink-0 bg-[var(--warn)]" aria-hidden="true" />
            <span className="text-[12px] font-semibold tracking-[0.03em] uppercase">{copy.driftTitle}</span>
            <span className="font-mono text-[12px] text-[var(--mute)]">{driftLine(drift.length)}</span>
          </header>
          {drift.map((row) => (
            <div key={row.id} className="flex items-center gap-3 border-b border-[var(--hair)] px-3 py-2.5 last:border-b-0">
              <span className="w-[52px] shrink-0 text-[10px] font-semibold tracking-[0.04em] text-[var(--mute)] uppercase">
                {row.kind}
              </span>
              <span className="min-w-[150px] text-[13px] font-semibold leading-none">{row.name}</span>
              <span className="font-mono text-[12px] leading-none text-[var(--mute)]">
                {formatPluginVersion(row.version)}
              </span>
              <span className="text-[var(--mute)]" aria-hidden="true">
                →
              </span>
              <span className="font-mono text-[12px] font-medium leading-none text-[var(--warn)]">
                {formatPluginVersion(row.upstream)}
              </span>
              <div className="flex-1" />
              <button
                type="button"
                className="h-[26px] shrink-0 border border-[var(--warn)] bg-transparent px-3 text-[11px] font-semibold tracking-[0.03em] text-[var(--warn)] uppercase disabled:opacity-45"
                disabled={!cliOk || busy}
                aria-label={`Update ${row.name}`}
                onClick={() => onUpdate?.(row.id)}
              >
                {copy.update}
              </button>
            </div>
          ))}
        </section>
      ) : null}

      <div className="flex flex-col gap-3">
        {/* One column per ~320px of width. The list is as long as the catalog, so a single column
            ran to several screens while the space beside it sat empty. The card cannot scroll on
            its own — `layout-contract.test.ts` allows exactly one page scroller — so columns are
            how it is kept near a screen. `overflow-hidden` here and `-mb-px` on the column box are
            a pair: the first gives the section a block formatting context so the second shortens
            it by a pixel instead of collapsing, which swallows the tallest column's last row
            border into the card's own. Drop either and that border doubles. */}
        <section className="overflow-hidden rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]">
          <header className="flex items-baseline justify-between gap-3 border-b border-[var(--hair)] px-3 py-2">
            <span className="shrink-0 text-[12px] font-semibold tracking-[0.03em] uppercase">Live on this scope</span>
            <span className="min-w-0 truncate font-mono text-[12px] text-[var(--mute)]" title={scopeLabel}>
              {scopeLabel}
            </span>
          </header>
          {rows.length === 0 ? (
            <p className="px-3 py-3 text-[13px] text-[var(--mute)]">Nothing live on this circuit.</p>
          ) : (
            <div className="-mb-px columns-[320px] gap-6 [column-rule:1px_solid_var(--hair)]">
              {rows.map((row) => (
              <div key={row.id} className="flex break-inside-avoid items-center gap-2.5 border-b border-[var(--hair)] px-3 py-[7px]">
                <span
                  className="size-2 shrink-0 translate-y-px rounded-full bg-[var(--live)] shadow-[0_0_7px_var(--live)]"
                  aria-hidden="true"
                />
                <span className="min-w-0 max-w-[60%] flex-[0_1_auto] truncate text-[13px] font-semibold" title={row.name}>
                  {row.name}
                </span>
                <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-[var(--mute)]" title={row.meta}>
                  {row.meta}
                </span>
                {row.togglable ? (
                  <Rocker
                    size="skill"
                    on={row.enabled}
                    busy={busy}
                    disabled={row.kind === "plugin" && (!pluginToggle || !row.togglable)}
                    ariaLabel={`${row.name} ${row.enabled ? "on" : "off"}`}
                    onToggle={() => onToggle(row, !row.enabled)}
                  />
                ) : (
                  // Matches `.rocker.skill` (22px, 70px): a shorter row would knock this column's
                  // hairlines out of register with its neighbours' for every row below it.
                  <span className="font-mono flex h-[22px] min-w-[70px] shrink-0 items-center gap-[7px] text-[11px] text-[var(--mute)]">
                    <span className="size-2 shrink-0 rounded-full bg-[var(--mute)]" aria-hidden="true" />
                    {row.kind === "plugin" ? "fixed" : "with plugin"}
                  </span>
                )}
              </div>
              ))}
            </div>
          )}
        </section>

        <section className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]">
          <header className="border-b border-[var(--hair)] px-3 py-2 text-[12px] font-semibold tracking-[0.03em] uppercase">
            Trip log
          </header>
          <div className="flex flex-col gap-[9px] px-3 py-2.5">
            {log.length === 0 ? (
              <p className="text-[13px] text-[var(--mute)]">No trips yet this session.</p>
            ) : (
              log.map((entry, index) => (
                <div key={`${entry.at}-${entry.tag}-${index}`} className="flex items-baseline gap-[9px]">
                  <span className="font-mono shrink-0 text-[11px] text-[var(--mute)]">{entry.at}</span>
                  <span
                    className={`shrink-0 px-1.5 py-0.5 text-[10px] font-semibold tracking-[0.03em] ${tripTagClass(entry.tag)}`}
                  >
                    {entry.tag}
                  </span>
                  <span className="text-[12px] leading-[1.35]">{entry.text}</span>
                </div>
              ))
            )}
          </div>
        </section>
      </div>
    </div>
  );
}
