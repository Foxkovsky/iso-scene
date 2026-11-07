'use strict';
const vscode = acquireVsCodeApi();
const img = document.querySelector('#image');
const views = document.querySelector('#views');
const canvas = document.querySelector('#canvas');
const ctx = canvas.getContext('2d');
const info = document.querySelector('#info');
const hint = document.querySelector('#hint');
const buttons = [...document.querySelectorAll('button[data-tab]')];
let tab = 'iso';
let data = null;
let model = null;
let drag = null;
let yaw = 0.7;
let pitch = 0.55;
let zoom = 1;

function switchTab(next) {
    const changed = tab !== next;
    tab = next;
    if (changed) vscode.postMessage({ type: 'tab', tab: next });
    buttons.forEach((b) => b.classList.toggle('active', b.dataset.tab === next));
    img.style.display = next === 'iso' ? 'block' : 'none';
    views.style.display = next === 'drawing' ? 'grid' : 'none';
    canvas.style.display = next === 'voxel' ? 'block' : 'none';
    hint.textContent = next === 'voxel'
        ? 'Вращение: перетаскивание мышью. Масштаб: колесо. Оси X/Y/Z — от начала сетки.'
        : '';
    if (next === 'iso' && data && data.iso) {
        img.src = 'data:image/png;base64,' + data.iso;
    }
    if (next === 'drawing' && data && data.drawing) {
        document.querySelectorAll('#views img').forEach((el, index) => {
            if (data.drawing[index]) el.src = 'data:image/png;base64,' + data.drawing[index];
        });
    }
    if (next === 'voxel') {
        if (model) draw();
        else ctx.clearRect(0, 0, canvas.width, canvas.height);
    }
}

buttons.forEach((b) => b.addEventListener('click', () => switchTab(b.dataset.tab)));

const readVox = (base64) => voxReader.readVox(base64);

function draw() {
    if (!model || tab !== 'voxel') return;
    const rect = canvas.getBoundingClientRect();
    const ratio = window.devicePixelRatio || 1;
    canvas.width = Math.round(rect.width * ratio);
    canvas.height = Math.round(rect.height * ratio);
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.clearRect(0, 0, rect.width, rect.height);

    const { dims, voxels, set, palette } = model;
    const center = [dims[0] / 2, dims[1] / 2, dims[2] / 2];
    const extent = Math.max(...dims);
    const cy = Math.cos(yaw), sy = Math.sin(yaw), cp = Math.cos(pitch), sp = Math.sin(pitch);
    const viewDir = [sy * cp, cy * cp, sp];
    const factor = Math.min(rect.width, rect.height) * 0.75 / extent * zoom;

    function project(v) {
        const x = v[0] - center[0], y = v[1] - center[1], z = v[2] - center[2];
        const horizontal = x * cy - y * sy;
        const depth = x * sy + y * cy;
        return [rect.width / 2 + horizontal * factor, rect.height / 2 + (depth * sp - z * cp) * factor, depth * cp + z * sp];
    }

    const dirs = [
        [[1, 0, 0], [[1, 0, 0], [1, 1, 0], [1, 1, 1], [1, 0, 1]]],
        [[-1, 0, 0], [[0, 0, 0], [0, 0, 1], [0, 1, 1], [0, 1, 0]]],
        [[0, 1, 0], [[0, 1, 0], [0, 1, 1], [1, 1, 1], [1, 1, 0]]],
        [[0, -1, 0], [[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]]],
        [[0, 0, 1], [[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]]],
        [[0, 0, -1], [[0, 0, 0], [0, 1, 0], [1, 1, 0], [1, 0, 0]]]
    ];
    const faces = [];
    for (const [x, y, z, c] of voxels) {
        for (const [n, verts] of dirs) {
            if (n[0] * viewDir[0] + n[1] * viewDir[1] + n[2] * viewDir[2] <= 0) continue;
            if (set.has((x + n[0]) + ',' + (y + n[1]) + ',' + (z + n[2]))) continue;
            const points = verts.map((v) => project([x + v[0], y + v[1], z + v[2]]));
            const depth = points.reduce((a, p) => a + p[2], 0) / 4;
            const light = 0.55 + 0.45 * Math.max(0, n[0] * 0.3 + n[1] * 0.3 + n[2] * 0.9);
            const idx = (c - 1) * 4;
            faces.push({
                points,
                depth,
                color: 'rgb(' + Math.round(palette[idx] * light) + ',' + Math.round(palette[idx + 1] * light) + ',' + Math.round(palette[idx + 2] * light) + ')'
            });
        }
    }
    faces.sort((a, b) => a.depth - b.depth);
    for (const f of faces) {
        ctx.beginPath();
        ctx.moveTo(f.points[0][0], f.points[0][1]);
        for (let i = 1; i < 4; i++) ctx.lineTo(f.points[i][0], f.points[i][1]);
        ctx.closePath();
        ctx.fillStyle = f.color;
        ctx.fill();
        ctx.strokeStyle = f.color;
        ctx.lineWidth = 0.45;
        ctx.stroke();
    }

    const zero = project([0, 0, 0]);
    const axisLength = extent * 0.35;
    const axes = [['X', '#ff6363', [axisLength, 0, 0]], ['Y', '#75dc8b', [0, axisLength, 0]], ['Z', '#7cb5ff', [0, 0, axisLength]]];
    ctx.save();
    ctx.lineWidth = 2;
    ctx.font = 'bold 13px sans-serif';
    for (const [label, color, delta] of axes) {
        const p = project(delta);
        ctx.beginPath();
        ctx.moveTo(zero[0], zero[1]);
        ctx.lineTo(p[0], p[1]);
        ctx.strokeStyle = color;
        ctx.stroke();
        ctx.fillStyle = color;
        ctx.fillText(label, p[0] + 3, p[1] - 3);
    }
    ctx.fillStyle = '#ffffff';
    ctx.beginPath();
    ctx.arc(zero[0], zero[1], 3, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillText('0', zero[0] + 4, zero[1] + 14);
    ctx.restore();
}

canvas.addEventListener('pointerdown', (e) => {
    drag = [e.clientX, e.clientY];
    canvas.setPointerCapture(e.pointerId);
});
canvas.addEventListener('pointermove', (e) => {
    if (!drag) return;
    yaw += (e.clientX - drag[0]) * 0.01;
    pitch = Math.max(-1.45, Math.min(1.45, pitch + (e.clientY - drag[1]) * 0.01));
    drag = [e.clientX, e.clientY];
    draw();
});
canvas.addEventListener('pointerup', () => { drag = null; });
canvas.addEventListener('pointercancel', () => { drag = null; });
canvas.addEventListener('wheel', (e) => {
    e.preventDefault();
    zoom = Math.max(0.2, Math.min(5, zoom * Math.exp(-e.deltaY * 0.001)));
    draw();
}, { passive: false });
window.addEventListener('resize', draw);

window.addEventListener('message', (event) => {
    const m = event.data;
    if (m.type === 'status') { info.textContent = m.text; return; }
    if (m.type === 'error') { info.textContent = 'Ошибка: ' + m.text; return; }
    if (m.type !== 'render') return;
    if (!data) data = {};
    if (m.kind === 'voxel') {
        try {
            if (m.voxelError) throw Error(m.voxelError);
            model = readVox(m.vox);
            info.textContent = m.info;
        } catch (err) {
            model = null;
            info.textContent = 'Воксельный просмотр: ' + err.message;
        }
    } else if (m.kind === 'drawing') {
        data.drawing = m.images;
        info.textContent = 'Обновлено';
    } else {
        data[m.kind] = m.image;
        info.textContent = 'Обновлено';
    }
    if (m.kind === tab) switchTab(tab);
});

switchTab('iso');
vscode.postMessage({ type: 'ready' });
