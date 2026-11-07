'use strict';
// Чтение .vox: одна или несколько моделей, собранных через scene graph.
(function (root, factory) {
    if (typeof module === 'object' && module.exports) module.exports = factory();
    else root.voxReader = factory();
})(typeof self !== 'undefined' ? self : this, function () {
    function readDict(view, bytes, start) {
        const result = {};
        let pos = start;
        const count = view.getUint32(pos, true);
        pos += 4;
        for (let i = 0; i < count; i++) {
            const keyLength = view.getUint32(pos, true); pos += 4;
            const key = String.fromCharCode(...bytes.slice(pos, pos + keyLength)); pos += keyLength;
            const valueLength = view.getUint32(pos, true); pos += 4;
            const value = String.fromCharCode(...bytes.slice(pos, pos + valueLength)); pos += valueLength;
            result[key] = value;
        }
        return [result, pos];
    }

    function readVox(base64) {
        const bytes = Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
        const view = new DataView(bytes.buffer);
        if (String.fromCharCode(...bytes.slice(0, 4)) !== 'VOX ') throw Error('Неверный VOX');
        let pos = 20;
        const models = [];
        const nodes = [];
        let palette = null;
        let current = null;
        while (pos + 12 <= bytes.length) {
            const tag = String.fromCharCode(...bytes.slice(pos, pos + 4));
            const length = view.getUint32(pos + 4, true);
            const base = pos + 12;
            if (base + length > bytes.length) throw Error('Повреждённый VOX');
            if (tag === 'SIZE') {
                current = {
                    dims: [
                        view.getInt32(base, true),
                        view.getInt32(base + 4, true),
                        view.getInt32(base + 8, true)
                    ],
                    values: null
                };
                models.push(current);
            } else if (tag === 'XYZI' && current) {
                const count = view.getUint32(base, true);
                current.values = bytes.slice(base + 4, base + 4 + count * 4);
            } else if (tag === 'RGBA') {
                palette = bytes.slice(base, base + length);
            } else if (tag === 'nTRN') {
                let p = base;
                const id = view.getUint32(p, true); p += 4;
                const [, afterDict] = readDict(view, bytes, p); p = afterDict;
                const child = view.getUint32(p, true); p += 4;
                p += 4; // reserved
                p += 4; // layer id
                const frames = view.getUint32(p, true); p += 4;
                const frameList = [];
                for (let i = 0; i < frames; i++) {
                    const [attrs, next] = readDict(view, bytes, p);
                    frameList.push(attrs);
                    p = next;
                }
                nodes.push({ tag: 'nTRN', id, child, frames: frameList });
            } else if (tag === 'nGRP') {
                let p = base;
                const id = view.getUint32(p, true); p += 4;
                const [, afterDict] = readDict(view, bytes, p); p = afterDict;
                const count = view.getUint32(p, true); p += 4;
                const children = [];
                for (let i = 0; i < count; i++) { children.push(view.getUint32(p, true)); p += 4; }
                nodes.push({ tag: 'nGRP', id, children });
            } else if (tag === 'nSHP') {
                let p = base;
                const id = view.getUint32(p, true); p += 4;
                const [, afterDict] = readDict(view, bytes, p); p = afterDict;
                const count = view.getUint32(p, true); p += 4;
                const refs = [];
                for (let i = 0; i < count; i++) {
                    refs.push(view.getUint32(p, true)); p += 4;
                    const [, next] = readDict(view, bytes, p); p = next;
                }
                nodes.push({ tag: 'nSHP', id, models: refs });
            }
            pos = base + length;
        }
        if (!palette || !models.length) throw Error('В VOX отсутствуют данные');

        const parent = new Map();
        for (const node of nodes) {
            if (node.tag === 'nTRN') parent.set(node.child, node);
            if (node.tag === 'nGRP') for (const child of node.children) parent.set(child, node);
        }

        const voxels = [];
        const set = new Set();
        const dims = [0, 0, 0];
        const merge = (modelIndex, offset) => {
            const model = models[modelIndex];
            if (!model || !model.values) return;
            for (let i = 0; i < model.values.length; i += 4) {
                const x = model.values[i] + offset[0];
                const y = model.values[i + 1] + offset[1];
                const z = model.values[i + 2] + offset[2];
                const c = model.values[i + 3];
                voxels.push([x, y, z, c]);
                set.add(x + ',' + y + ',' + z);
                dims[0] = Math.max(dims[0], x + 1);
                dims[1] = Math.max(dims[1], y + 1);
                dims[2] = Math.max(dims[2], z + 1);
            }
        };

        const shapes = nodes.filter((node) => node.tag === 'nSHP');
        if (shapes.length === 0) {
            merge(0, [0, 0, 0]);
        } else {
            for (const shape of shapes) {
                const offset = [0, 0, 0];
                let node = shape;
                let guard = 0;
                while (node && guard++ < 64) {
                    const up = parent.get(node.id);
                    if (!up) break;
                    if (up.tag === 'nTRN' && up.frames[0] && up.frames[0]._t) {
                        const parts = up.frames[0]._t.trim().split(/\s+/).map(Number);
                        offset[0] += parts[0] || 0;
                        offset[1] += parts[1] || 0;
                        offset[2] += parts[2] || 0;
                    }
                    node = up;
                }
                for (const modelIndex of shape.models) merge(modelIndex, offset);
            }
        }
        if (!voxels.length) throw Error('В VOX нет вокселей');
        return { dims, voxels, set, palette };
    }

    return { readVox };
});
