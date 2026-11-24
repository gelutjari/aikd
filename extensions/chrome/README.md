# Send to AIKD — Chrome / Edge (Manifest V3)

1. Open `chrome://extensions` (or `edge://extensions`), toggle **Developer mode**, click **Load unpacked**, and pick this `extensions/chrome/` folder.
2. Start the daemon (`aikd daemon`) and keep the default port `9090`; use the popup's **Port** field for sandbox `9091` (persisted in `chrome.storage.local`).
3. Select text → right-click **Send selection to AIKD**, or right-click anywhere → **Clip page to AIKD**; both POST to `/api/send` with `source: "chrome-extension"`.
4. Press `Ctrl+Shift+Y` (the `_execute_action` command) to open the popup, which auto-fills the tab title, URL and current selection, then send, copy a Context Pack, or open AIKD Studio.
5. Badge/notification feedback shows `OK`/`ERR` plus the daemon's Shield count — secrets are redacted locally before anything is written to your vault.
