'use strict';
// iso-scene: живой предпросмотр .scene/.model в боковой панели.
const vscode = require('vscode');
const cp = require('child_process');
const crypto = require('crypto');
const fs = require('fs');
const os = require('os');
const path = require('path');
const figureHelp = require('./figure_help');

let panel;
let current;
let timer;
let version = 0;
let activeTab = 'iso';
let diagnostics;
let status;
let output;

function config() {
    const c = vscode.workspace.getConfiguration('isoScene');
    return {
        executable: c.get('executable') || 'iso-scene',
        delay: Math.max(0, c.get('updateDelay') ?? 150)
    };
}

function cliCandidates() {
    const configured = config().executable;
    const fallback = path.join(os.homedir(), '.cargo', 'bin', 'iso-scene');
    return configured === fallback ? [configured] : [configured, fallback];
}

function runCli(args, cwd) {
    return new Promise((resolve, reject) => {
        const candidates = cliCandidates();
        const attempt = (index) => {
            if (index >= candidates.length) {
                reject(new Error('iso-scene CLI не найден. Установи: cargo install --path crates/scene-cli, ' +
                    'или задай путь в настройке isoScene.executable.'));
                return;
            }
            const child = cp.spawn(candidates[index], args, { cwd, windowsHide: true });
            let stdout = '';
            let stderr = '';
            let started = false;
            child.on('spawn', () => { started = true; });
            child.stdout.on('data', (b) => { stdout += b.toString(); });
            child.stderr.on('data', (b) => { stderr += b.toString(); });
            child.on('error', (error) => {
                if (!started && error.code === 'ENOENT') attempt(index + 1);
                else reject(error);
            });
            child.on('close', (code) => {
                if (!started) return;
                if (code === 0) resolve({ stdout, stderr });
                else reject(new Error((stderr || stdout || `код ${code}`).trim()));
            });
        };
        attempt(0);
    });
}

function liveFile(document) {
    const dir = path.join(os.tmpdir(), 'iso-scene-live');
    fs.mkdirSync(dir, { recursive: true });
    const ext = path.extname(document.uri.fsPath) || '.scene';
    const hash = crypto.createHash('sha1').update(document.uri.fsPath).digest('hex').slice(0, 12);
    const file = path.join(dir, 'preview-' + hash + ext);
    fs.writeFileSync(file, document.getText());
    return file;
}

function imageData(file) {
    return fs.readFileSync(file).toString('base64');
}

function showError(document, message) {
    const text = String(message || '');
    const match = text.match(/:(\d+):(?:\d+:)?\s*(?:error\[[^\]]+\]:\s*)?(.*)/);
    const line = match ? Math.max(0, Math.min(document.lineCount - 1, Number(match[1]) - 1)) : 0;
    const detail = match ? match[2] : text;
    diagnostics.set(document.uri, [
        new vscode.Diagnostic(document.lineAt(line).range, detail, vscode.DiagnosticSeverity.Error)
    ]);
    if (panel) panel.webview.postMessage({ type: 'error', text });
}

async function render(document, tab, token) {
    if (!panel) return;
    const file = document.uri.fsPath;
    const base = path.dirname(file);
    panel.webview.postMessage({ type: 'status', text: 'Обновление…' });
    if (status) status.text = '$(sync~spin) Scene';
    try {
        const source = liveFile(document);
        const dir = path.dirname(source);
        if (tab === 'iso') {
            const out = path.join(dir, 'iso.png');
            await runCli(['render', source, '--base', base, '-o', out], base);
            if (token !== version) return;
            panel.webview.postMessage({ type: 'render', kind: 'iso', image: imageData(out) });
        } else if (tab === 'drawing') {
            const images = [];
            for (const view of ['top', 'front', 'side']) {
                const out = path.join(dir, `view-${view}.png`);
                await runCli(['render', source, '--base', base, '--view', view, '-o', out], base);
                images.push(imageData(out));
            }
            if (token !== version) return;
            panel.webview.postMessage({ type: 'render', kind: 'drawing', images });
        } else {
            const out = path.join(dir, 'model.vox');
            const result = await runCli(['voxel', source, '--base', base, '-o', out], base);
            const match = result.stdout.match(/\((\d+)x(\d+)x(\d+),\s*(\d+) voxels/);
            const info = match
                ? `Сетка ${match[1]} × ${match[2]} × ${match[3]}, вокселей: ${match[4]}.`
                : 'Готово.';
            if (token !== version) return;
            panel.webview.postMessage({ type: 'render', kind: 'voxel', vox: imageData(out), info });
        }
        diagnostics.delete(document.uri);
    } catch (error) {
        showError(document, error.message || error);
    } finally {
        if (status) status.text = '$(eye) Scene';
    }
}

function schedule(document) {
    clearTimeout(timer);
    timer = setTimeout(() => {
        if (!panel || !document || document.isClosed) return;
        version += 1;
        render(document, activeTab, version);
    }, config().delay);
}

function html(nonce, uri, voxUri) {
    return `<!doctype html><html lang="ru"><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data:; style-src 'unsafe-inline'; script-src 'nonce-${nonce}';">
<style>
body{margin:0;background:var(--vscode-editor-background);color:var(--vscode-foreground);font-family:var(--vscode-font-family)}
nav{display:flex;gap:4px;padding:8px;position:sticky;top:0;background:var(--vscode-editor-background)}
button{background:var(--vscode-button-secondaryBackground);color:var(--vscode-button-secondaryForeground);border:0;padding:7px 10px;border-radius:6px;cursor:pointer}
button.active{background:var(--vscode-button-background);color:var(--vscode-button-foreground)}
#info{padding:8px;white-space:pre-wrap;color:#9aa0a8}
img{max-width:100%;image-rendering:pixelated;display:none}
#image{display:block}
#views{display:none;grid-template-columns:repeat(3,1fr);gap:8px;padding:8px;border-radius:8px;
  background-color:#202226;
  background-image:linear-gradient(rgba(255,255,255,.06) 1px,transparent 1px),
                   linear-gradient(90deg,rgba(255,255,255,.06) 1px,transparent 1px);
  background-size:32px 32px}
#views figure{margin:0;padding:8px}
#views figcaption{text-align:center;color:#9aa0a8;font-size:12px;margin-top:4px}
canvas{width:100%;height:70vh;touch-action:none;display:none;background:#202226;border-radius:8px}
#hint{padding:8px;color:#9aa0a8}
</style></head><body>
<nav>
  <button data-tab="iso" class="active">Изометрия</button>
  <button data-tab="drawing">Чертёж</button>
  <button data-tab="voxel">Воксели</button>
</nav>
<div id="info">Загрузка…</div>
<img id="image" alt="Предпросмотр">
<div id="views">
  <figure><img alt="top"><figcaption>сверху (XY)</figcaption></figure>
  <figure><img alt="front"><figcaption>спереди (XZ)</figcaption></figure>
  <figure><img alt="side"><figcaption>сбоку (YZ)</figcaption></figure>
</div>
<canvas id="canvas"></canvas>
<div id="hint"></div>
<script nonce="${nonce}" src="${voxUri}"></script>
<script nonce="${nonce}" src="${uri}"></script>
</body></html>`;
}

function start(document) {
    if (!vscode.workspace.isTrusted) {
        vscode.window.showWarningMessage('Для запуска iso-scene нужно доверенное рабочее пространство.');
        return;
    }
    current = document;
    if (!panel) {
        activeTab = 'iso';
        panel = vscode.window.createWebviewPanel('isoScenePreview', 'Scene: предпросмотр',
            vscode.ViewColumn.Beside, {
                enableScripts: true,
                localResourceRoots: [vscode.Uri.file(path.join(__dirname, 'media'))],
                retainContextWhenHidden: true
            });
        const nonce = crypto.randomBytes(16).toString('hex');
        const uri = panel.webview.asWebviewUri(vscode.Uri.file(path.join(__dirname, 'media', 'viewer.js')));
        const voxUri = panel.webview.asWebviewUri(vscode.Uri.file(path.join(__dirname, 'media', 'vox.js')));
        panel.webview.html = html(nonce, uri, voxUri);
        panel.onDidDispose(() => {
            panel = undefined;
            current = undefined;
            clearTimeout(timer);
        });
        panel.webview.onDidReceiveMessage((m) => {
            if (m.type === 'ready') schedule(current);
            if (m.type === 'tab' && ['iso', 'drawing', 'voxel'].includes(m.tab)) {
                activeTab = m.tab;
                schedule(current);
            }
        });
    } else {
        panel.reveal(vscode.ViewColumn.Beside, true);
        schedule(document);
    }
}

async function build(document) {
    const file = document.uri.fsPath;
    const base = path.dirname(file);
    const stem = path.basename(file, path.extname(file));
    const outDir = path.join(base, 'out');
    fs.mkdirSync(outDir, { recursive: true });
    output.show(true);
    output.appendLine('Сборка: ' + file);
    const source = liveFile(document);
    try {
        const png = path.join(outDir, `${stem}.png`);
        await runCli(['render', source, '--base', base, '-o', png], base);
        output.appendLine(png);
        const vox = path.join(outDir, `${stem}.vox`);
        await runCli(['voxel', source, '--base', base, '-o', vox], base);
        output.appendLine(vox);
        if (path.extname(file).toLowerCase() === '.scene') {
            const tiled = path.join(outDir, `${stem}-tiled`);
            await runCli(['tiled', source, '--base', base, '--tile-layer', '-o', tiled], base);
            output.appendLine(path.join(tiled, 'scene.tmj'));
        }
        diagnostics.delete(document.uri);
        vscode.window.showInformationMessage('iso-scene: готово, см. ' + outDir);
    } catch (error) {
        showError(document, error.message || error);
        output.appendLine(String(error.message || error));
        vscode.window.showErrorMessage('Ошибка сборки iso-scene. Подробности в выводе Iso Scene.');
    }
}

function activate(context) {
    diagnostics = vscode.languages.createDiagnosticCollection('iso-scene');
    output = vscode.window.createOutputChannel('Iso Scene');
    status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 50);
    status.text = '$(eye) Scene';
    status.command = 'isoScene.preview';

    const selector = { language: 'iso-scene', scheme: 'file' };

    const hover = vscode.languages.registerHoverProvider(selector, {
        provideHover(document, position) {
            const line = document.lineAt(position.line).text;
            const match = /^\s*(box|cyl|sphere|slantbox|slantcyl|ramp|object|use|merge|place)\b/i.exec(line);
            if (!match) return;
            let key = match[1].toLowerCase();
            if (key === 'box' && /^\s*box\s+from\b/i.test(line)) key = 'box from';
            const start = match.index + match[0].search(/\S/);
            const end = start + match[1].length;
            if (position.character < start || position.character >= end) return;
            const entry = figureHelp[key];
            if (!entry) return;
            const md = new vscode.MarkdownString();
            md.appendCodeblock(entry.signature, 'text');
            md.appendMarkdown('\n' + entry.description);
            return new vscode.Hover(md, new vscode.Range(position.line, start, position.line, end));
        }
    });

    const completion = vscode.languages.registerCompletionItemProvider(selector, {
        provideCompletionItems(document, position) {
            const before = document.lineAt(position.line).text.slice(0, position.character);
            if (!/^\s*[a-z]*$/i.test(before)) return;
            return Object.entries(figureHelp).map(([name, entry]) => {
                const item = new vscode.CompletionItem(name, vscode.CompletionItemKind.Snippet);
                item.detail = entry.signature;
                item.documentation = new vscode.MarkdownString(entry.description);
                item.insertText = new vscode.SnippetString(entry.snippet);
                return item;
            });
        }
    });

    context.subscriptions.push(
        hover, completion, diagnostics, status, output,
        vscode.commands.registerCommand('isoScene.preview', () => {
            const doc = vscode.window.activeTextEditor?.document || current;
            if (doc?.languageId === 'iso-scene') start(doc);
        }),
        vscode.commands.registerCommand('isoScene.build', async () => {
            const doc = vscode.window.activeTextEditor?.document || current;
            if (!doc || doc.languageId !== 'iso-scene') return;
            if (!vscode.workspace.isTrusted) {
                vscode.window.showWarningMessage('Для сборки нужно доверенное рабочее пространство.');
                return;
            }
            if (doc.isDirty && !await doc.save()) return;
            build(doc);
        }),
        vscode.workspace.onDidChangeTextDocument((e) => {
            if (current && e.document === current && panel) schedule(current);
        }),
        vscode.workspace.onDidChangeConfiguration((e) => {
            if (e.affectsConfiguration('isoScene') && panel) schedule(current);
        }),
        vscode.window.onDidChangeActiveTextEditor((e) => {
            if (e?.document.languageId === 'iso-scene') {
                status.show();
                if (panel && e.document !== current) {
                    current = e.document;
                    schedule(current);
                }
            } else {
                status.hide();
            }
        })
    );
    if (vscode.window.activeTextEditor?.document.languageId === 'iso-scene') status.show();
}

function deactivate() {
    clearTimeout(timer);
}

module.exports = { activate, deactivate };
