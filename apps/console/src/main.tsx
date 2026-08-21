import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router-dom";

import { bindApi, createApi } from "@/api/node";
import { App } from "@/app";
import { getToken, setToken } from "@/lib/auth";
import { initTheme } from "@/lib/theme";

import "./index.css";

initTheme();

const api = createApi("node", {
  base: "",
  getToken,
  setToken: (token) => {
    setToken(token);
    if (token === null && window.location.pathname !== "/pair") {
      window.location.replace("/pair");
    }
  },
});
bindApi(api);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <BrowserRouter>
      <App />
    </BrowserRouter>
  </StrictMode>,
);
