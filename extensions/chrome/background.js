/**
 * Send to AIKD — Manifest V3 service worker.
 *
 * Context menus:
 *   - "Send selection to AIKD" -> POST /api/send  (tags: ["web-selection"])
 *   - "Clip page to AIKD"      -> POST /api/send  (tags: ["web-clip"])
 *
 * Settings live in chrome.storage.local:
 *   aikdPort   number  default 9090 (set 9091 for sandbox testing)
 *   aikdNotify boolean default true
 *
 * Feedback: action badge (OK green / ERR red) plus a desktop notification
 * that surfaces how many secrets the daemon's Shield redacted.
 */
"use strict";

const DEFAULT_PORT = 9090;
const HOSTS = ["127.0.0.1", "localhost"];
const REQUEST_TIMEOUT_MS = 9000;
const BADGE_TTL_MS = 4000;

const BADGE_OK = { text: "OK", color: [34, 197, 94, 255] };
const BADGE_ERR = { text: "ERR", color: [239, 68, 68, 255] };

const MENU_SELECTION = "aikd-send-selection";
const MENU_CLIP = "aikd-clip-page";

function normalizePort(value) {
  const port = Number.parseInt(value, 10);
  return Number.isFinite(port) && port >= 1 && port <= 65535 ? port : DEFAULT_PORT;
}

function readSettings() {
  return new Promise((resolve) => {
    chrome.storage.local.get({ aikdPort: DEFAULT_PORT, aikdNotify: true }, (items) => {
      resolve({
        port: normalizePort(items && items.aikdPort),
        notify: !items || items.aikdNotify !== false,
      });
    });
  });
}

/**
 * Calls the daemon, trying 127.0.0.1 first and localhost as a fallback.
 * Unwraps the shared `{ success, data, error }` REST envelope.
 */
async function apiRequest(path, init, port) {
  let lastError = null;

  for (const host of HOSTS) {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);
    try {
      const response = await fetch(`http://${host}:${port}${path}`, {
        ...init,
        signal: controller.signal,
        cache: "no-store",
      });
      clearTimeout(timer);

      const text = await response.text();
      let payload = null;
      try {
        payload = text ? JSON.parse(text) : null;
      } catch (_) {
        payload = null;
      }

      if (payload && payload.success === false) {
        return {
          ok: false,
          status: response.status,
          error: payload.error || `HTTP ${response.status}`,
        };
      }
      if (!response.ok) {
        return {
          ok: false,
          status: response.status,
          error: (payload && payload.error) || `HTTP ${response.status}`,
        };
      }

      const data =
        payload && Object.prototype.hasOwnProperty.call(payload, "data") ? payload.data : payload;
      return { ok: true, status: response.status, data };
    } catch (error) {
      clearTimeout(timer);
      lastError = error;
    }
  }

  const aborted = lastError && lastError.name === "AbortError";
  return {
    ok: false,
    status: 0,
    error: aborted ? "AIKD daemon timed out" : `AIKD daemon not reachable on port ${port}`,
  };
}

function flashBadge(badge) {
  try {
    chrome.action.setBadgeBackgroundColor({ color: badge.color }, () => void chrome.runtime.lastError);
    chrome.action.setBadgeText({ text: badge.text }, () => void chrome.runtime.lastError);
    setTimeout(() => {
      chrome.action.setBadgeText({ text: "" }, () => void chrome.runtime.lastError);
    }, BADGE_TTL_MS);
  } catch (_) {
    /* badges are cosmetic */
  }
}

async function showNotification(title, message) {
  const { notify } = await readSettings();
  if (!notify) return;
  try {
    chrome.notifications.create(
      `aikd-${Date.now()}`,
      {
        type: "basic",
        iconUrl: chrome.runtime.getURL("icons/icon128.png"),
        title: title.slice(0, 64),
        message: message.slice(0, 240),
        priority: 0,
      },
      () => void chrome.runtime.lastError
    );
  } catch (_) {
    /* notifications are best-effort */
  }
}

async function report(ok, title, message) {
  flashBadge(ok ? BADGE_OK : BADGE_ERR);
  await showNotification(ok ? `AIKD · ${title}` : `AIKD · ${title} failed`, message);
}

async function saveNote(payload) {
  const { port } = await readSettings();
  const result = await apiRequest(
    "/api/send",
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    },
    port
  );
  if (!result.ok) throw new Error(result.error || "AIKD request failed");
  return result.data || {};
}

function truncate(value, max) {
  const text = String(value || "").replace(/\s+/g, " ").trim();
  return text.length > max ? `${text.slice(0, max).trimEnd()}…` : text;
}

function hostOf(url) {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch (_) {
    return "web page";
  }
}

function selectionTitle(pageTitle, selection) {
  const firstLine =
    selection
      .split(/\r?\n/)
      .map((line) => line.trim())
      .find(Boolean) || "";
  const cleaned = firstLine.replace(/^[#>*\-\s]+/, "");
  return truncate(cleaned.length >= 12 ? cleaned : pageTitle, 90) || "Web selection";
}

function successMessage(data) {
  const redacted = Number(data && data.redacted_count) || 0;
  const shield =
    redacted > 0
      ? `Shield: ${redacted} secret${redacted === 1 ? "" : "s"} redacted`
      : "Shield: clean";
  return data && data.folder ? `Filed in ${data.folder} · ${shield}` : shield;
}

async function handleSelection(info, tab) {
  const selection = String(info.selectionText || "").trim();
  if (!selection) throw new Error("No text selected.");

  const url = (tab && tab.url) || info.pageUrl || "";
  const pageTitle = (tab && tab.title) || hostOf(url);

  const data = await saveNote({
    title: selectionTitle(pageTitle, selection),
    body: selection,
    kind: "note",
    scope: "global",
    folder: "Inbox",
    project: "",
    tags: ["web-selection"],
    source: "chrome-extension",
    source_url: url,
    pinned: false,
  });

  await report(true, "Selection sent", successMessage(data));
}

/**
 * Injected into the page by chrome.scripting.executeScript.
 * Must stay fully self-contained (it is serialized, not closed over).
 */
function extractPageContent() {
  const meta = (selector) => {
    const node = document.querySelector(selector);
    return node && node.content ? String(node.content).trim() : "";
  };
  const root = document.querySelector("article, main, [role='main']") || document.body;
  const raw = root && typeof root.innerText === "string" ? root.innerText : "";
  const text = raw
    .replace(/[ \t]+\n/g, "\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();

  return {
    title: document.title || location.hostname,
    url: location.href,
    description: meta('meta[name="description"]') || meta('meta[property="og:description"]'),
    text: text.slice(0, 12000), // keep clips token-friendly; mirrors CLIP limit in README
  };
}

async function handleClipPage(tab) {
  if (!tab || typeof tab.id !== "number") throw new Error("No active tab to clip.");

  let extraction = null;
  try {
    const results = await chrome.scripting.executeScript({
      target: { tabId: tab.id },
      func: extractPageContent,
    });
    extraction = results && results[0] ? results[0].result : null;
  } catch (_) {
    throw new Error("This page cannot be read (browser and store pages are protected).");
  }

  if (!extraction) throw new Error("Could not extract page content.");
  if (!extraction.text && !extraction.description) throw new Error("Nothing readable on this page.");

  const url = extraction.url || tab.url || "";
  const title = truncate(extraction.title || tab.title || hostOf(url), 120) || "Web clip";

  const pieces = [`> Source: ${url}`];
  if (extraction.description) pieces.push(`> ${truncate(extraction.description, 300)}`);
  if (extraction.text) pieces.push(extraction.text);

  const data = await saveNote({
    title,
    body: pieces.join("\n\n").trim(),
    kind: "note",
    scope: "global",
    folder: "Inbox",
    project: "",
    tags: ["web-clip"],
    source: "chrome-extension",
    source_url: url,
    pinned: false,
  });

  await report(true, "Page clipped", successMessage(data));
}

function registerContextMenus() {
  chrome.contextMenus.removeAll(() => {
    void chrome.runtime.lastError;
    chrome.contextMenus.create(
      {
        id: MENU_SELECTION,
        title: "Send selection to AIKD",
        contexts: ["selection"],
      },
      () => void chrome.runtime.lastError
    );
    chrome.contextMenus.create(
      {
        id: MENU_CLIP,
        title: "Clip page to AIKD",
        contexts: ["page"],
      },
      () => void chrome.runtime.lastError
    );
  });
}

chrome.runtime.onInstalled.addListener(() => {
  registerContextMenus();
});

chrome.runtime.onStartup.addListener(() => {
  registerContextMenus();
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
  let job = null;
  let label = "";
  if (info.menuItemId === MENU_SELECTION) {
    label = "Selection";
    job = handleSelection(info, tab);
  } else if (info.menuItemId === MENU_CLIP) {
    label = "Page clip";
    job = handleClipPage(tab);
  }
  if (!job) return;

  job.catch((error) => {
    report(false, label, error && error.message ? error.message : String(error));
  });
});
