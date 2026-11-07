'use strict';
// node --test editors/vscode/test
// Нужен собранный CLI: ISO_SCENE=<путь> или target/{release,debug}/iso-scene.
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { pathToFileURL } = require('url');
const { LspClient } = require('../lsp_client');

const repo = path.resolve(__dirname, '..', '..', '..');
const cli = process.env.ISO_SCENE || ['release', 'debug']
    .map((profile) => path.join(repo, 'target', profile, 'iso-scene'))
    .find((file) => fs.existsSync(file));

function nextNotification(client, method) {
    return new Promise((resolve) => {
        const listener = (name, params) => {
            if (name !== method) return;
            client.off('notification', listener);
            resolve(params);
        };
        client.on('notification', listener);
    });
}

test('diagnostics, hover, completion and definition through iso-scene lsp', { skip: !cli }, async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'iso-scene-vscode-'));
    fs.writeFileSync(path.join(dir, 'desk.model'),
        'version 1;\nobject desk (5, 2) {\n    box at (0, 0, 0) size (w, d, 1);\n}\n');
    const uri = pathToFileURL(path.join(dir, 'room.scene')).toString();
    const client = new LspClient([cli], { cwd: dir, rootUri: pathToFileURL(dir).toString() });
    const capabilities = await client.start();
    assert.strictEqual(capabilities.hoverProvider, true);

    let published = nextNotification(client, 'textDocument/publishDiagnostics');
    client.notify('textDocument/didOpen', { textDocument: {
        uri, languageId: 'iso-scene', version: 1,
        text: 'version 1;\ninclude "desk.model";\nrender iso; scale 8;\nplace sofa at (0, 0, 0);\n'
    } });
    let params = await published;
    assert.strictEqual(params.uri, uri);
    assert.strictEqual(params.diagnostics[0].code, 'E0301');
    assert.strictEqual(params.diagnostics[0].range.start.line, 3);

    published = nextNotification(client, 'textDocument/publishDiagnostics');
    client.notify('textDocument/didChange', {
        textDocument: { uri, version: 2 },
        contentChanges: [{ text: 'version 1;\ninclude "desk.model";\nrender iso; scale 8;\nplace desk at (0, 0, 0);\n' }]
    });
    params = await published;
    assert.deepStrictEqual(params.diagnostics, []);

    const at = { textDocument: { uri }, position: { line: 3, character: 7 } };
    const hover = await client.request('textDocument/hover', at);
    assert.match(hover.contents.value, /object desk \(5, 2\)/);
    const definition = await client.request('textDocument/definition', at);
    assert.strictEqual(definition.range.start.line, 1);
    assert.ok(definition.uri.endsWith('/desk.model'));
    const items = await client.request('textDocument/completion',
        { textDocument: { uri }, position: { line: 3, character: 0 } });
    assert.ok(items.some((item) => item.label === 'box' && item.insertTextFormat === 2));

    await client.stop();
    fs.rmSync(dir, { recursive: true, force: true });
});

test('a missing CLI rejects start', async () => {
    const client = new LspClient([path.join(os.tmpdir(), 'no-such-iso-scene')]);
    await assert.rejects(client.start(), /не найден/);
});

test('a CLI without `lsp` rejects start instead of hanging', async () => {
    // `false` starts and exits at once, as an old iso-scene would on an
    // unknown command.
    const client = new LspClient(['false']);
    await assert.rejects(client.start(), /завершился/);
});

test('messages split across chunks and glued together are both parsed', () => {
    const client = new LspClient([]);
    const seen = [];
    client.on('notification', (method, params) => seen.push([method, params]));
    const frame = (message) => {
        const body = Buffer.from(JSON.stringify(message), 'utf8');
        return Buffer.concat([Buffer.from(`Content-Length: ${body.length}\r\n\r\n`), body]);
    };
    const one = frame({ jsonrpc: '2.0', method: 'a', params: { text: 'привет' } });
    const two = frame({ jsonrpc: '2.0', method: 'b', params: {} });
    const all = Buffer.concat([one, two]);
    client.receive(all.subarray(0, 10));
    client.receive(all.subarray(10, one.length + 3));
    client.receive(all.subarray(one.length + 3));
    assert.deepStrictEqual(seen, [['a', { text: 'привет' }], ['b', {}]]);
});
