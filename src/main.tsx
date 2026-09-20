import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import RelayApp from "./RelayApp";

const root = document.getElementById("root");

if (!root) {
  throw new Error("Relay could not find its application root.");
}

createRoot(root).render(
  <StrictMode>
    <RelayApp />
  </StrictMode>,
);
