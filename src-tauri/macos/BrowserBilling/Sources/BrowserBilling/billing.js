// Adapted from CodexBar's OpenAISubscriptionMetadata capture strategy (MIT).
// Only normalized subscription metadata leaves this isolated, temporary WebKit reader.
(() => {
  if (window.location.origin !== "https://chatgpt.com" || window !== window.top) return;
  const originalFetch = window.fetch.bind(window);
  let captured = null;
  let expected = null;
  function tokenIdentity(token) {
    if (typeof token !== "string") return null;
    const claims = JSON.parse(atob(token.split(".")[1].replace(/-/g, "+").replace(/_/g, "/")));
    return claims["https://api.openai.com/auth"];
  }
  function tokenAccount(token) { return tokenIdentity(token)?.chatgpt_account_id; }
  function fields(body) {
    const date = Object.hasOwn(body, "active_until") ? body.active_until : body.activeUntil;
    const renew = Object.hasOwn(body, "will_renew") ? body.will_renew : body.willRenew;
    if (!(date === null || (typeof date === "string" && date.length < 64)) ||
        !(renew === null || typeof renew === "boolean")) return null;
    return { active_until: date, will_renew: renew };
  }
  window.fetch = async (...args) => {
    const account = expected;
    const response = await originalFetch(...args);
    try {
      const input = args[0];
      const url = new URL(typeof input === "string" ? input : input.url, window.location.href);
      const method = args[1]?.method ?? input?.method ?? "GET";
      const headers = new Headers(args[1]?.headers ?? input?.headers);
      const target = headers.get("ChatGPT-Account-ID");
      const bearer = headers.get("Authorization")?.replace(/^Bearer /i, "");
      if (account && account === expected && response.ok && method.toUpperCase() === "GET" &&
          url.origin === "https://chatgpt.com" && url.pathname === "/backend-api/subscriptions" &&
          (target === account || (!target && bearer && tokenAccount(bearer) === account))) {
        captured = fields(await response.clone().json());
      }
    } catch (_) { /* Keep the provider's response untouched. */ }
    return response;
  };
  window.__onNOffReadBilling = async (account, expectedUser) => {
    let failure = "unavailable";
    expected = account;
    captured = null;
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 12000);
    async function session() {
      const response = await originalFetch("/api/auth/session", { credentials: "include", signal: controller.signal });
      if (!response.ok) throw new Error("session unavailable");
      const token = (await response.json()).accessToken;
      const identity = tokenIdentity(token);
      if (expectedUser && identity?.chatgpt_user_id !== expectedUser) {
        failure = identity?.chatgpt_user_id ? "accountMismatch" : "unavailable";
        throw new Error("user could not be verified");
      }
      if (identity?.chatgpt_account_id !== account) {
        if (!expectedUser) { failure = identity?.chatgpt_account_id ? "accountMismatch" : "unavailable"; throw new Error("account mismatch"); }
        const memberships = await originalFetch("/backend-api/accounts", {
          credentials: "include", signal: controller.signal,
          headers: { Authorization: "Bearer " + token, Accept: "application/json" },
        });
        if (!memberships.ok) throw new Error("membership unavailable");
        const body = await memberships.json();
        if (!Array.isArray(body.items)) throw new Error("membership unavailable");
        if (!body.items.some(item => item?.id === account)) { failure = "accountMismatch"; throw new Error("workspace not available"); }
      }
      return token;
    }
    try {
      const token = await session();
      let result = null;
      try {
        const response = await originalFetch("/backend-api/subscriptions", {
          credentials: "include", signal: controller.signal,
          headers: { Authorization: "Bearer " + token, "ChatGPT-Account-ID": account, Accept: "application/json" },
        });
        if (response.ok) result = fields(await response.json());
      } catch (_) { /* Let the provider's billing page perform the request. */ }
      if (!result) {
        window.location.hash = "#settings/Account";
        await new Promise(resolve => setTimeout(resolve, 100));
        window.location.hash = "#settings/Billing";
        while (!captured && !controller.signal.aborted) await new Promise(resolve => setTimeout(resolve, 200));
        result = captured;
      }
      if (!result || controller.signal.aborted) return {error:"unavailable"};
      await session();
      return { accountId: account, ...result };
    } catch (_) { return {error:failure}; }
    finally { clearTimeout(timer); controller.abort(); expected = null; captured = null; }
  };
})();
