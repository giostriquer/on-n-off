import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ScopeBar } from "./ScopeBar";

const projects = [
  {
    id: "e:/dev/on-n-off",
    label: "on-n-off",
    path: String.raw`E:\dev\on-n-off`,
    branch: "main",
    skillCount: 2,
    mcpCount: 1,
  },
  {
    id: "e:/dev/conoswiki",
    label: "conoswiki",
    path: String.raw`E:\dev\conoswiki`,
    branch: "",
    skillCount: 0,
    mcpCount: 0,
  },
];

describe("ScopeBar", () => {
  it("opens a searchable picker instead of pills", async () => {
    const onSelect = vi.fn();
    const onPickFolder = vi.fn();
    const onOpenPath = vi.fn();
    render(
      <ScopeBar
        agentId="claude"
        projects={projects}
        selectedPath={null}
        note="global agent config is the source of truth"
        tally="3 plugins · 4 skills · 0 mcps live on Claude"
        globalItems={7}
        onSelect={onSelect}
        onPickFolder={onPickFolder}
        onOpenPath={onOpenPath}
      />,
    );
    expect(screen.getByText("SCOPE")).toBeTruthy();
    expect(screen.getByText("All projects")).toBeTruthy();
    expect(screen.getByText("global config")).toBeTruthy();
    expect(screen.queryByText("Choose folder…")).toBeNull();
    await fireEvent.click(screen.getByRole("button", { name: /All projects/i }));
    expect(screen.getByPlaceholderText("Search projects, or paste a folder path…")).toBeTruthy();
    expect(screen.getByText("GLOBAL")).toBeTruthy();
    expect(screen.getByText("7 global items")).toBeTruthy();
    expect(screen.getByText("on-n-off")).toBeTruthy();
    expect(screen.getByText("main")).toBeTruthy();
    expect(screen.getByText("conoswiki")).toBeTruthy();
    await fireEvent.click(screen.getByText("on-n-off"));
    expect(onSelect).toHaveBeenCalledWith(String.raw`E:\dev\on-n-off`);
  });

  it("filters projects and pastes a folder path", async () => {
    const onOpenPath = vi.fn();
    const onPickFolder = vi.fn();
    render(
      <ScopeBar
        agentId="codex"
        projects={projects}
        selectedPath={String.raw`E:\dev\on-n-off`}
        note={String.raw`local skills · E:\dev\on-n-off`}
        tally="1 plugins · 2 skills · 1 mcps live on Codex"
        globalItems={4}
        onSelect={() => undefined}
        onPickFolder={onPickFolder}
        onOpenPath={onOpenPath}
      />,
    );
    expect(screen.getByText("2 local skills · 1 project mcps")).toBeTruthy();
    await fireEvent.click(screen.getByRole("button", { expanded: false }));
    const search = screen.getByPlaceholderText("Search projects, or paste a folder path…");
    await fireEvent.input(search, { target: { value: "cono" } });
    expect(screen.getByText("conoswiki")).toBeTruthy();
    expect(screen.getAllByText("on-n-off")).toHaveLength(1);
    await fireEvent.input(search, { target: { value: String.raw`D:\tmp\scratch` } });
    expect(screen.getByText("Open scratch")).toBeTruthy();
    expect(screen.getByText("NEW")).toBeTruthy();
    await fireEvent.click(screen.getByText("Open scratch"));
    expect(onOpenPath).toHaveBeenCalledWith(String.raw`D:\tmp\scratch`);
  });

  it("picks a folder from the picker footer", async () => {
    const onPickFolder = vi.fn();
    render(
      <ScopeBar
        agentId="claude"
        projects={projects}
        selectedPath={null}
        note="global agent config is the source of truth"
        tally="0 plugins · 0 skills · 0 mcps live on Claude"
        globalItems={0}
        onSelect={() => undefined}
        onPickFolder={onPickFolder}
        onOpenPath={() => undefined}
      />,
    );
    await fireEvent.click(screen.getByRole("button", { expanded: false }));
    await fireEvent.click(screen.getByText("Choose folder…"));
    expect(onPickFolder).toHaveBeenCalled();
  });
});
