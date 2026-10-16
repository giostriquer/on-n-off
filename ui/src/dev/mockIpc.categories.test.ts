import { expect, it } from "vitest";
import { readAccountCategories, setAccountCategory } from "$lib/api";

it("keeps the category a card saves and clears it on a blank, beside the saved logins' own", async () => {
  window.history.replaceState(null, "", "/?mock=ok&latency=0");
  await import("./mockIpc");

  expect(await setAccountCategory("muse", "email:you@example.com", "  Personal ")).toBe("Personal");
  const categories = await readAccountCategories();
  expect(categories.muse).toEqual({ "email:you@example.com": "Personal" });
  expect(categories.codex?.["codex-2"]).toBe("Client A / research");
  expect(await setAccountCategory("muse", "email:you@example.com", " ")).toBeNull();
  expect((await readAccountCategories()).muse).toEqual({});
});
