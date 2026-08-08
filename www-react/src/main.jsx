import React from "react";
import { createRoot } from "react-dom/client";
import "@xyflow/react/dist/style.css";
import "./styles.css";
import App from "./components/App.jsx";
import { brokerFetch, onNotification, transport } from "./broker/transport.js";

// Harness hook (browser-tier CI + the ETSI proxy forward through the page).
window.brokerFetch = brokerFetch;
window.__antares = { transport, notifications: [] };
onNotification((doc) => {
  const n = window.__antares.notifications;
  n.push(doc);
  if (n.length > 50) n.shift();
});

createRoot(document.getElementById("root")).render(<App />);
