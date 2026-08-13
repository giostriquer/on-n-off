import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import InstallSheet from "./InstallSheet.svelte";

describe("InstallSheet", () => {
  it("keeps Install disabled and shows an error for empty or garbage input", async () => {
    const user = userEvent.setup();
    render(InstallSheet, {
      props: {
        agentName: "Claude",
        onCancel: () => undefined,
        onInstall: () => undefined,
      },
    });

    const install = screen.getByRole("button", { name: "Install" });
    expect(install.hasAttribute("disabled")).toBe(true);

    const field = screen.getByPlaceholderText("owner/repo or https://…");
    await user.type(field, "???");
    expect(screen.getByText("Use an HTTPS git URL or owner/repo.")).toBeTruthy();
    expect(install.hasAttribute("disabled")).toBe(true);
  });

  it("enables Install for owner/repo", async () => {
    const user = userEvent.setup();
    render(InstallSheet, {
      props: {
        agentName: "Claude",
        onCancel: () => undefined,
        onInstall: () => undefined,
      },
    });

    await user.type(screen.getByPlaceholderText("owner/repo or https://…"), "acme/tools");
    expect(screen.getByRole("button", { name: "Install" }).hasAttribute("disabled")).toBe(false);
  });
});
