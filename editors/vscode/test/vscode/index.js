'use strict';
// Runs inside VS Code (see run.js): the extension against `iso-scene lsp`,
// through the same commands the editor uses for hovers, F12, completion and
// the outline. The workspace is a temporary copy of examples/.
const assert = require('assert');
const path = require('path');
const vscode = require('vscode');

const folder = () => vscode.workspace.workspaceFolders[0].uri;
const file = (name) => vscode.Uri.joinPath(folder(), name);

async function until(what, check, timeout = 15000) {
    const start = Date.now();
    for (;;) {
        const value = await check();
        if (value) return value;
        if (Date.now() - start > timeout) throw new Error('timed out waiting for ' + what);
        await new Promise((resolve) => setTimeout(resolve, 50));
    }
}

async function open(name) {
    const document = await vscode.workspace.openTextDocument(file(name));
    await vscode.window.showTextDocument(document);
    return document;
}

async function replaceAll(document, text) {
    const edit = new vscode.WorkspaceEdit();
    edit.replace(document.uri, new vscode.Range(0, 0, document.lineCount, 0), text);
    assert.ok(await vscode.workspace.applyEdit(edit));
}

function hoverText(hovers) {
    return hovers.flatMap((hover) => hover.contents)
        .map((part) => (typeof part === 'string' ? part : part.value)).join('\n');
}

const position = (document, needle, offset = 1) => document.positionAt(document.getText().indexOf(needle) + offset);

const checks = {
    async 'a clean scene has no problems'() {
        const document = await open('classroom.scene');
        // The server answers on open; give it time, then expect nothing.
        await new Promise((resolve) => setTimeout(resolve, 1500));
        assert.deepStrictEqual(vscode.languages.getDiagnostics(document.uri), []);
    },

    async 'hover shows an object declared in an included model'() {
        const document = await open('classroom.scene');
        const hovers = await until('hover', async () => {
            const result = await vscode.commands.executeCommand(
                'vscode.executeHoverProvider', document.uri, position(document, 'place desk', 7));
            return hoverText(result).includes('object desk') ? result : undefined;
        });
        assert.match(hoverText(hovers), /object desk \(5, 2\)/);
        assert.match(hoverText(hovers), /desk\.model/);
    },

    async 'hover explains a keyword from the spec'() {
        const document = await open('classroom.scene');
        const result = await vscode.commands.executeCommand(
            'vscode.executeHoverProvider', document.uri, position(document, 'scale 32', 1));
        assert.match(hoverText(result), /1\.\.256/);
    },

    async 'F12 jumps into the included model'() {
        const document = await open('classroom.scene');
        const locations = await vscode.commands.executeCommand(
            'vscode.executeDefinitionProvider', document.uri, position(document, 'place chair', 7));
        assert.strictEqual(locations.length, 1);
        const target = locations[0].uri || locations[0].targetUri;
        const range = locations[0].range || locations[0].targetSelectionRange;
        assert.strictEqual(path.basename(target.fsPath), 'chair.model');
        const model = await vscode.workspace.openTextDocument(target);
        assert.strictEqual(model.getText(range), 'chair');
    },

    async 'completion offers objects after place and snippets for shapes'() {
        const document = await open('classroom.scene');
        const end = document.positionAt(document.getText().length);
        const edit = new vscode.WorkspaceEdit();
        edit.insert(document.uri, end, '\nplace ');
        await vscode.workspace.applyEdit(edit);
        const at = document.positionAt(document.getText().length);
        const list = await vscode.commands.executeCommand('vscode.executeCompletionItemProvider', document.uri, at);
        // From the server: objects come as classes with the detail `object`
        // (VS Code's own word suggestions would also list `desk`).
        const objects = list.items
            .filter((item) => item.kind === vscode.CompletionItemKind.Class && item.detail === 'object')
            .map((item) => (typeof item.label === 'string' ? item.label : item.label.label));
        assert.deepStrictEqual(objects.sort(), ['chair', 'desk']);
        await vscode.commands.executeCommand('undo');

        const model = await open('desk.model');
        const inside = position(model, 'box at', 0);
        const shapes = await vscode.commands.executeCommand('vscode.executeCompletionItemProvider', model.uri, inside);
        const boxes = shapes.items.filter((item) => (typeof item.label === 'string' ? item.label : item.label.label) === 'box');
        assert.strictEqual(boxes.length, 1, 'one `box`');
        assert.ok(boxes[0].insertText instanceof vscode.SnippetString, 'a snippet');
    },

    async 'the outline lists objects and materials'() {
        const document = await open('desk.model');
        const symbols = await vscode.commands.executeCommand('vscode.executeDocumentSymbolProvider', document.uri);
        const names = symbols.map((symbol) => symbol.name);
        assert.deepStrictEqual(names, ['oak', 'steel', 'desk']);
    },

    async 'typing an error shows it in Problems and fixing it clears it'() {
        const document = await open('classroom.scene');
        const original = document.getText();
        try {
            await replaceAll(document, original.replace('place desk', 'place sofa'));
            const problems = await until('a problem', () => {
                const list = vscode.languages.getDiagnostics(document.uri);
                return list.length ? list : undefined;
            });
            assert.strictEqual(problems[0].code, 'E0301');
            assert.strictEqual(problems[0].source, 'iso-scene');
            // The evaluator puts this error on the whole placement.
            assert.match(document.getText(problems[0].range), /^place sofa at /);
        } finally {
            await replaceAll(document, original);
        }
        await until('no problems', () => vscode.languages.getDiagnostics(document.uri).length === 0);
    },

    async 'an error in an included model is shown on the include'() {
        const model = await open('chair.model');
        const original = model.getText();
        const scene = await open('classroom.scene');
        try {
            await replaceAll(model, original.replace('size (leg, leg, leg_h)', 'size (0, leg, leg_h)'));
            await model.save();
            const problems = await until('include problem', () => {
                const list = vscode.languages.getDiagnostics(scene.uri);
                return list.length ? list : undefined;
            });
            assert.match(problems[0].message, /in `chair\.model` at \d+:\d+/);
            assert.strictEqual(problems[0].range.start.line, scene.positionAt(scene.getText().indexOf('include "chair')).line);
        } finally {
            await replaceAll(model, original);
            await model.save();
        }
        await until('no problems', () => vscode.languages.getDiagnostics(scene.uri).length === 0);
    },

    async 'walls files are checked too'() {
        const walls = await open('room.walls');
        await replaceAll(walls, 'version 1;\nlayout {\n"#?#"\n}\n');
        const problems = await until('walls problem', () => {
            const list = vscode.languages.getDiagnostics(walls.uri);
            return list.length ? list : undefined;
        });
        assert.strictEqual(problems[0].code, 'E0202');
    },

    async 'build writes the png, vox and Tiled map'() {
        const document = await open('classroom.scene');
        await vscode.commands.executeCommand('isoScene.build');
        const out = vscode.Uri.joinPath(folder(), 'out');
        for (const name of ['classroom.png', 'classroom.vox', 'classroom-tiled/scene.tmj']) {
            await until(name, async () => {
                try {
                    return (await vscode.workspace.fs.stat(vscode.Uri.joinPath(out, name))).size > 0;
                } catch (error) {
                    return false;
                }
            }, 30000);
        }
        assert.deepStrictEqual(vscode.languages.getDiagnostics(document.uri), []);
    },

    async 'the preview opens without adding problems'() {
        const document = await open('classroom.scene');
        await vscode.commands.executeCommand('isoScene.preview');
        await new Promise((resolve) => setTimeout(resolve, 2000));
        assert.deepStrictEqual(vscode.languages.getDiagnostics(document.uri), []);
    },

    async 'a CLI without lsp falls back as well'() {
        const config = vscode.workspace.getConfiguration('isoScene');
        const working = config.get('executable');
        // `false` starts and exits at once, like an iso-scene from before `lsp`.
        await config.update('executable', 'false', vscode.ConfigurationTarget.Workspace);
        try {
            const document = await open('desk.model');
            await until('fallback hover', async () => {
                const result = await vscode.commands.executeCommand(
                    'vscode.executeHoverProvider', document.uri, position(document, 'box at', 1));
                return hoverText(result).includes('size (w, d, h) [material]');
            });
        } finally {
            await config.update('executable', working, vscode.ConfigurationTarget.Workspace);
        }
        const scene = await open('classroom.scene');
        await until('server hover again', async () => {
            const result = await vscode.commands.executeCommand(
                'vscode.executeHoverProvider', scene.uri, position(scene, 'place desk', 7));
            return hoverText(result).includes('object desk');
        });
    },

    async 'without a working CLI the built-in hovers take over'() {
        const config = vscode.workspace.getConfiguration('isoScene');
        const working = config.get('executable');
        await config.update('executable', '/nonexistent/iso-scene', vscode.ConfigurationTarget.Workspace);
        try {
            const document = await open('desk.model');
            const hovers = await until('fallback hover', async () => {
                const result = await vscode.commands.executeCommand(
                    'vscode.executeHoverProvider', document.uri, position(document, 'box at', 1));
                return hoverText(result).includes('size (w, d, h) [material]') ? result : undefined;
            });
            assert.ok(hovers.length > 0);
        } finally {
            // And back: the server returns after the setting is restored.
            await config.update('executable', working, vscode.ConfigurationTarget.Workspace);
        }
        const scene = await open('classroom.scene');
        await until('server hover again', async () => {
            const result = await vscode.commands.executeCommand(
                'vscode.executeHoverProvider', scene.uri, position(scene, 'place desk', 7));
            return hoverText(result).includes('object desk');
        });
    }
};

async function run() {
    const extension = vscode.extensions.all.find((item) => item.packageJSON.name === 'iso-scene');
    assert.ok(extension, 'extension is loaded');
    await extension.activate();
    console.log('extension ' + extension.packageJSON.version + ' from ' + extension.extensionPath);
    const failures = [];
    for (const [name, check] of Object.entries(checks)) {
        try {
            await check();
            console.log('ok   ' + name);
        } catch (error) {
            failures.push(name);
            console.log('FAIL ' + name + '\n     ' + (error.stack || error));
        }
    }
    console.log(`${Object.keys(checks).length - failures.length} passed, ${failures.length} failed`);
    if (failures.length) throw new Error(failures.length + ' check(s) failed');
}

module.exports = { run };
