'use strict';
// The extension's LSP glue against a fake `vscode` module and the real
// `iso-scene lsp`. It cannot replace a run in VS Code, but it catches broken
// wiring: events that do not reach the server, results not converted.
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const Module = require('module');
const { pathToFileURL } = require('url');

const repo = path.resolve(__dirname, '..', '..', '..');
const cli = process.env.ISO_SCENE || ['release', 'debug']
    .map((profile) => path.join(repo, 'target', profile, 'iso-scene'))
    .find((file) => fs.existsSync(file));

function fakeVscode(dir) {
    const listeners = {};
    const on = (name) => (listener) => {
        (listeners[name] ||= []).push(listener);
        return { dispose() {} };
    };
    const providers = {};
    const diagnostics = new Map();
    class Range {
        constructor(sl, sc, el, ec) { Object.assign(this, { sl, sc, el, ec }); }
    }
    class Uri {
        constructor(text) { this.text = text; this.scheme = 'file'; this.fsPath = new URL(text).pathname; }
        toString() { return this.text; }
        static parse(text) { return new Uri(text); }
        static file(file) { return new Uri(pathToFileURL(file).toString()); }
    }
    class MarkdownString {
        constructor(value = '') { this.value = value; }
        appendCodeblock(code) { this.value += code; }
        appendMarkdown(text) { this.value += text; }
    }
    const vscode = {
        Range, Uri, MarkdownString,
        Diagnostic: class { constructor(range, message, severity) { Object.assign(this, { range, message, severity }); } },
        DiagnosticSeverity: { Error: 0 },
        Hover: class { constructor(contents, range) { Object.assign(this, { contents, range }); } },
        CompletionItem: class { constructor(label, kind) { Object.assign(this, { label, kind }); } },
        CompletionItemKind: { Snippet: 14 },
        SnippetString: class { constructor(value) { this.value = value; } },
        Location: class { constructor(uri, range) { Object.assign(this, { uri, range }); } },
        DocumentSymbol: class { constructor(name, detail, kind, range, selectionRange) { Object.assign(this, { name, detail, kind, range, selectionRange }); } },
        StatusBarAlignment: { Right: 2 },
        ViewColumn: { Beside: -2 },
        workspace: {
            isTrusted: true,
            workspaceFolders: [{ uri: Uri.file(dir) }],
            textDocuments: [],
            getConfiguration: () => ({ get: (key) => (key === 'executable' ? cli : undefined) }),
            onDidOpenTextDocument: on('open'),
            onDidCloseTextDocument: on('close'),
            onDidSaveTextDocument: on('save'),
            onDidChangeTextDocument: on('change'),
            onDidChangeConfiguration: on('config'),
            onDidGrantWorkspaceTrust: on('trust')
        },
        languages: {
            createDiagnosticCollection: () => ({
                set: (uri, list) => diagnostics.set(uri.toString(), list),
                delete: (uri) => diagnostics.delete(uri.toString()),
                clear: () => diagnostics.clear(),
                dispose() {}
            }),
            registerHoverProvider: (_, provider) => { providers.hover = provider; },
            registerCompletionItemProvider: (_, provider) => { providers.completion = provider; },
            registerDefinitionProvider: (_, provider) => { providers.definition = provider; },
            registerDocumentSymbolProvider: (_, provider) => { providers.symbols = provider; }
        },
        window: {
            createOutputChannel: () => ({ append() {}, appendLine() {}, show() {}, dispose() {} }),
            createStatusBarItem: () => ({ show() {}, hide() {}, dispose() {} }),
            onDidChangeActiveTextEditor: on('editor'),
            activeTextEditor: undefined
        },
        commands: { registerCommand: () => ({ dispose() {} }) }
    };
    return { vscode, listeners, providers, diagnostics };
}

function document(file, text, version = 1) {
    return {
        languageId: 'iso-scene',
        uri: { scheme: 'file', fsPath: file, toString: () => pathToFileURL(file).toString() },
        version,
        getText: () => text
    };
}

async function until(check) {
    for (let i = 0; i < 200; i += 1) {
        const value = check();
        if (value) return value;
        await new Promise((resolve) => setTimeout(resolve, 10));
    }
    throw new Error('timed out');
}

test('the extension forwards documents to iso-scene lsp and converts its answers', { skip: !cli }, async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'iso-scene-ext-'));
    fs.writeFileSync(path.join(dir, 'desk.model'),
        'version 1;\nobject desk (5, 2) {\n    box at (0, 0, 0) size (w, d, 1);\n}\n');
    const file = path.join(dir, 'room.scene');
    const fake = fakeVscode(dir);
    const resolve = Module._resolveFilename;
    Module._resolveFilename = function (request, ...rest) {
        return request === 'vscode' ? 'vscode' : resolve.call(this, request, ...rest);
    };
    require.cache.vscode = { id: 'vscode', filename: 'vscode', loaded: true, exports: fake.vscode };
    const extension = require('../extension');
    extension.activate({ subscriptions: [] });
    try {
        await check(fake, dir, file);
    } finally {
        // Stop the server even when a check fails, or the test never ends.
        await extension.deactivate();
        Module._resolveFilename = resolve;
        fs.rmSync(dir, { recursive: true, force: true });
    }
});

async function check(fake, dir, file) {
    const uri = pathToFileURL(file).toString();
    const broken = document(file, 'version 1;\ninclude "desk.model";\nrender iso; scale 8;\nplace sofa at (0, 0, 0);\n');
    // The client starts asynchronously; open the document once it runs.
    await until(() => {
        fake.listeners.open.forEach((listener) => listener(broken));
        return fake.diagnostics.get(uri);
    });
    const [problem] = fake.diagnostics.get(uri);
    assert.strictEqual(problem.code, 'E0301');
    assert.strictEqual(problem.range.sl, 3);

    const fixed = document(file, 'version 1;\ninclude "desk.model";\nrender iso; scale 8;\nplace desk at (0, 0, 0);\n', 2);
    fake.listeners.change.forEach((listener) => listener({ document: fixed, contentChanges: [{}] }));
    await until(() => fake.diagnostics.get(uri)?.length === 0);

    const position = { line: 3, character: 7 };
    const hover = await fake.providers.hover.provideHover(fixed, position);
    assert.match(hover.contents.value, /object desk \(5, 2\)/);
    const location = await fake.providers.definition.provideDefinition(fixed, position);
    assert.ok(location.uri.toString().endsWith('/desk.model'));
    assert.strictEqual(location.range.sl, 1);
    const items = await fake.providers.completion.provideCompletionItems(fixed, { line: 3, character: 0 });
    const box = items.find((item) => item.label === 'box');
    assert.ok(box.insertText.value.startsWith('box at ('));
    // VS Code asks for the symbols of open documents only.
    const model = document(path.join(dir, 'desk.model'), fs.readFileSync(path.join(dir, 'desk.model'), 'utf8'));
    fake.listeners.open.forEach((listener) => listener(model));
    const symbols = await fake.providers.symbols.provideDocumentSymbols(model);
    assert.deepStrictEqual(symbols.map((symbol) => [symbol.name, symbol.detail]), [['desk', 'object desk (5, 2)']]);
    assert.strictEqual(symbols[0].selectionRange.sl, 1);
}
