import { render, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import Overview from "./Overview.svelte";

describe("Overview", () => {
  it("renders gauges, live rows, and trip log", () => {
    render(Overview, {
      props: {
        counts: {
          plugins: { on: 1, total: 2 },
          skills: { on: 2, total: 4 },
          mcp: { on: 1, total: 2 },
        },
        rows: [
          {
            kind: "plugin",
            id: "workbench@workshop",
            name: "workbench",
            meta: "plugin · workshop",
            enabled: true,
            togglable: true,
          },
          {
            kind: "skill",
            id: "statusline",
            name: "statusline",
            meta: "skill · user",
            enabled: true,
            togglable: true,
          },
          {
            kind: "mcp",
            id: "github",
            name: "github",
            meta: "mcp · npx -y @modelcontextprotocol/server-github",
            enabled: true,
            togglable: true,
          },
        ],
        drift: [
          {
            kind: "plugin",
            id: "workbench@workshop",
            name: "workbench",
            version: "0.22.1",
            upstream: "0.23.0",
          },
        ],
        log: [{ at: "14:02", tag: "ON", text: "workbench enabled for Claude · all projects" }],
        pluginToggle: true,
        onToggle: () => undefined,
      },
    });
    expect(screen.getByText("Plugins")).toBeTruthy();
    expect(screen.getByText("Skills")).toBeTruthy();
    expect(screen.getByText("MCP servers")).toBeTruthy();
    expect(screen.getAllByText("1")).toHaveLength(2);
    expect(screen.getAllByText("on / 2 installed")).toHaveLength(2);
    expect(screen.getByText("Out of sync with upstream")).toBeTruthy();
    expect(screen.getByText("1 plugin behind catalog")).toBeTruthy();
    expect(screen.getByText("v0.22.1")).toBeTruthy();
    expect(screen.getByText("v0.23.0")).toBeTruthy();
    expect(screen.getByText("Update")).toBeTruthy();
    expect(screen.getByText("Live on this scope")).toBeTruthy();
    expect(screen.getAllByText("workbench").length).toBeGreaterThan(0);
    expect(screen.getByText("statusline")).toBeTruthy();
    expect(screen.getByText("github")).toBeTruthy();
    expect(screen.getByText("Trip log")).toBeTruthy();
    expect(screen.getByText("14:02")).toBeTruthy();
    expect(screen.getByText("workbench enabled for Claude · all projects")).toBeTruthy();
  });

  it("shows empty live and trip copy", () => {
    render(Overview, {
      props: {
        counts: {
          plugins: { on: 0, total: 0 },
          skills: { on: 0, total: 0 },
          mcp: { on: 0, total: 0 },
        },
        rows: [],
        log: [],
        onToggle: () => undefined,
      },
    });
    expect(screen.getByText("Nothing live on this circuit.")).toBeTruthy();
    expect(screen.getByText("No trips yet this session.")).toBeTruthy();
  });
});
