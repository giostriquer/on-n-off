// Exercise the shipped WebKit reader; replace only browser and HTTP boundaries.
import source from "../../../../src-tauri/macos/BrowserBilling/Sources/BrowserBilling/billing.js?raw";
import { afterEach, expect, it, vi } from "vitest";
afterEach(() => vi.useRealTimers());
const billing = {active_until:"2026-10-10T12:00:00Z",will_renew:false};
const token = (id: string) => "h." + btoa(JSON.stringify({"https://api.openai.com/auth":{chatgpt_account_id:id}})) + ".s";
const response = (body: unknown, ok = true): any => ({ok,json:async () => body,clone:() => response(body,ok)});
function setup(fetch = vi.fn(async (url: string) => response(url.includes("/api/auth/session") ? {accessToken:token("account-a")} : billing))) {
  const window: Record<string, any> = {fetch,location:{origin:"https://chatgpt.com",href:"https://chatgpt.com/",hash:""}};
  window.top = window;
  new Function("window",source)(window);
  return {window,fetch};
}
it("returns only account-bound billing metadata, without credentials", async () => {
  const {window,fetch} = setup();
  expect(await window.__onNOffReadBilling("account-a")).toEqual({accountId:"account-a",...billing});
  expect(fetch.mock.calls.map(call => call[0])).toEqual(["/api/auth/session","/backend-api/subscriptions","/api/auth/session"]);
});
it("rejects an unrelated browser session before requesting billing", async () => {
  const {window,fetch} = setup();
  expect(await window.__onNOffReadBilling("account-b")).toEqual({error:"accountMismatch"});
  expect(fetch).toHaveBeenCalledTimes(1);
});
it("preserves explicit empty billing", async () => {
  const {window} = setup(vi.fn(async url => response(url.includes("/api/auth/session") ? {accessToken:token("account-a")} : {active_until:null,will_renew:false})));
  expect(await window.__onNOffReadBilling("account-a")).toEqual({accountId:"account-a",active_until:null,will_renew:false});
});
it("captures the provider billing request after a refused direct request", async () => {
  vi.useFakeTimers();
  let calls = 0;
  const {window} = setup(vi.fn(async url => url.includes("/api/auth/session") ? response({accessToken:token("account-a")}) : response(billing,++calls > 1)));
  const result = window.__onNOffReadBilling("account-a");
  await vi.advanceTimersByTimeAsync(101);
  expect(window.location.hash).toBe("#settings/Billing");
  await window.fetch("/backend-api/subscriptions",{headers:{"ChatGPT-Account-ID":"account-b"}});
  await vi.advanceTimersByTimeAsync(200);
  await window.fetch("/backend-api/subscriptions",{headers:{"ChatGPT-Account-ID":"account-a"}});
  await vi.advanceTimersByTimeAsync(200);
  expect(await result).toEqual({accountId:"account-a",...billing});
});
it("rejects a session switch while billing is in flight", async () => {
  let sessions = 0;
  const {window} = setup(vi.fn(async url => response(url.includes("/api/auth/session") ? {accessToken:token(++sessions === 1 ? "account-a" : "account-b")} : billing)));
  expect(await window.__onNOffReadBilling("account-a")).toEqual({error:"accountMismatch"});
});
it("stops waiting when no billing response arrives", async () => {
  vi.useFakeTimers();
  const {window} = setup(vi.fn(async url => url.includes("/api/auth/session") ? response({accessToken:token("account-a")}) : response({},false)));
  const result = window.__onNOffReadBilling("account-a");
  await vi.advanceTimersByTimeAsync(12500);
  expect(await result).toEqual({error:"unavailable"});
});

const userToken = (user: string, workspace: string) => "h." + btoa(JSON.stringify({"https://api.openai.com/auth":{chatgpt_user_id:user,chatgpt_account_id:workspace}})) + ".s";
it("checks workspace membership when the correct user has a different default workspace", async () => {
  const {window,fetch} = setup(vi.fn(async url => response(url.includes("/api/auth/session") ? {accessToken:userToken("user-a","personal")} : url === "/backend-api/accounts" ? {items:[{id:"team"}]} : billing)));
  expect(await window.__onNOffReadBilling("team","user-a")).toEqual({accountId:"team",...billing});
  expect(fetch.mock.calls.filter(call=>call[0] === "/backend-api/accounts")).toHaveLength(2);
});
it("refuses another user even when both users belong to the same workspace", async () => {
  const {window,fetch} = setup(vi.fn(async url => response(url.includes("/api/auth/session") ? {accessToken:userToken("user-b","team")} : billing)));
  expect(await window.__onNOffReadBilling("team","user-a")).toEqual({error:"accountMismatch"});
  expect(fetch).toHaveBeenCalledTimes(1);
});
it("does not treat an unreadable membership list as proof of a wrong account", async () => {
  const {window} = setup(vi.fn(async url => url.includes("/api/auth/session") ? response({accessToken:userToken("user-a","personal")}) : response({},false)));
  expect(await window.__onNOffReadBilling("team","user-a")).toEqual({error:"unavailable"});
});
