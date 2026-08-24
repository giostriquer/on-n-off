import {
  createRootRoute,
  createRoute,
  createRouter,
  lazyRouteComponent,
  redirect,
} from "@tanstack/react-router";
import { AppShell } from "@/features/shell/AppShell";

const rootRoute = createRootRoute({
  component: AppShell,
});

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  beforeLoad: () => {
    throw redirect({ to: "/overview" });
  },
});

const overviewRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/overview",
  component: lazyRouteComponent(() => import("@/routes/overview"), "OverviewRoute"),
});

const pluginsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/plugins",
  component: lazyRouteComponent(() => import("@/routes/plugins"), "PluginsRoute"),
});

const skillsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/skills",
  component: lazyRouteComponent(() => import("@/routes/skills"), "SkillsRoute"),
});

const mcpRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/mcp",
  component: lazyRouteComponent(() => import("@/routes/mcp"), "McpRoute"),
});

const usageRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/usage",
  component: lazyRouteComponent(() => import("@/routes/usage"), "UsageRoute"),
});

const limitsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/limits",
  component: lazyRouteComponent(() => import("@/routes/limits"), "LimitsRoute"),
});

const githubRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/github",
  component: lazyRouteComponent(() => import("@/routes/github"), "GithubRoute"),
});

const configRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/config",
  component: lazyRouteComponent(() => import("@/routes/config"), "ConfigRoute"),
});

const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  component: lazyRouteComponent(() => import("@/routes/settings"), "SettingsRoute"),
});

const routeTree = rootRoute.addChildren([
  indexRoute,
  overviewRoute,
  pluginsRoute,
  skillsRoute,
  mcpRoute,
  usageRoute,
  limitsRoute,
  githubRoute,
  configRoute,
  settingsRoute,
]);

export const router = createRouter({
  routeTree,
  defaultPreload: "intent",
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
