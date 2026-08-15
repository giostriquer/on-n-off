import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { McpList } from "./McpList";
import type { McpServerDto } from "$lib/types";

const filterMcpListCall = vi.hoisted(() => vi.fn());

vi.mock("$lib/filterTab", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/filterTab")>();
  return {
    ...actual,
    filterMcpList: (...args: Parameters<typeof actual.filterMcpList>) => {
      filterMcpListCall();
      return actual.filterMcpList(...args);
    },
  };
});

const server: McpServerDto = {
  id: "github",
  name: "GitHub",
  system: "stdio",
  source: "npx github-mcp",
  enabled: true,
  togglable: true,
};

describe("McpList", () => {
  it("uses an already-derived server list without filtering it again", () => {
    render(
      <McpList
        tab={{ plugins: [], userSkills: [], mcpServers: [server] }}
        servers={[server]}
        filterQuery="github"
        onToggle={vi.fn()}
      />,
    );

    expect(screen.getByText("GitHub")).toBeInTheDocument();
    expect(filterMcpListCall).not.toHaveBeenCalled();
  });
});
