import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { ApplicationDetailWindow } from "./features/applications/ApplicationDetailWindow";
import { ThemeProvider } from "./features/theme/ThemeProvider";
import { DraftGuardProvider } from "./shared/DraftGuardProvider";
import "./styles.css";

const root = document.getElementById("root");

if (!root) {
  throw new Error("OfferTrack root element was not found.");
}

const detailWindow =
  new URLSearchParams(window.location.search).get("window") ===
  "application-detail";

if (detailWindow) document.body.classList.add("detail-window-body");

createRoot(root).render(
  <StrictMode>
    {detailWindow ? (
      <ThemeProvider>
        <DraftGuardProvider>
          <ApplicationDetailWindow />
        </DraftGuardProvider>
      </ThemeProvider>
    ) : (
      <App />
    )}
  </StrictMode>,
);
