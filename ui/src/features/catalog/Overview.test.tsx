import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Overview } from "./Overview";

vi.mock("@tanstack/react-router", () => ({
  Link: ({
    to,
    children,
    className,
    ...rest
  }: {
    to: string;
    children?: React.ReactNode;
    className?: string;
    [key: string]: unknown;
  }) => (
    <a href={to} className={className} {...rest}>
      {children}
    </a>
  ),
}));

vi.mock("@/features/usage/OverviewUsageCard", () => ({
  OverviewUsageCard: () => null,
}));

describe("Overview", () => {
  it("renders gauges, live rows, and trip log", () => {
    render(
      <Overview
        counts={{
          plugins: { on: 1, total: 2 },
          skills: { on: 2, total: 4 },
          mcp: { on: 1, total: 2 },
        }}
        rows={[
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
        ]}
        drift={[
          {
            kind: "plugin",
            id: "workbench@workshop",
            name: "workbench",
            version: "0.22.1",
            upstream: "0.23.0",
          },
        ]}
        log={[{ at: "14:02", tag: "ON", text: "workbench enabled for Claude · all projects" }]}
        pluginToggle={true}
        onToggle={() => undefined}
      />,
    );
    expect(screen.getByRole("link", { name: /Plugins: 1 on of 2 installed/i })).toBeTruthy();
    expect(screen.getByRole("link", { name: /Skills: 2 on of 4 installed/i })).toBeTruthy();
    expect(screen.getByRole("link", { name: /MCP servers: 1 on of 2 installed/i })).toBeTruthy();
    expect(screen.getByRole("link", { name: /Plugins/i }).getAttribute("href")).toBe("/plugins");
    expect(screen.getByRole("link", { name: /Skills/i }).getAttribute("href")).toBe("/skills");
    expect(screen.getByRole("link", { name: /MCP servers/i }).getAttribute("href")).toBe("/mcp");
    expect(screen.getAllByText("1")).toHaveLength(2);
    expect(screen.getAllByText("on / 2 installed")).toHaveLength(2);
    expect(screen.getByText("Out of sync with upstream")).toBeTruthy();
    expect(screen.getByText("1 plugin outdated")).toBeTruthy();
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
    render(
      <Overview
        counts={{
          plugins: { on: 0, total: 0 },
          skills: { on: 0, total: 0 },
          mcp: { on: 0, total: 0 },
        }}
        rows={[]}
        log={[]}
        onToggle={() => undefined}
      />,
    );
    expect(screen.getByText("Nothing live on this circuit.")).toBeTruthy();
    expect(screen.getByText("No trips yet this session.")).toBeTruthy();
  });
});
