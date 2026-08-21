import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router-dom";

import { bindApi, createApi } from "@/api/node";
import { App } from "@/app";
import { dropRejectedBearer, getToken, setToken } from "@/lib/auth";
import { initTheme } from "@/lib/theme";

import "./index.css";

initTheme();

const api = createApi("node", {
  base: "",
  getToken,
  setToken: (token) => {
    setToken(token);
    if (token === null && !getToken() && window.location.pathname !== "/pair") {
      window.location.replace("/pair");
    }
  },
  dropRejectedBearer,
});
bindApi(api);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <BrowserRouter>
      <App />
    </BrowserRouter>
  </StrictMode>,
);
