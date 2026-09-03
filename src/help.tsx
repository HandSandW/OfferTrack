import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { HelpWindow } from "./features/help/HelpWindow";
import { ThemeProvider } from "./features/theme/ThemeProvider";
import "./styles.css";
import "./features/help/help.css";

const root = document.getElementById("root");
if (!root) throw new Error("OfferTrack help root was not found.");
createRoot(root).render(
  <StrictMode>
    <ThemeProvider>
      <HelpWindow />
    </ThemeProvider>
  </StrictMode>,
);
