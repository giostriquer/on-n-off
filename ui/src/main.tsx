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

if (import.meta.env.DEV && new URLSearchParams(window.location.search).has("mock")) {
  void import("./dev/mockIpc").then(boot);
} else {
  boot();
}
