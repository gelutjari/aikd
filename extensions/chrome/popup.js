/**
 * Send to AIKD — popup controller.
 *
 * Talks directly to the local daemon REST API:
 *   GET  /api/health        connection dot + latency
 *   GET  /api/overview      project dropdown, version, notes count
 *   POST /api/send          save captured content
 *   GET  /api/context-pack  copy a dense Markdown pack for web AI chats
 *
 * Settings persist in chrome.storage.local: aikdPort, aikdProject, aikdNotify.
 */
"use strict";

const DEFAULT_PORT = 9090;
const HOSTS = ["127.0.0.1", "localhost"];
const MAX_TAGS = 12;
const TAG_RE = /#([\p{L}\p{N}_-]{2,40})/gu;
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

  const settings = await storageGet({ aikdPort: DEFAULT_PORT, aikdProject: "", aikdNotify: true });
  state.port = normalizePort(settings.aikdPort);
  state.project = typeof settings.aikdProject === "string" ? settings.aikdProject : "";
  el.port.value = String(state.port);
  el.notify.checked = settings.aikdNotify !== false;
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
    if (isGlobal) setStatus("Scope global — project will be ignored.", "info");
  });

  el.project.addEventListener("change", () => {
    state.project = el.project.value;
    storageSet({ aikdProject: state.project });
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

  if (!el.title.value.trim()) {
    el.title.value = String(tab.title || "").replace(/\s+/g, " ").trim();
  }

  if (!tab.id) return;
  try {
    const results = await chrome.scripting.executeScript({
      target: { tabId: tab.id },
      func: () => {
        const selected = window.getSelection ? window.getSelection().toString() : "";
        return selected.slice(0, 8000);
      },
    });
    const selection = results && results[0] && results[0].result ? String(results[0].result).trim() : "";
    if (selection && !el.body.value.trim()) {
      el.body.value = selection;
      el.selectionHint.hidden = false;
      el.selectionHint.textContent = `${selection.length.toLocaleString()} chars selected`;
      updateCounter();
      scheduleSecretScan();
    }
  } catch (_) {
    /* chrome://, Web Store and PDF viewer pages are not scriptable */
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
  state.project = "";
  storageSet({ aikdPort: port, aikdProject: "" });
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
