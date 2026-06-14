const vscode = require("vscode");
const http = require("http");

let statusBarItem;
let daemonProcess;

function getBaseUrl() {
    const cfg = vscode.workspace.getConfiguration("aikd");
    return `http://127.0.0.1:${cfg.get("restPort", 9090)}`;
}

function getHeaders() {
    const cfg = vscode.workspace.getConfiguration("aikd");
    const token = cfg.get("authToken", "");
    const headers = { "Content-Type": "application/json" };
    if (token) headers["Authorization"] = `Bearer ${token}`;
    return headers;
}

function apiRequest(method, path, body) {
    return new Promise((resolve, reject) => {
        const url = new URL(path, getBaseUrl());
        const options = {
            hostname: url.hostname,
            port: url.port,
            path: url.pathname + url.search,
            method,
            headers: getHeaders(),
        };

        const req = http.request(options, (res) => {
            let data = "";
            res.on("data", (chunk) => (data += chunk));
            res.on("end", () => {
                try {
                    resolve(JSON.parse(data));
                } catch {
                    resolve(data);
                }
            });
        });

        req.on("error", reject);
        req.setTimeout(5000, () => {
            req.destroy();
            reject(new Error("Request timeout"));
        });

        if (body) req.write(JSON.stringify(body));
        req.end();
    });
}

async function ensureDaemon() {
    try {
        await apiRequest("GET", "/api/stats");
        return true;
    } catch {
        const { spawn } = require("child_process");
        daemonProcess = spawn("aikd", ["daemon", "--foreground"], {
            stdio: "ignore",
            detached: true,
        });
        daemonProcess.unref();
        await new Promise((r) => setTimeout(r, 2000));
        try {
            await apiRequest("GET", "/api/stats");
            return true;
        } catch {
            return false;
        }
    }
}

function activate(context) {
    statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
    statusBarItem.text = "$(database) AIKD";
    statusBarItem.tooltip = "AIKD Knowledge Daemon";
    statusBarItem.show();
    context.subscriptions.push(statusBarItem);

    ensureDaemon().then((ok) => {
        statusBarItem.text = ok ? "$(database) AIKD" : "$(warning) AIKD";
    });

    context.subscriptions.push(
        vscode.commands.registerCommand("aikd.search", async () => {
            const query = await vscode.window.showInputBox({ prompt: "Search knowledge base" });
            if (!query) return;

            try {
                const result = await apiRequest("GET", `/api/query?q=${encodeURIComponent(query)}&limit=10`);
                if (result.success && result.data && result.data.length > 0) {
                    const items = result.data.map((r) => ({
                        label: r.file_path,
                        description: r.heading_text,
                        detail: r.content.substring(0, 200),
                    }));
                    const selected = await vscode.window.showQuickPick(items, { placeHolder: "Search results" });
                    if (selected) {
                        const doc = await vscode.workspace.openTextDocument(selected.label);
                        vscode.window.showTextDocument(doc);
                    }
                } else {
                    vscode.window.showInformationMessage("No results found");
                }
            } catch (e) {
                vscode.window.showErrorMessage(`AIKD search failed: ${e.message}`);
            }
        })
    );

    context.subscriptions.push(
        vscode.commands.registerCommand("aikd.scan", async () => {
            try {
                const result = await apiRequest("POST", "/api/scan", {});
                vscode.window.showInformationMessage(`AIKD: ${result.data || "Scan complete"}`);
            } catch (e) {
                vscode.window.showErrorMessage(`AIKD scan failed: ${e.message}`);
            }
        })
    );

    context.subscriptions.push(
        vscode.commands.registerCommand("aikd.stats", async () => {
            try {
                const result = await apiRequest("GET", "/api/stats");
                if (result.success) {
                    const d = result.data;
                    vscode.window.showInformationMessage(
                        `AIKD: ${d.files} files, ${d.chunks} chunks, ${d.embeddings} embeddings, ${d.sessions} sessions`
                    );
                }
            } catch (e) {
                vscode.window.showErrorMessage(`AIKD stats failed: ${e.message}`);
            }
        })
    );

    context.subscriptions.push(
        vscode.commands.registerCommand("aikd.status", async () => {
            try {
                const result = await apiRequest("GET", "/api/stats");
                if (result.success) {
                    vscode.window.showInformationMessage(`AIKD v${result.data.version} — OK`);
                }
            } catch {
                vscode.window.showWarningMessage("AIKD daemon not running");
            }
        })
    );
}

function deactivate() {
    if (daemonProcess) {
        daemonProcess.kill();
    }
}

module.exports = { activate, deactivate };
