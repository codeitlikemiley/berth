import { url_is_loopback } from "@/lib/auth";

/** A tunneled tab's 127.0.0.1 is the laptop, not the parked box. */
export function canEmbedViewer(
  browserOrigin: string,
  viewerUrl: string,
): boolean {
  return url_is_loopback(browserOrigin) && url_is_loopback(viewerUrl);
}
