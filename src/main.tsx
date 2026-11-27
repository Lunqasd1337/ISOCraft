import React from "react";
import ReactDOM from "react-dom/client";
import "./i18n";
import "./index.css";
import App from "./App";
import { AppContextMenu } from "./components/AppContextMenu";
import { ErrorBoundary } from "./components/ErrorBoundary";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
    <AppContextMenu />
  </React.StrictMode>
);
