/**
 * Send to AIKD — popup controller.
 *
 * Talks directly to the local daemon REST API:
 *   GET  /api/health        connection dot + latency
 *   GET  /api/overview      project dropdown, version, notes count
 *   POST /api/send          save captured content
 *   GET  /api/context-pack  copy a dense Markdown pack for web AI chats
 *
 * Settings persist in chrome.storage.local: aikdPort, aikdProject, aikdNotify,
 * aikdKind, aikdScope, aikdFolder. Capture preferences are restored on every open.
 */
"use strict";

const DEFAULT_PORT = 9090;
const HOSTS = ["127.0.0.1", "localhost"];
const MAX_TAGS = 12;
const EXTRACT_LIMIT = 12000;
const TAG_RE = /#([\p{L}\p{N}_-]{2,40})/gu;
const DOMAIN_TAGS = [
  [/chatgpt\.com|chat\.openai\.com|openai\.com/i, "chatgpt"],
  [/claude\.ai|anthropic\.com/i, "claude"],
  [/gemini\.google\.com|bard\.google\.com/i, "gemini"],
  [/copilot\.microsoft\.com/i, "copilot"],
  [/perplexity\.ai/i, "perplexity"],
  [/grok\.com/i, "grok"],
  [/deepseek\.com/i, "deepseek"],
  [/chat\.mistral\.ai|mistral\.ai/i, "mistral"],
  [/poe\.com/i, "poe"],
  [/github\.com/i, "github"],
  [/stackoverflow\.com|stackexchange\.com/i, "stackoverflow"],
  [/youtube\.com|youtu\.be/i, "youtube"],
  [/reddit\.com/i, "reddit"],
  [/medium\.com|substack\.com|dev\.to|hashnode\.com/i, "blog"],
  [/notion\.so/i, "notion"],
  [/docs\.google\.com/i, "google-docs"],
  [/arxiv\.org/i, "paper"],
];
const SECRET_RE =
  /\bsk-(?:ant-|proj-)?[A-Za-z0-9_-]{16,}|\bAIza[0-9A-Za-z_-]{20,}|\bgh[pousr]_[A-Za-z0-9_]{20,}|\bAKIA[0-9A-Z]{16}\b|\bxox[baprs]-[A-Za-z0-9-]{12,}|\bBearer\s+[A-Za-z0-9._-]{20,}|\b(?:api_key|token|secret|password)\s*[:=]\s*["']?[^\s"']{16,}/gi;

const state = {
  port: DEFAULT_PORT,
  online: false,
  checking: false,
  checkedOnce: false,
  projects: [],
  project: "",
  tags: [],
  tab: null,
  busy: false,
  healthTimer: null,
  scanTimer: null,
};

const el = {};

document.addEventListener("DOMContentLoaded", boot);

async function boot() {
  cacheElements();
  bindEvents();

  const settings = await storageGet({
    aikdPort: DEFAULT_PORT,
    aikdProject: "",
    aikdNotify: true,
    aikdKind: "note",
    aikdScope: "project",
    aikdFolder: "Inbox",
  });
  state.port = normalizePort(settings.aikdPort);
  state.project = typeof settings.aikdProject === "string" ? settings.aikdProject : "";
  el.port.value = String(state.port);
  el.notify.checked = settings.aikdNotify !== false;
  el.kind.value = pickOption(el.kind, settings.aikdKind, "note");
  el.scope.value = pickOption(el.scope, settings.aikdScope, "project");
  el.folder.value = pickOption(el.folder, settings.aikdFolder, "Inbox");
  el.project.disabled = el.scope.value === "global";

  renderTags();
  updateCounter();
  if (/Mac|iPhone|iPad/.test(navigator.platform || "")) {
    el.kbdHint.textContent = "⌘⇧Y";
  }
  await fillFromTab();

  const online = await checkHealth();
  if (!online) {
    setStatus(`No daemon on 127.0.0.1:${state.port} — run "aikd daemon" or change the port.`, "warn");
  }
  await loadProjects();

  state.healthTimer = setInterval(() => checkHealth(), 5000);
  window.addEventListener("unload", () => clearInterval(state.healthTimer));
}

function cacheElements() {
  const ids = [
    "connPill",
    "dot",
    "connText",
    "port",
    "recheck",
    "daemonMeta",
    "sourceHost",
    "selectionHint",
    "title",
    "charCount",
    "body",
    "kind",
    "scope",
    "folder",
    "project",
    "refreshProjects",
    "tagBox",
    "tagChips",
    "tagInput",
    "shield",
    "status",
    "send",
    "copyPack",
    "openStudio",
    "notify",
    "kbdHint",
  ];
  for (const id of ids) el[id] = document.getElementById(id);
}

function bindEvents() {
  el.send.addEventListener("click", onSend);
  el.copyPack.addEventListener("click", onCopyContextPack);
  el.openStudio.addEventListener("click", onOpenStudio);
  el.recheck.addEventListener("click", refreshConnection);
  el.refreshProjects.addEventListener("click", loadProjects);

  el.port.addEventListener("change", commitPort);
  el.port.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      commitPort();
    }
  });

  el.body.addEventListener("input", () => {
    updateCounter();
    scheduleSecretScan();
  });

  el.scope.addEventListener("change", () => {
    const isGlobal = el.scope.value === "global";
    el.project.disabled = isGlobal;
    persistCapturePrefs();
    if (isGlobal) setStatus("Scope global — project will be ignored.", "info");
  });
  el.kind.addEventListener("change", persistCapturePrefs);
  el.folder.addEventListener("change", persistCapturePrefs);

  el.project.addEventListener("change", () => {
    state.project = el.project.value;
    persistCapturePrefs();
  });

  el.tagInput.addEventListener("keydown", (event) => {
    if (event.key === "Enter" || event.key === ",") {
      event.preventDefault();
      addTag(el.tagInput.value);
      el.tagInput.value = "";
    } else if (event.key === "Backspace" && !el.tagInput.value) {
      state.tags.pop();
      renderTags();
    }
  });
  el.tagInput.addEventListener("blur", () => {
    addTag(el.tagInput.value);
    el.tagInput.value = "";
  });

  el.tagChips.addEventListener("click", (event) => {
    const chip = event.target.closest(".chip");
    if (!chip || !chip.dataset.tag) return;
    state.tags = state.tags.filter((tag) => tag !== chip.dataset.tag);
    renderTags();
  });

  el.notify.addEventListener("change", () => {
    storageSet({ aikdNotify: el.notify.checked });
  });
}

/* ------------------------------ tab capture ------------------------------ */

/**
 * Injected into the active page by chrome.scripting.executeScript.
 * Must stay fully self-contained (it is serialized, not closed over).
 *
 * Priority: live selection -> AI chat transcript -> article/main text -> body text.
 * Returns `{ mode, text, title, url, description }` so the popup can always
 * compose a non-empty BODY even on protected pages.
 */
function extractPageForAikd() {
  const LIMIT = 12000;
  const clean = (value) =>
    String(value || "")
      .replace(/\u00a0/g, " ")
      .replace(/[ \t]+\n/g, "\n")
      .replace(/\n{3,}/g, "\n\n")
      .trim();
  const meta = (selector) => {
    const node = document.querySelector(selector);
    return node && node.content ? clean(node.content) : "";
  };

  const url = String(location.href || "");
  const title = String(document.title || location.hostname || "");
  const description =
    meta('meta[name="description"]') || meta('meta[property="og:description"]');

  let selection = "";
  try {
    selection = clean(window.getSelection ? window.getSelection().toString() : "");
  } catch (_) {
    selection = "";
  }
  if (selection) {
    return {
      mode: "selection",
      text: selection.slice(0, LIMIT),
      title,
      url,
      description,
    };
  }

  const chatSelector = [
    "[data-message-author-role]",
    "[data-testid='user-message']",
    "[data-testid='assistant-message']",
    ".font-claude-message",
    "user-query",
    "model-response",
    "[data-testid^='conversation-turn-']",
  ].join(",");

  const roleOf = (node) => {
    const attr = node.getAttribute ? node.getAttribute("data-message-author-role") : "";
    if (attr) {
      const role = String(attr).toLowerCase();
      if (role === "user" || role === "human") return "You";
      if (role === "assistant" || role === "ai" || role === "model") return "Assistant";
      return attr;
    }
    const testid = node.getAttribute ? node.getAttribute("data-testid") : "";
    if (testid === "user-message") return "You";
    if (testid === "assistant-message") return "Assistant";
    const tag = node.tagName ? node.tagName.toLowerCase() : "";
    if (tag === "user-query") return "You";
    if (tag === "model-response") return "Assistant";
    if (node.classList && node.classList.contains("font-claude-message")) return "Assistant";
    return "";
  };

  let messages = [];
  try {
    const candidates = Array.prototype.slice
      .call(document.querySelectorAll(chatSelector), 0, 250)
      .filter((node) => clean(node.innerText).length >= 2);
    // Keep the innermost nodes so wrappers do not duplicate whole turns.
    const picked = candidates.filter(
      (node) => !candidates.some((other) => other !== node && node.contains(other))
    );
    const seen = Object.create(null);
    for (const node of picked) {
      const text = clean(node.innerText);
      if (!text) continue;
      const key = text.slice(0, 180);
      if (seen[key]) continue;
      seen[key] = true;
      messages.push({ role: roleOf(node), text: text.slice(0, 4000) });
      if (messages.length >= 80) break;
    }
  } catch (_) {
    messages = [];
  }

  if (messages.length) {
    const transcript = messages
      .slice(-50)
      .map((message) => `${message.role || "Message"}:\n${message.text}`)
      .join("\n\n");
    const text = transcript.length > LIMIT ? transcript.slice(-LIMIT) : transcript;
    return { mode: "chat", text, title, url, description };
  }

  const articleSelectors = [
    "article",
    "main",
    "[role='main']",
    ".markdown-body",
    ".post-content",
    ".entry-content",
    ".article-body",
    ".article-content",
    "#content",
    ".content",
  ];
  let articleText = "";
  for (const selector of articleSelectors) {
    let best = "";
    try {
      document.querySelectorAll(selector).forEach((node) => {
        const text = clean(node.innerText);
        if (text.length > best.length) best = text;
      });
    } catch (_) {
      best = "";
    }
    if (best.length >= 160) {
      articleText = best;
      break;
    }
    if (best.length > articleText.length) articleText = best;
  }

  if (articleText.length >= 80) {
    return {
      mode: "article",
      text: articleText.slice(0, LIMIT),
      title,
      url,
      description,
    };
  }

  let bodyText = "";
  try {
    bodyText = clean(document.body ? document.body.innerText : "");
  } catch (_) {
    bodyText = "";
  }
  if (bodyText) {
    return {
      mode: "page",
      text: bodyText.slice(0, LIMIT),
      title,
      url,
      description,
    };
  }

  return { mode: "none", text: "", title, url, description };
}

async function fillFromTab() {
  let tab = null;
  try {
    const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
    tab = tabs && tabs[0] ? tabs[0] : null;
  } catch (_) {
    tab = null;
  }
  if (!tab) {
    el.sourceHost.textContent = "no active tab";
    return;
  }

  state.tab = tab;
  const url = tab.url || "";
  el.sourceHost.textContent = url || "active tab";
  el.sourceHost.title = url;
  el.selectionHint.hidden = false;
  el.selectionHint.textContent = "reading page…";

  let extraction = null;
  if (typeof tab.id === "number") {
    try {
      const results = await chrome.scripting.executeScript({
        target: { tabId: tab.id },
        func: extractPageForAikd,
      });
      extraction = results && results[0] ? results[0].result : null;
    } catch (_) {
      /* chrome://, Web Store, PDF viewer and other protected pages */
      extraction = null;
    }
  }

  const selection =
    extraction && extraction.mode === "selection" ? String(extraction.text || "").trim() : "";

  if (!el.title.value.trim()) {
    el.title.value = String((extraction && extraction.title) || tab.title || "")
      .replace(/\s+/g, " ")
      .trim();
  }

  if (!el.body.value.trim()) {
    let body = "";
    let hint = "";
    if (selection) {
      body = selection.slice(0, EXTRACT_LIMIT);
      hint = `${body.length.toLocaleString()} chars selected`;
    } else if (extraction && extraction.text) {
      body = composeAutoBody(extraction, url);
      hint = `auto · ${body.length.toLocaleString()} chars · ${modeLabel(extraction.mode)}`;
      suggestDomainTag(extraction.url || url, extraction.mode);
    } else {
      body = fallbackBody(tab, extraction);
      hint = "auto · readable text unavailable — placeholder added";
      suggestDomainTag(url, "page");
    }
    el.body.value = body;
    el.selectionHint.hidden = false;
    el.selectionHint.textContent = hint;
    updateCounter();
    scheduleSecretScan();
  } else {
    el.selectionHint.hidden = true;
  }

  if (!el.title.value.trim()) {
    el.title.value = deriveTitle(el.body.value);
  }
}

function composeAutoBody(extraction, url) {
  const source = String(extraction.url || url || "").trim();
  const description = String(extraction.description || "").trim();
  const text = String(extraction.text || "").trim();
  const parts = [];
  if (source) parts.push(`> Source: ${source}`);
  if (description) parts.push(`> ${truncate(description, 300)}`);
  if (text) parts.push(text);
  return parts.join("\n\n").slice(0, EXTRACT_LIMIT).trim();
}

function fallbackBody(tab, extraction) {
  const url = String((extraction && extraction.url) || (tab && tab.url) || "");
  const title = String(
    (extraction && extraction.title) || (tab && tab.title) || hostOf(url) || "Captured page"
  );
  const description = extraction && extraction.description ? truncate(extraction.description, 300) : "";
  const parts = [`# ${truncate(title, 140) || "Captured page"}`];
  if (url) parts.push(`> Source: ${url}`);
  if (description) parts.push(`> ${description}`);
  parts.push("");
  parts.push("_Page text could not be read automatically. Add your notes here._");
  return parts.join("\n");
}

function modeLabel(mode) {
  if (mode === "chat") return "chat transcript";
  if (mode === "article") return "article text";
  if (mode === "page") return "page text";
  if (mode === "selection") return "selection";
  return "page";
}

function suggestDomainTag(url, mode) {
  if (state.tags.length) return;
  const tag = domainTagFor(url, mode);
  if (tag) addTag(tag);
}

function domainTagFor(url, mode) {
  const host = hostOf(url).toLowerCase();
  for (const [pattern, tag] of DOMAIN_TAGS) {
    if (pattern.test(host)) return tag;
  }
  return mode === "chat" ? "ai-chat" : "web-clip";
}

function hostOf(url) {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch (_) {
    return "";
  }
}

/* -------------------------------- daemon --------------------------------- */

function normalizePort(value) {
  const port = Number.parseInt(value, 10);
  return Number.isFinite(port) && port >= 1 && port <= 65535 ? port : DEFAULT_PORT;
}

function commitPort() {
  const port = normalizePort(el.port.value);
  el.port.value = String(port);
  if (port === state.port) {
    refreshConnection();
    return;
  }
  state.port = port;
  storageSet({ aikdPort: port });
  refreshConnection();
}

async function refreshConnection() {
  setStatus(`Checking 127.0.0.1:${state.port}…`, "info");
  const online = await checkHealth();
  if (!online) {
    setStatus(`No daemon on 127.0.0.1:${state.port} — run "aikd daemon" or change the port.`, "warn");
    return;
  }
  setStatus("Daemon connected.", "ok");
  await loadProjects();
}

async function checkHealth() {
  if (state.checking) return state.online;
  state.checking = true;
  const started = performance.now();
  const res = await api("/api/health", { timeout: 3500 });
  const elapsed = Math.round(performance.now() - started);
  state.checking = false;

  const wasOnline = state.online;
  state.online = res.ok;
  setConn(res.ok, elapsed);

  if (state.checkedOnce) {
    if (res.ok && !wasOnline) setStatus("Daemon connected.", "ok");
    if (!res.ok && wasOnline) {
      setStatus(`Daemon went offline on port ${state.port}.`, "warn");
    }
  }
  state.checkedOnce = true;
  return res.ok;
}

function setConn(online, elapsed) {
  el.dot.classList.toggle("on", online);
  el.dot.classList.toggle("off", !online);
  el.connText.textContent = online ? `online · ${elapsed}ms` : "offline";
  el.connPill.title = online
    ? `AIKD daemon reachable at 127.0.0.1:${state.port}`
    : `No AIKD daemon at 127.0.0.1:${state.port}`;
}

async function loadProjects() {
  if (!state.online) {
    state.projects = [];
    renderProjects([]);
    el.daemonMeta.textContent = "offline";
    el.daemonMeta.title = "Start the daemon to load projects";
    return;
  }

  const res = await api("/api/overview");
  if (!res.ok) {
    el.daemonMeta.textContent = "overview unavailable";
    el.daemonMeta.title = res.error || "";
    return;
  }

  const data = res.data || {};
  state.projects = Array.isArray(data.projects) ? data.projects.filter((p) => p && p.path) : [];
  renderProjects(state.projects);
  updateMeta(data);
}

function updateMeta(data) {
  const bits = [];
  if (data.version) {
    const version = String(data.version);
    bits.push(version.startsWith("v") ? version : `v${version}`);
  }
  const notes = Number(data.notes_count);
  if (Number.isFinite(notes)) bits.push(`${notes} notes`);
  const clients = Number(data.active_clients_count);
  if (Number.isFinite(clients) && clients > 0) bits.push(`${clients} clients`);
  el.daemonMeta.textContent = bits.length ? bits.join(" · ") : "daemon online";
  el.daemonMeta.title = [
    data.schema_version !== undefined ? `schema v${data.schema_version}` : "",
    Number.isFinite(Number(data.files)) ? `${data.files} files` : "",
    Number.isFinite(Number(data.chunks)) ? `${data.chunks} chunks` : "",
  ]
    .filter(Boolean)
    .join(" · ");
}

function renderProjects(projects) {
  const paths = [];
  el.project.textContent = "";
  el.project.add(new Option("Global — no project", ""));

  for (const project of projects) {
    const path = project.path;
    if (!path || paths.includes(path)) continue;
    const name = project.name || basename(path);
    const files = Number(project.files);
    const option = new Option(`${name}${Number.isFinite(files) && files > 0 ? ` · ${files} files` : ""}`, path);
    option.title = `${path}${project.git_branch ? ` · ${project.git_branch}` : ""}`;
    el.project.add(option);
    paths.push(path);
  }

  if (state.project && !paths.includes(state.project)) {
    const saved = new Option(`${basename(state.project)} · saved`, state.project);
    saved.title = state.project;
    el.project.add(saved);
    paths.push(state.project);
  }

  if (!state.project && projects.length === 1 && projects[0].path) {
    state.project = projects[0].path;
  }

  el.project.value = state.project && paths.includes(state.project) ? state.project : "";
  if (!el.project.value) state.project = "";
  el.project.disabled = el.scope.value === "global";
}

/* -------------------------------- sending -------------------------------- */

async function onSend() {
  if (state.busy) return;

  const body = el.body.value.trim();
  const title = el.title.value.trim();
  if (!body && !title) {
    setStatus("Add a title or some body text first.", "warn");
    el.body.focus();
    return;
  }

  const scope = el.scope.value;
  // Keep #tags the user typed but never sent them twice.
  addTag(el.tagInput.value);
  el.tagInput.value = "";
  const project = scope === "project" ? state.project : "";

  const payload = {
    title: title || deriveTitle(body),
    body: body || title,
    kind: el.kind.value,
    scope,
    folder: el.folder.value,
    project,
    tags: mergeTags(state.tags, extractTags(body)),
    source: "chrome-extension",
    source_url: state.tab ? state.tab.url || "" : "",
    pinned: false,
  };

  setBusy(true);
  setStatus("Sending to AIKD…", "info");
  try {
    const res = await api("/api/send", { method: "POST", body: payload });
    if (!res.ok) {
      setStatus(res.error || "Send failed.", "err");
      renderShield("live", countSecrets(body));
      return;
    }

    const data = res.data || {};
    const redacted = Number(data.redacted_count) || 0;
    renderShield("saved", redacted);

    const details = [];
    if (data.folder) details.push(`filed in ${data.folder}`);
    if (Array.isArray(data.extracted_tags) && data.extracted_tags.length) {
      details.push(`tags ${data.extracted_tags.map((tag) => `#${tag}`).join(" ")}`);
    }
    setStatus(`Saved${details.length ? ` · ${details.join(" · ")}` : ""}.`, "ok");

    el.body.value = "";
    state.tags = [];
    renderTags();
    updateCounter();
    el.selectionHint.hidden = true;
  } catch (error) {
    setStatus(error && error.message ? error.message : String(error), "err");
  } finally {
    setBusy(false);
  }
}

function setBusy(busy) {
  state.busy = busy;
  el.send.disabled = busy;
  el.send.textContent = busy ? "Sending…" : "Send to AIKD";
}

function deriveTitle(body) {
  const line =
    String(body || "")
      .split(/\r?\n/)
      .map((part) => part.trim())
      .find(Boolean) || "";
  const cleaned = line.replace(/^[#>*\-\s]+/, "");
  return truncate(cleaned, 90) || "Quick capture";
}

/* ------------------------------ context pack ----------------------------- */

async function onCopyContextPack() {
  const project = el.scope.value === "project" ? state.project : "";
  const query = project ? `?project=${encodeURIComponent(project)}` : "";
  setStatus("Building context pack…", "info");
  const res = await api(`/api/context-pack${query}`);
  if (!res.ok) {
    if (res.status === 404) {
      setStatus("Context pack needs AIKD v2.1+ (endpoint not found).", "warn");
    } else {
      setStatus(res.error || "Could not build a context pack.", "err");
    }
    return;
  }

  const data = res.data || {};
  const markdown =
    typeof data.markdown === "string"
      ? data.markdown
      : typeof data === "string"
        ? data
        : data.markdown === undefined
          ? ""
          : String(data.markdown);
  if (!markdown.trim()) {
    setStatus("Context pack was empty — index a project first.", "warn");
    return;
  }

  const copied = await copyText(markdown);
  const tokens = Number(data.tokens_est) || Math.ceil(markdown.length / 4);
  const items = Number(data.items_count) || 0;
  if (copied) {
    setStatus(`Copied context pack · ~${tokens.toLocaleString()} tokens${items ? ` · ${items} items` : ""}.`, "ok");
  } else {
    setStatus("Clipboard blocked — open Studio to copy the pack.", "warn");
  }
}

function onOpenStudio() {
  const url = `http://127.0.0.1:${state.port}/ui`;
  try {
    chrome.tabs.create({ url });
  } catch (_) {
    window.open(url, "_blank");
  }
}

async function copyText(text) {
  // In an unfocused popup (or headless), clipboard write can stay pending forever.
  // Bound it, then fall back to the legacy execCommand path.
  try {
    if (await withTimeout(navigator.clipboard.writeText(text), 1500)) return true;
  } catch (_) {
    /* fall through to legacy path */
  }
  try {
    const area = document.createElement("textarea");
    area.value = text;
    area.style.position = "fixed";
    area.style.opacity = "0";
    document.body.appendChild(area);
    area.select();
    const ok = document.execCommand("copy");
    area.remove();
    return ok;
  } catch (_) {
    return false;
  }
}

function withTimeout(promise, ms) {
  return new Promise((resolve) => {
    let settled = false;
    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      resolve(false);
    }, ms);
    Promise.resolve(promise).then(
      () => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        resolve(true);
      },
      () => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        resolve(false);
      }
    );
  });
}

/* --------------------------------- shield -------------------------------- */

function scheduleSecretScan() {
  clearTimeout(state.scanTimer);
  state.scanTimer = setTimeout(() => {
    renderShield("live", countSecrets(el.body.value));
  }, 320);
}

function countSecrets(text) {
  if (!text) return 0;
  SECRET_RE.lastIndex = 0;
  const matches = String(text).match(SECRET_RE);
  return matches ? matches.length : 0;
}

function renderShield(mode, count) {
  if (mode === "live") {
    if (!count) {
      el.shield.hidden = true;
      return;
    }
    el.shield.hidden = false;
    el.shield.className = "shield warn";
    el.shield.textContent = `Shield armed · ${count} potential secret${count === 1 ? "" : "s"} will be redacted before saving`;
    return;
  }

  el.shield.hidden = false;
  if (count > 0) {
    el.shield.className = "shield warn";
    el.shield.textContent = `Shield: ${count} secret${count === 1 ? "" : "s"} redacted before saving`;
  } else {
    el.shield.className = "shield safe";
    el.shield.textContent = "Shield: clean — no secrets detected";
  }
}

/* ---------------------------------- tags --------------------------------- */

function addTag(raw) {
  const tag = String(raw || "")
    .trim()
    .replace(/^#+/, "")
    .replace(/\s+/g, "-")
    .toLowerCase();
  if (!tag) return;
  if (!/^[\p{L}\p{N}_-]{1,40}$/u.test(tag)) {
    setStatus("Tags allow letters, numbers, dashes and underscores.", "warn");
    return;
  }
  if (state.tags.includes(tag)) return;
  if (state.tags.length >= MAX_TAGS) {
    setStatus(`Tag limit is ${MAX_TAGS}.`, "warn");
    return;
  }
  state.tags.push(tag);
  renderTags();
}

function renderTags() {
  el.tagChips.textContent = "";
  for (const tag of state.tags) {
    const chip = document.createElement("button");
    chip.type = "button";
    chip.className = "chip";
    chip.dataset.tag = tag;
    chip.title = `Remove #${tag}`;
    chip.append(`#${tag}`);

    const remove = document.createElement("span");
    remove.className = "x";
    remove.textContent = "×";
    chip.append(remove);
    el.tagChips.append(chip);
  }
}

function extractTags(text) {
  const found = [];
  if (!text) return found;
  TAG_RE.lastIndex = 0;
  let match;
  while ((match = TAG_RE.exec(String(text))) !== null) {
    const tag = match[1].toLowerCase();
    if (!found.includes(tag)) found.push(tag);
  }
  return found;
}

function mergeTags(...lists) {
  const merged = [];
  for (const list of lists) {
    for (const tag of list) {
      const normalized = String(tag || "").toLowerCase();
      if (normalized && !merged.includes(normalized)) merged.push(normalized);
    }
  }
  return merged.slice(0, MAX_TAGS);
}

/* --------------------------------- helpers ------------------------------- */

async function api(path, { method = "GET", body, timeout = 9000 } = {}) {
  let lastError = null;

  for (const host of HOSTS) {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeout);
    try {
      const response = await fetch(`http://${host}:${state.port}${path}`, {
        method,
        headers: body === undefined ? undefined : { "Content-Type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
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
          data: payload.data,
        };
      }
      if (!response.ok) {
        return {
          ok: false,
          status: response.status,
          error: (payload && payload.error) || `HTTP ${response.status}`,
          data: payload && payload.data,
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
    error: aborted ? "AIKD daemon timed out" : `AIKD daemon not reachable on port ${state.port}`,
  };
}

function setStatus(message, tone) {
  el.status.textContent = message;
  el.status.className = tone ? `status ${tone}` : "status";
}

function updateCounter() {
  el.charCount.textContent = `${el.body.value.length.toLocaleString()} chars`;
}

function truncate(value, max) {
  const text = String(value || "").replace(/\s+/g, " ").trim();
  return text.length > max ? `${text.slice(0, max).trimEnd()}…` : text;
}

function basename(path) {
  const parts = String(path || "").split(/[\\/]/).filter(Boolean);
  return parts.length ? parts[parts.length - 1] : "project";
}

function pickOption(select, value, fallback) {
  const wanted = typeof value === "string" ? value : "";
  const options = Array.from(select.options || []);
  return options.some((option) => option.value === wanted) ? wanted : fallback;
}

function persistCapturePrefs() {
  state.project = el.project.value;
  storageSet({
    aikdKind: el.kind.value,
    aikdScope: el.scope.value,
    aikdFolder: el.folder.value,
    aikdProject: el.project.value,
  });
}

function storageGet(defaults) {
  return new Promise((resolve) => {
    chrome.storage.local.get(defaults, (items) => resolve(items || defaults));
  });
}

function storageSet(values) {
  return new Promise((resolve) => {
    chrome.storage.local.set(values, () => resolve());
  });
}
