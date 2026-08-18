import { useEffect } from "react";
import { Outlet, useNavigate, useRouterState } from "@tanstack/react-router";
import { RefreshCw } from "lucide-react";
import { AgentBanner } from "@/features/agents/AgentBanner";
import { ConfirmDialog } from "@/features/catalog/ConfirmDialog";
import { InstallSheet } from "@/features/catalog/InstallSheet";
import { ScopeBar } from "@/features/scope/ScopeBar";
import {
  SCREEN_PATH,
  pathToScreen,
  readScreen,
  useAgentSession,
} from "@/features/session/SessionProvider";
import { LeftRail } from "@/features/shell/LeftRail";
import { UpdateStrip } from "@/features/updater/UpdateStrip";
import { preloadUsageChart } from "@/features/usage/LazyUsageChart";
import { globalItemCount, tallyLine, type Screen } from "$lib/catalog";
import { copy } from "$lib/copy";
import * as api from "$lib/api";
import type { AgentInfo } from "$lib/types";
import markUrl from "../../../../src-tauri/icons/128x128.png";

/** Screens that read no per-provider tab data, so they stay mounted while a provider tab loads. */
const PROVIDER_INDEPENDENT_SCREENS: ReadonlySet<Screen> = new Set<Screen>(["usage", "limits", "settings"]);

export function AppShell() {
  const navigate = useNavigate();
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const screen = pathToScreen(pathname);

  const session = useAgentSession();
  const {
    theme,
    setTheme,
    visibleAgents,
    selected,
    setSelected,
    currentAgent,
    currentTab,
    banner,
    canInstall,
    counts,
    allOn,
    cliLine,
    masterNote,
    showMasterCut,
    currentProjects,
    currentScopePath,
    scopeNote,
    installOpen,
    setInstallOpen,
    installError,
    clearInstallError,
    uninstallTarget,
    setUninstallTarget,
    setFilter,
    loadTab,
    selectScope,
    pickProjectFolder,
    openProjectPath,
    masterCut,
    pickFolder,
    install,
    confirmUninstall,
  } = session;

  useEffect(() => {
    localStorage.setItem("on-n-off.screen", screen);
  }, [screen]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void api
      .onOpenLimitsWindow(() => {
        void navigate({ to: SCREEN_PATH.limits });
      })
      .then((stop) => {
        if (disposed) {
          stop();
        } else {
          unlisten = stop;
        }
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [navigate]);

  useEffect(() => {
    if (sessionStorage.getItem("on-n-off.routed")) {
      return;
    }
    sessionStorage.setItem("on-n-off.routed", "1");
    const saved = readScreen();
    if (saved !== "overview") {
      void navigate({ to: SCREEN_PATH[saved] });
    }
  }, [navigate]);

  function onAgentTabKey(event: React.KeyboardEvent) {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") {
      return;
    }
    event.preventDefault();
    const index = visibleAgents.findIndex((agent: AgentInfo) => agent.id === selected);
    if (index < 0) {
      return;
    }
    const next =
      event.key === "ArrowRight"
        ? visibleAgents[(index + 1) % visibleAgents.length]
        : visibleAgents[(index - 1 + visibleAgents.length) % visibleAgents.length];
    setSelected(next.id);
  }

  function goScreen(next: Screen) {
    if (next === "usage") {
      void preloadUsageChart();
    }
    void navigate({ to: SCREEN_PATH[next] });
  }

  return (
    <div className="app-frame bg-[var(--void)] text-[var(--silkscreen)]">
      <header className="flex shrink-0 items-center gap-3.5 border-b border-[var(--hair)] bg-[var(--plate)] px-4 py-[11px]">
        <div className="flex shrink-0 items-center gap-2.5">
          <img src={markUrl} alt="" className="size-7 rounded-md" width={28} height={28} />
          <div className="text-[15px] font-bold tracking-[0.09em]">ON-N-OFF</div>
        </div>
        <div
          className="inline-grid grid-flow-col overflow-hidden rounded-lg border border-[var(--hair)]"
          role="tablist"
          aria-label="Agent"
          tabIndex={-1}
          onKeyDown={onAgentTabKey}
        >
          {visibleAgents.map((agent: AgentInfo) => (
            <button
              key={agent.id}
              type="button"
              role="tab"
              id={`agent-tab-${agent.id}`}
              aria-selected={selected === agent.id}
              aria-controls="agent-panel"
              tabIndex={selected === agent.id ? 0 : -1}
              className={`h-[30px] min-w-[92px] cursor-pointer rounded-none border-0 px-3 text-[11.5px] font-semibold tracking-[0.05em] uppercase ${
                selected === agent.id
                  ? "bg-[var(--fill)] text-[var(--fill-ink)]"
                  : "bg-[var(--plate)] text-[var(--mute)]"
              }`}
              onClick={() => setSelected(agent.id)}
            >
              {agent.displayName}
            </button>
          ))}
        </div>
        <div
          className="flex items-center gap-2 pl-1"
          aria-label={
            currentAgent.cliOk
              ? `${currentAgent.displayName} online`
              : `${currentAgent.displayName} offline`
          }
        >
          <span
            className={`size-[7px] rounded-full ${
              currentAgent.cliOk ? "bg-[var(--live)] shadow-[0_0_8px_var(--live)]" : "bg-[var(--mute)]"
            }`}
            aria-hidden="true"
          />
          <span className="font-mono text-[11.5px] text-[var(--mute)]">{cliLine}</span>
        </div>
        <div className="flex-1" />
        <label className="flex h-8 items-center gap-2 rounded-lg border border-[var(--hair)] bg-[var(--well)] px-2.5">
          <span className="text-[10px] font-semibold tracking-[0.04em] text-[var(--mute)]">FILTER</span>
          <span className="sr-only">{copy.filterPlaceholder}</span>
          <input
            className="w-[180px] bg-transparent text-[13px] placeholder:text-[var(--mute)] focus-visible:outline-none"
            type="search"
            placeholder={copy.filterPlaceholder}
            value={currentTab.filter}
            onChange={(event) => setFilter(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                setFilter("");
              }
            }}
          />
        </label>
        <button
          type="button"
          className="flex size-8 items-center justify-center rounded-lg border border-[var(--hair)] bg-[var(--well)] text-[var(--silkscreen)]"
          title={copy.refresh}
          aria-label={copy.refresh}
          onClick={() => void loadTab(selected, true)}
        >
          <RefreshCw className="size-3.5" aria-hidden="true" />
        </button>
        <button
          type="button"
          className="h-8 rounded-lg border border-[var(--fill)] bg-[var(--fill)] px-3.5 text-[11.5px] font-semibold tracking-[0.04em] text-[var(--fill-ink)] disabled:opacity-45"
          disabled={!canInstall}
          onClick={() => setInstallOpen(true)}
        >
          + INSTALL
        </button>
      </header>

      <UpdateStrip />

      <ScopeBar
        agentId={selected}
        projects={currentProjects}
        selectedPath={currentScopePath}
        note={scopeNote}
        tally={tallyLine(counts, currentAgent.displayName)}
        globalItems={globalItemCount(currentTab.dto)}
        onSelect={(path) => void selectScope(path)}
        onPickFolder={() => void pickProjectFolder()}
        onOpenPath={(path) => void openProjectPath(path)}
      />

      <div className="app-body">
        <LeftRail
          screen={screen}
          counts={counts}
          theme={theme}
          masterOn={allOn}
          masterNote={masterNote}
          showMasterCut={showMasterCut}
          busy={currentTab.inFlight}
          masterDisabled={!currentTab.dto || currentTab.inFlight}
          onUsageIntent={() => void preloadUsageChart()}
          onScreen={goScreen}
          onThemeChange={setTheme}
          onMaster={(enabled) => void masterCut(enabled)}
        />
        <div
          id="agent-panel"
          className="app-scroll bg-[var(--void)]"
          role="tabpanel"
          aria-labelledby={`agent-tab-${selected}`}
        >
          {banner && screen !== "settings" ? <AgentBanner message={banner} /> : null}
          {currentTab.loading && !currentTab.dto && !PROVIDER_INDEPENDENT_SCREENS.has(screen) ? (
            <p className="m-5 text-[13px] text-[var(--mute)]">Loading {currentAgent.displayName}…</p>
          ) : (
            <Outlet />
          )}
        </div>
      </div>

      {installOpen ? (
        <InstallSheet
          agentName={currentAgent.displayName}
          busy={currentTab.inFlight}
          error={installError}
          installFolder={currentAgent.installFolder}
          onCancel={() => {
            setInstallOpen(false);
            clearInstallError();
          }}
          onInstall={(source) => {
            void install(source).then((ok) => {
              if (ok) {
                goScreen("plugins");
              }
            });
          }}
          onPickFolder={pickFolder}
        />
      ) : null}

      {uninstallTarget ? (
        <ConfirmDialog
          title={copy.uninstallTitle(uninstallTarget.name)}
          body={copy.uninstallBody(currentAgent.displayName)}
          busy={currentTab.inFlight}
          onCancel={() => setUninstallTarget(null)}
          onConfirm={() => void confirmUninstall()}
        />
      ) : null}

      <style>{`
        .sr-only {
          position: absolute;
          width: 1px;
          height: 1px;
          overflow: hidden;
          clip: rect(0 0 0 0);
        }
      `}</style>
    </div>
  );
}
