const vscode = require('vscode');
const { spawn } = require('child_process');
const crypto = require('crypto');
const fs = require('fs');
const os = require('os');
const path = require('path');

let panel = null;
let panelFile = null;

function cliCandidates() {
    const configured = vscode.workspace.getConfiguration('isoScene').get('executable') || 'iso-scene';
    const fallback = path.join(os.homedir(), '.cargo', 'bin', 'iso-scene');
    return configured === fallback ? [configured] : [configured, fallback];
}

function runCli(args, cwd) {
    return new Promise((resolve, reject) => {
        const candidates = cliCandidates();
        const attempt = (index) => {
            if (index >= candidates.length) {
                reject(new Error(
                    'iso-scene CLI not found. Install it with `cargo install --path crates/scene-cli` ' +
                    'or set `isoScene.executable`.'
                ));
                return;
            }
            const child = spawn(candidates[index], args, { cwd, windowsHide: true });
            let stdout = '';
            let stderr = '';
            let started = false;
            child.on('spawn', () => { started = true; });
            child.stdout.on('data', (chunk) => { stdout += chunk.toString(); });
            child.stderr.on('data', (chunk) => { stderr += chunk.toString(); });
            child.on('error', (error) => {
                if (!started && error.code === 'ENOENT') {
                    attempt(index + 1);
                } else {
                    reject(error);
                }
            });
            child.on('close', (code) => {
                if (!started) {
                    return;
                }
                if (code === 0) {
                    resolve({ stdout, stderr });
                } else {
                    reject(new Error((stderr || stdout || `exit code ${code}`).trim()));
                }
            });
        };
        attempt(0);
    });
}

function previewPath(file) {
    const dir = path.join(os.tmpdir(), 'iso-scene-preview');
    fs.mkdirSync(dir, { recursive: true });
    const hash = crypto.createHash('sha1').update(file).digest('hex').slice(0, 16);
    return path.join(dir, `${hash}.png`);
}

function html(imageUri, file, details) {
    return `<!doctype html>
<html lang="ru">
<head>
<meta charset="utf-8">
<style>
  body { margin: 0; padding: 16px; background: #1a1c22; color: #d7d9dd; font-family: var(--vscode-font-family); }
  .frame { display: flex; justify-content: center; padding: 12px; background: #202226; border-radius: 8px; }
  img { max-width: 100%; max-height: calc(100vh - 120px); image-rendering: pixelated; }
  .caption { margin-top: 10px; font-size: 12px; color: #9aa0a8; }
  pre { white-space: pre-wrap; color: #ff9a9a; }
</style>
</head>
<body>
  <div class="frame"><img src="${imageUri}" alt="iso-scene preview"></div>
  <div class="caption">${file} — сохрани файл, чтобы обновить предпросмотр.</div>
  ${details ? `<pre>${details}</pre>` : ''}
</body>
</html>`;
}

async function renderDocument(document) {
    const file = document.uri.fsPath;
    const extension = path.extname(file).toLowerCase();
    if (extension === '.walls') {
        throw new Error('Файл .walls — это раскладка стен. Экспортируй сцену: iso-scene tiled scene.scene --walls ...');
    }
    const out = previewPath(file);
    fs.rmSync(out, { force: true });
    await runCli(['render', file, '-o', out], path.dirname(file));
    return out;
}

async function showPreview(document) {
    if (!document) {
        vscode.window.showInformationMessage('iso-scene: открой .scene или .model файл.');
        return;
    }
    try {
        const out = await renderDocument(document);
        if (!panel) {
            panel = vscode.window.createWebviewPanel(
                'isoScenePreview',
                'iso-scene',
                vscode.ViewColumn.Beside,
                { enableScripts: false, localResourceRoots: [vscode.Uri.file(path.dirname(out))] }
            );
            panel.onDidDispose(() => { panel = null; panelFile = null; });
        }
        panelFile = document.uri.fsPath;
        panel.title = `iso-scene: ${path.basename(panelFile)}`;
        const imageUri = panel.webview.asWebviewUri(vscode.Uri.file(out));
        panel.webview.html = html(`${imageUri}?t=${Date.now()}`, path.basename(panelFile), '');
        panel.reveal(vscode.ViewColumn.Beside, true);
    } catch (error) {
        if (!panel) {
            panel = vscode.window.createWebviewPanel(
                'isoScenePreview',
                'iso-scene',
                vscode.ViewColumn.Beside,
                { enableScripts: false }
            );
            panel.onDidDispose(() => { panel = null; panelFile = null; });
        }
        panelFile = document.uri.fsPath;
        panel.title = `iso-scene: ${path.basename(panelFile)} (ошибка)`;
        panel.webview.html = html('', path.basename(panelFile), String(error.message || error));
    }
}

async function exportPng(document) {
    if (!document) {
        return;
    }
    const file = document.uri.fsPath;
    const out = path.join(path.dirname(file), `${path.basename(file, path.extname(file))}.png`);
    try {
        await runCli(['render', file, '-o', out], path.dirname(file));
        vscode.window.showInformationMessage(`iso-scene: ${out}`);
    } catch (error) {
        vscode.window.showErrorMessage(`iso-scene: ${error.message || error}`);
    }
}

async function exportTiled(document) {
    if (!document) {
        return;
    }
    const file = document.uri.fsPath;
    const out = path.join(path.dirname(file), `${path.basename(file, path.extname(file))}-tiled`);
    try {
        await runCli(['tiled', file, '--tile-layer', '-o', out], path.dirname(file));
        vscode.window.showInformationMessage(`iso-scene: ${out}/scene.tmj`);
    } catch (error) {
        vscode.window.showErrorMessage(`iso-scene: ${error.message || error}`);
    }
}

function activate(context) {
    context.subscriptions.push(
        vscode.commands.registerCommand('isoScene.preview', () => {
            const editor = vscode.window.activeTextEditor;
            return showPreview(editor && editor.document);
        }),
        vscode.commands.registerCommand('isoScene.exportPng', () => {
            const editor = vscode.window.activeTextEditor;
            return exportPng(editor && editor.document);
        }),
        vscode.commands.registerCommand('isoScene.exportTiled', () => {
            const editor = vscode.window.activeTextEditor;
            return exportTiled(editor && editor.document);
        }),
        vscode.workspace.onDidSaveTextDocument((document) => {
            if (panel && panelFile === document.uri.fsPath) {
                showPreview(document);
            }
        })
    );
}

function deactivate() {
    if (panel) {
        panel.dispose();
    }
}

module.exports = { activate, deactivate };
