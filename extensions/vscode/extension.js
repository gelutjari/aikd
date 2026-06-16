const vscode = require("vscode");
const http = require("http");

let statusBarItem;

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
        req.setTimeout(10000, () => {
            req.destroy();
            reject(new Error("Request timeout"));
        });

        if (body) req.write(JSON.stringify(body));
        req.end();
    });
}

async function checkDaemon() {
    try {
        await apiRequest("GET", "/api/health");
        return true;
    } catch {
        return false;
    }
}

function activate(context) {
    statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
    statusBarItem.text = "$(database) AIKD";
    statusBarItem.tooltip = "AIKD Knowledge Daemon";
    statusBarItem.show();
    context.subscriptions.push(statusBarItem);

    checkDaemon().then((ok) => {
        statusBarItem.text = ok ? "$(database) AIKD" : "$(warning) AIKD";
        statusBarItem.tooltip = ok ? "AIKD daemon running" : "AIKD daemon not running";
    });

    // Search knowledge base
    context.subscriptions.push(
        vscode.commands.registerCommand("aikd.search", async () => {
            const query = await vscode.window.showInputBox({ prompt: "Search knowledge base" });
            if (!query) return;

            try {
                const result = await apiRequest("GET", `/api/query?q=${encodeURIComponent(query)}&limit=10`);
                if (result.success && result.data && result.data.length > 0) {
                    const items = result.data.map((r) => ({
                        label: r.file_path,
                        description: r.heading_text || "",
                        detail: (r.content || "").substring(0, 200).replace(/\n/g, " "),
                        line: r.line_start || 1,
                    }));
                    const selected = await vscode.window.showQuickPick(items, {
                        placeHolder: `${result.data.length} results for "${query}"`,
                    });
                    if (selected) {
                        const doc = await vscode.workspace.openTextDocument(selected.label);
                        const editor = await vscode.window.showTextDocument(doc);
                        const line = Math.max(0, (selected.line || 1) - 1);
                        const range = new vscode.Range(line, 0, line, 0);
                        editor.revealRange(range, vscode.TextEditorRevealType.InCenter);
                        editor.selection = new vscode.Selection(range.start, range.start);
                    }
                } else {
                    vscode.window.showInformationMessage(`No results for "${query}"`);
                }
            } catch (e) {
                vscode.window.showErrorMessage(`AIKD search failed: ${e.message}`);
            }
        })
    );

    // Scan & index files
    context.subscriptions.push(
        vscode.commands.registerCommand("aikd.scan", async () => {
            try {
                vscode.window.showInformationMessage("AIKD: Scanning...");
                const result = await apiRequest("POST", "/api/scan", {});
                vscode.window.showInformationMessage(`AIKD: ${result.data || "Scan complete"}`);
            } catch (e) {
                vscode.window.showErrorMessage(`AIKD scan failed: ${e.message}`);
            }
        })
    );

    // Show statistics
    context.subscriptions.push(
        vscode.commands.registerCommand("aikd.stats", async () => {
            try {
                const result = await apiRequest("GET", "/api/stats");
                if (result.success) {
                    const d = result.data;
                    const output = vscode.window.createOutputChannel("AIKD Stats");
                    output.clear();
                    output.appendLine(`AIKD v${d.version}`);
                    output.appendLine(`Files:         ${d.files}`);
                    output.appendLine(`Chunks:        ${d.chunks}`);
                    output.appendLine(`Embeddings:    ${d.embeddings} (${d.dimensions}d)`);
                    output.appendLine(`Sessions:      ${d.sessions}`);
                    output.appendLine(`Conversations: ${d.conversations}`);
                    output.show();
                }
            } catch (e) {
                vscode.window.showErrorMessage(`AIKD stats failed: ${e.message}`);
            }
        })
    );

    // Show status
    context.subscriptions.push(
        vscode.commands.registerCommand("aikd.status", async () => {
            try {
                const result = await apiRequest("GET", "/api/status");
                if (result.success) {
                    const d = result.data;
                    statusBarItem.text = "$(database) AIKD";
                    statusBarItem.tooltip = `AIKD daemon running\nPort: ${d.rest_port}\nCPU: ${d.cpu_cores} cores\nRAM: ${d.ram_gb.toFixed(1)} GB\nEmbedding: ${d.embedding_enabled ? "ON" : "OFF"}`;
                    vscode.window.showInformationMessage(
                        `AIKD: ${d.cpu_cores} cores, ${d.ram_gb.toFixed(1)} GB RAM, embedding ${d.embedding_enabled ? "ON" : "OFF"}`
                    );
                }
            } catch {
                statusBarItem.text = "$(warning) AIKD";
                statusBarItem.tooltip = "AIKD daemon not running";
                vscode.window.showWarningMessage("AIKD daemon not running. Start with: aikd daemon");
            }
        })
    );
}

function deactivate() {}

module.exports = { activate, deactivate };
