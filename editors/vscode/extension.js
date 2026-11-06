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

function assetDir(file) {
    const hash = crypto.createHash('sha1').update(file).digest('hex').slice(0, 16);
    const dir = path.join(os.tmpdir(), 'iso-scene-preview', hash);
    fs.mkdirSync(dir, { recursive: true });
    return dir;
}

async function renderAssets(document) {
    const file = document.uri.fsPath;
    const extension = path.extname(file).toLowerCase();
    if (extension === '.walls') {
        throw new Error('Файл .walls — это раскладка стен. Экспортируй сцену: iso-scene tiled scene.scene --walls ...');
    }
    const dir = assetDir(file);
    const cwd = path.dirname(file);
    const iso = path.join(dir, 'iso.png');
    fs.rmSync(iso, { force: true });
    await runCli(['render', file, '-o', iso], cwd);

    const views = {};
    for (const view of ['top', 'front', 'side']) {
        const out = path.join(dir, `${view}.png`);
        fs.rmSync(out, { force: true });
        await runCli(['render', file, '--view', view, '-o', out], cwd);
        views[view] = out;
    }

    let voxel = null;
    try {
        const out = path.join(dir, 'model.vox');
        fs.rmSync(out, { force: true });
        await runCli(['voxel', file, '-o', out], cwd);
        voxel = out;
    } catch (error) {
        voxel = null;
    }

    return { dir, iso, views, voxel };
}

function viewerScript() {
    return `
const canvas = document.getElementById('vox');
const ctx = canvas.getContext('2d');
let model = null, zoom = 1, panX = 0, panY = 0;

function parseVox(b64) {
  const bytes = Uint8Array.from(atob(b64), c => c.charCodeAt(0));
  const view = new DataView(bytes.buffer);
  let pos = 20, dims = null, data = null, palette = null;
  while (pos + 12 <= bytes.length) {
    const tag = String.fromCharCode(bytes[pos], bytes[pos+1], bytes[pos+2], bytes[pos+3]);
    const n = view.getUint32(pos + 4, true);
    const base = pos + 12;
    if (tag === 'SIZE') dims = [view.getInt32(base, true), view.getInt32(base+4, true), view.getInt32(base+8, true)];
    if (tag === 'XYZI') { const count = view.getUint32(base, true); data = bytes.slice(base + 4, base + 4 + count * 4); }
    if (tag === 'RGBA') palette = bytes.slice(base, base + n);
    pos = base + n;
  }
  if (!dims || !data || !palette) throw new Error('bad vox');
  const voxels = [], set = new Set();
  for (let i = 0; i < data.length; i += 4) {
    const x = data[i], y = data[i+1], z = data[i+2], c = data[i+3];
    voxels.push([x, y, z, c]);
    set.add(x + ',' + y + ',' + z);
  }
  voxels.sort((a, b) => (a[0] + a[1] + a[2]) - (b[0] + b[1] + b[2]));
  return { dims, voxels, set, palette };
}

function colorOf(palette, c, shade) {
  const i = (c - 1) * 4;
  const r = Math.round(palette[i] * shade);
  const g = Math.round(palette[i+1] * shade);
  const b = Math.round(palette[i+2] * shade);
  return 'rgb(' + r + ',' + g + ',' + b + ')';
}

function draw() {
  if (!model) return;
  const rect = canvas.getBoundingClientRect();
  const ratio = window.devicePixelRatio || 1;
  canvas.width = Math.max(1, Math.round(rect.width * ratio));
  canvas.height = Math.max(1, Math.round(rect.height * ratio));
  ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
  ctx.clearRect(0, 0, rect.width, rect.height);
  const { dims, voxels, set, palette } = model;
  const sBase = Math.min(rect.width / (dims[0] + dims[1]), rect.height / (dims[0] + dims[1] + 2 * dims[2] + 2)) * 1.6 * zoom;
  const ox = rect.width / 2 + panX;
  const oy = rect.height / 2 + panY;
  const px = (x, y, z) => [ (x - y) * sBase / 2 + ox, ((x + y) / 2 - z) * sBase / 2 + oy ];
  const face = (pts, fill, stroke) => {
    ctx.beginPath();
    ctx.moveTo(pts[0][0], pts[0][1]);
    for (let i = 1; i < pts.length; i++) ctx.lineTo(pts[i][0], pts[i][1]);
    ctx.closePath();
    ctx.fillStyle = fill;
    ctx.fill();
    if (stroke) { ctx.strokeStyle = stroke; ctx.lineWidth = 1; ctx.stroke(); }
  };
  for (const [x, y, z, c] of voxels) {
    if (!set.has((x+1) + ',' + y + ',' + z)) face([px(x+1,y,z), px(x+1,y+1,z), px(x+1,y+1,z+1), px(x+1,y,z+1)], colorOf(palette, c, 0.78));
    if (!set.has(x + ',' + (y+1) + ',' + z)) face([px(x,y+1,z), px(x+1,y+1,z), px(x+1,y+1,z+1), px(x,y+1,z+1)], colorOf(palette, c, 0.62));
    if (!set.has(x + ',' + y + ',' + (z+1))) face([px(x,y,z+1), px(x+1,y,z+1), px(x+1,y+1,z+1), px(x,y+1,z+1)], colorOf(palette, c, 1.0));
  }
}

function start() {
  model = parseVox(window.__VOX__);
  draw();
  let dragging = false, lastX = 0, lastY = 0;
  canvas.addEventListener('mousedown', e => { dragging = true; lastX = e.clientX; lastY = e.clientY; });
  window.addEventListener('mouseup', () => { dragging = false; });
  window.addEventListener('mousemove', e => {
    if (!dragging) return;
    panX += e.clientX - lastX; panY += e.clientY - lastY;
    lastX = e.clientX; lastY = e.clientY; draw();
  });
  canvas.addEventListener('wheel', e => {
    e.preventDefault();
    zoom = Math.min(12, Math.max(0.2, zoom * Math.exp(-e.deltaY * 0.0015)));
    draw();
  }, { passive: false });
  window.addEventListener('resize', draw);
  document.querySelectorAll('[data-tab]').forEach(button => {
    button.addEventListener('click', () => {
      document.querySelectorAll('[data-tab]').forEach(b => b.classList.toggle('active', b === button));
      document.querySelectorAll('[data-panel]').forEach(p => p.style.display = p.dataset.panel === button.dataset.tab ? 'block' : 'none');
      if (button.dataset.tab === 'voxel') draw();
    });
  });
}
start();
`;
}

function html(assets, file, error) {
    if (error) {
        return `<!doctype html><html lang="ru"><head><meta charset="utf-8">
<style>body{margin:0;padding:16px;background:#1a1c22;color:#d7d9dd;font-family:var(--vscode-font-family)}
pre{white-space:pre-wrap;color:#ff9a9a}</style></head>
<body><h3>${path.basename(file)}</h3><pre>${error}</pre></body></html>`;
    }
    const nonce = crypto.randomBytes(16).toString('base64');
    const iso = panel.webview.asWebviewUri(vscode.Uri.file(assets.iso));
    const top = panel.webview.asWebviewUri(vscode.Uri.file(assets.views.top));
    const front = panel.webview.asWebviewUri(vscode.Uri.file(assets.views.front));
    const side = panel.webview.asWebviewUri(vscode.Uri.file(assets.views.side));
    const vox = assets.voxel ? fs.readFileSync(assets.voxel).toString('base64') : '';
    return `<!doctype html><html lang="ru"><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src ${panel.webview.cspSource} data:; style-src 'unsafe-inline'; script-src 'nonce-${nonce}';">
<style>
body{margin:0;background:#1a1c22;color:#d7d9dd;font-family:var(--vscode-font-family)}
nav{display:flex;gap:4px;padding:8px;position:sticky;top:0;background:#1a1c22}
button{background:#2a2d33;color:#d7d9dd;border:0;padding:7px 10px;border-radius:6px;cursor:pointer}
button.active{background:#3d6ea8;color:#fff}
.panel{display:none;padding:0 12px 12px}
.panel.active{display:block}
.frame{display:flex;justify-content:center;padding:12px;background:#202226;border-radius:8px}
img{max-width:100%;max-height:calc(100vh - 150px);image-rendering:pixelated}
.caption{color:#9aa0a8;font-size:12px;margin-top:6px}
.views{display:grid;grid-template-columns:repeat(3,1fr);gap:10px}
.views figure{margin:0;background:#202226;border-radius:8px;padding:8px}
.views img{max-width:100%;max-height:60vh}
.views figcaption{text-align:center;color:#9aa0a8;font-size:12px;margin-top:4px}
canvas{width:100%;height:70vh;display:block;background:#202226;border-radius:8px;touch-action:none;cursor:grab}
</style></head>
<body>
<nav>
  <button data-tab="iso" class="active">Изометрия</button>
  <button data-tab="drawing">Чертёж</button>
  <button data-tab="voxel">Воксели</button>
</nav>
<div class="panel active" data-panel="iso">
  <div class="frame"><img src="${iso}?t=${Date.now()}" alt="iso"></div>
  <div class="caption">${path.basename(file)} — сохрани файл, чтобы обновить.</div>
</div>
<div class="panel" data-panel="drawing">
  <div class="views">
    <figure><img src="${top}?t=${Date.now()}" alt="top"><figcaption>сверху (XY)</figcaption></figure>
    <figure><img src="${front}?t=${Date.now()}" alt="front"><figcaption>спереди (XZ)</figcaption></figure>
    <figure><img src="${side}?t=${Date.now()}" alt="side"><figcaption>сбоку (YZ)</figcaption></figure>
  </div>
</div>
<div class="panel" data-panel="voxel">
  ${assets.voxel
        ? '<canvas id="vox"></canvas><div class="caption">колесо — масштаб, перетаскивание — сдвиг.</div>'
        : '<div class="caption" style="color:#ff9a9a">Не удалось собрать воксели (слишком большая сетка?).</div>'}
</div>
<script nonce="${nonce}">${assets.voxel ? `window.__VOX__ = "${vox}";` : ''}${assets.voxel ? viewerScript() : ''}</script>
</body></html>`;
}

async function showPreview(document) {
    if (!document) {
        vscode.window.showInformationMessage('iso-scene: открой .scene или .model файл.');
        return;
    }
    const file = document.uri.fsPath;
    try {
        const assets = await renderAssets(document);
        if (!panel) {
            panel = vscode.window.createWebviewPanel(
                'isoScenePreview',
                'iso-scene',
                vscode.ViewColumn.Beside,
                {
                    enableScripts: true,
                    localResourceRoots: [vscode.Uri.file(assets.dir)]
                }
            );
            panel.onDidDispose(() => { panel = null; panelFile = null; });
        }
        panelFile = file;
        panel.title = `iso-scene: ${path.basename(file)}`;
        panel.webview.html = html(assets, file, '');
        panel.reveal(vscode.ViewColumn.Beside, true);
    } catch (error) {
        if (!panel) {
            panel = vscode.window.createWebviewPanel('isoScenePreview', 'iso-scene', vscode.ViewColumn.Beside, { enableScripts: false });
            panel.onDidDispose(() => { panel = null; panelFile = null; });
        }
        panelFile = file;
        panel.title = `iso-scene: ${path.basename(file)} (ошибка)`;
        panel.webview.html = html(null, file, String(error.message || error));
    }
}

async function build(document) {
    if (!document) {
        return;
    }
    const file = document.uri.fsPath;
    const stem = path.basename(file, path.extname(file));
    const outDir = path.join(path.dirname(file), 'out');
    fs.mkdirSync(outDir, { recursive: true });
    const written = [];
    try {
        const png = path.join(outDir, `${stem}.png`);
        await runCli(['render', file, '-o', png], path.dirname(file));
        written.push(png);
        const vox = path.join(outDir, `${stem}.vox`);
        await runCli(['voxel', file, '-o', vox], path.dirname(file));
        written.push(vox);
        if (path.extname(file).toLowerCase() === '.scene') {
            const tiled = path.join(outDir, `${stem}-tiled`);
            await runCli(['tiled', file, '--tile-layer', '-o', tiled], path.dirname(file));
            written.push(path.join(tiled, 'scene.tmj'));
        }
        vscode.window.showInformationMessage(`iso-scene: ${written.join(', ')}`);
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
        vscode.commands.registerCommand('isoScene.build', () => {
            const editor = vscode.window.activeTextEditor;
            return build(editor && editor.document);
        }),
        vscode.commands.registerCommand('isoScene.exportPng', async () => {
            const editor = vscode.window.activeTextEditor;
            if (!editor) return;
            const file = editor.document.uri.fsPath;
            const out = path.join(path.dirname(file), `${path.basename(file, path.extname(file))}.png`);
            try {
                await runCli(['render', file, '-o', out], path.dirname(file));
                vscode.window.showInformationMessage(`iso-scene: ${out}`);
            } catch (error) {
                vscode.window.showErrorMessage(`iso-scene: ${error.message || error}`);
            }
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
