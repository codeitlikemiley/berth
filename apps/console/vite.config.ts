import path from "node:path";
import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

const root = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(root, "src"),
    },
  },
  server: {
    proxy: {
      "/v1": { target: "http://127.0.0.1:7432", ws: true },
      "/healthz": { target: "http://127.0.0.1:7432" },
    },
  },
  test: {
    // Playwright owns e2e/; vitest would otherwise try to run those specs.
    exclude: ["node_modules/**", "dist/**", "e2e/**"],
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
