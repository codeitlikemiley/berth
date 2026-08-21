import { describe, expect, it } from "vitest";

import { canEmbedViewer } from "./viewer";

const viewer = "http://127.0.0.1:6080/vnc.html";

describe("canEmbedViewer", () => {
  it("does not embed loopback noVNC from a tunneled origin", () => {
    expect(canEmbedViewer("https://x.trycloudflare.com", viewer)).toBe(false);
  });

  it("embeds loopback noVNC from a loopback console origin", () => {
    expect(canEmbedViewer("http://127.0.0.1:7432", viewer)).toBe(true);
  });

  it("embeds from localhost and ::1 console origins", () => {
    expect(canEmbedViewer("http://localhost:7432", viewer)).toBe(true);
    expect(canEmbedViewer("http://[::1]:7432", viewer)).toBe(true);
  });

  it("does not embed a non-loopback viewer even on loopback", () => {
    expect(
      canEmbedViewer(
        "http://127.0.0.1:7432",
        "https://x.trycloudflare.com/vnc.html",
      ),
    ).toBe(false);
  });
});
