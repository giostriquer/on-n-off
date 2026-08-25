import "./tokens.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Root } from "./Root";

function boot() {
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <Root />
    </StrictMode>,
  );
}

// `?mock[=scenario]` on a dev build runs the UI against synthetic IPC (see dev/mockIpc.ts);
// the branch is dead code in production builds, so the mock never ships.
if (import.meta.env.DEV && new URLSearchParams(window.location.search).has("mock")) {
  void import("./dev/mockIpc").then(boot);
} else {
  boot();
}
