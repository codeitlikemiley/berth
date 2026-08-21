const BEARER_KEY = "berth.bearer";

export function url_is_loopback(origin: string): boolean {
  let hostname: string;
  try {
    hostname = new URL(origin).hostname;
  } catch {
    return false;
  }
  return (
    hostname === "127.0.0.1" ||
    hostname === "localhost" ||
    hostname === "::1" ||
    hostname === "[::1]"
  );
}

function loopbackOrigin(): boolean {
  return url_is_loopback(window.location.origin);
}

export function getToken(): string | null {
  const session = sessionStorage.getItem(BEARER_KEY);
  if (session) {
    return session;
  }
  if (!loopbackOrigin()) {
    return null;
  }
  const local = localStorage.getItem(BEARER_KEY);
  if (local) {
    sessionStorage.setItem(BEARER_KEY, local);
    return local;
  }
  return null;
}

export function setToken(token: string | null): void {
  if (token === null) {
    sessionStorage.removeItem(BEARER_KEY);
    return;
  }
  sessionStorage.setItem(BEARER_KEY, token);
}

/** 401 must not wipe a newer remembered bearer from another tab. */
export function dropRejectedBearer(rejected: string | null): void {
  if (rejected === null) {
    sessionStorage.removeItem(BEARER_KEY);
    return;
  }
  if (sessionStorage.getItem(BEARER_KEY) === rejected) {
    sessionStorage.removeItem(BEARER_KEY);
  }
  if (localStorage.getItem(BEARER_KEY) === rejected) {
    localStorage.removeItem(BEARER_KEY);
  }
}

export function rememberToken(token: string): void {
  if (!loopbackOrigin()) {
    return;
  }
  localStorage.setItem(BEARER_KEY, token);
}

/** Revoke issues a new bearer; a stale remembered token would 401 on the next visit. */
export function refreshRememberedToken(token: string): void {
  if (!loopbackOrigin()) {
    return;
  }
  if (localStorage.getItem(BEARER_KEY) === null) {
    return;
  }
  localStorage.setItem(BEARER_KEY, token);
}
