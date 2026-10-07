import "./styles.css";

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { initPwa } from "@/lib/pwa";

import { App } from "./App.tsx";

initPwa();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
