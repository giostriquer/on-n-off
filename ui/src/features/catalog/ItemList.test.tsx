import userEvent from "@testing-library/user-event";
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ItemList } from "./ItemList";
import type { AgentTabDto } from "$lib/types";

const rockerRender = vi.hoisted(() => vi.fn());
const filterSkillListCall = vi.hoisted(() => vi.fn());
const sortPluginsCall = vi.hoisted(() => vi.fn());

vi.mock("@/features/agents/Rocker", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/features/agents/Rocker")>();
  return {
    Rocker: (props: React.ComponentProps<typeof actual.Rocker>) => {
      rockerRender(props.ariaLabel);
      return <actual.Rocker {...props} />;
    },
  };
});

vi.mock("$lib/filterTab", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/filterTab")>();
  return {
    ...actual,
    filterSkillList: (...args: Parameters<typeof actual.filterSkillList>) => {
      filterSkillListCall();
      return actual.filterSkillList(...args);
    },
  };
});

vi.mock("$lib/catalog", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/catalog")>();
  return {
    ...actual,
    sortPlugins: (...args: Parameters<typeof actual.sortPlugins>) => {
      sortPluginsCall();
      return actual.sortPlugins(...args);
    },
  };
});

const largeSkills: AgentTabDto = {
  plugins: [],
  userSkills: Array.from({ length: 200 }, (_, index) => ({
    id: `skill-${String(199 - index).padStart(3, "0")}`,
    pluginId: null,
    name: `Skill ${String(199 - index).padStart(3, "0")}`,
    description: `Fixture skill ${199 - index}`,
    enabled: true,
    togglable: true,
  })),
  mcpServers: [],
};

beforeEach(() => {
  rockerRender.mockClear();
  filterSkillListCall.mockClear();
  sortPluginsCall.mockClear();
});

describe("ItemList", () => {
  it("preserves a large Skills list order, controls, keyboard behavior, and stable row renders", async () => {
    const user = userEvent.setup();
    const onToggleSkill = vi.fn();
    const props = {
      kind: "skill" as const,
      tab: largeSkills,
      items: [...largeSkills.userSkills].reverse(),
      expandedIds: new Set<string>(),
      cliOk: true,
      pluginToggle: true,
      onToggleExpand: vi.fn(),
      onTogglePlugin: vi.fn(),
      onToggleSkill,
      onUninstall: vi.fn(),
    };
    const view = render(<ItemList {...props} />);
    const articles = view.container.querySelectorAll("article");

    expect(articles).toHaveLength(200);
    expect(articles[0]).toHaveTextContent("Skill 000");
    expect(articles[199]).toHaveTextContent("Skill 199");
    expect(screen.getAllByRole("button")).toHaveLength(203);
    expect(rockerRender).toHaveBeenCalledTimes(200);
    expect(filterSkillListCall).not.toHaveBeenCalled();

    await user.tab();
    expect(screen.getByRole("button", { name: "all" })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole("button", { name: "on" })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole("button", { name: "off" })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole("button", { name: "Skill 000 on" })).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(onToggleSkill).toHaveBeenCalledWith(
      expect.objectContaining({ id: "skill-000" }),
      false,
    );

    view.rerender(<ItemList {...props} />);
    expect(rockerRender).toHaveBeenCalledTimes(200);
    expect(filterSkillListCall).not.toHaveBeenCalled();
  });

  it("does not sort an already-derived plugin list again", () => {
    const plugin = {
      id: "workbench@workshop",
      name: "workbench",
      source: "workshop",
      version: "0.23.0",
      upstream: "0.23.0",
      enabled: true,
      togglable: true,
      skills: [],
    };
    render(
      <ItemList
        kind="plugin"
        tab={{ plugins: [plugin], userSkills: [], mcpServers: [] }}
        items={[plugin]}
        expandedIds={new Set()}
        cliOk
        pluginToggle
        onToggleExpand={vi.fn()}
        onTogglePlugin={vi.fn()}
        onToggleSkill={vi.fn()}
        onUninstall={vi.fn()}
      />,
    );

    expect(screen.getByText("workbench")).toBeInTheDocument();
    expect(sortPluginsCall).not.toHaveBeenCalled();
  });
});
