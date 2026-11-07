'use strict';
// VSCODE=/usr/share/code/code xvfb-run -a node editors/vscode/test/vscode/run.js [--vsix file.vsix]
// Starts the installed VS Code (`code`, or $VSCODE) in a throwaway profile on
// a copy of examples/ and runs index.js inside it. Without --vsix the
// extension is loaded from the source folder; with it, the packaged
// extension is installed into the profile first. Needs a display (use
// xvfb-run on a headless machine) and a built CLI.
const cp = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');

const repo = path.resolve(__dirname, '..', '..', '..', '..');
const extension = path.resolve(__dirname, '..', '..');
// VSCODE: the editor binary that runs the tests (the Electron executable,
// e.g. /usr/share/code/code; the `code` wrapper may hand off to a running
// window). VSCODE_CLI: the `code` command, used to install a .vsix.
const code = process.env.VSCODE || 'code';
const codeCli = process.env.VSCODE_CLI || 'code';
const cli = process.env.ISO_SCENE || path.join(repo, 'target', 'release', 'iso-scene');
const vsix = process.argv.includes('--vsix') ? path.resolve(process.argv[process.argv.indexOf('--vsix') + 1]) : null;

const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'iso-scene-vscode-it-'));
const workspace = path.join(temp, 'workspace');
fs.cpSync(path.join(repo, 'examples'), workspace, { recursive: true });
fs.writeFileSync(path.join(workspace, 'room.walls'), 'version 1;\nlayout {\n"###"\n"#.#"\n"###"\n}\n');
fs.mkdirSync(path.join(workspace, '.vscode'));
fs.writeFileSync(path.join(workspace, '.vscode', 'settings.json'),
    JSON.stringify({ 'isoScene.executable': cli, 'security.workspace.trust.enabled': false }, null, 2));

const profile = ['--user-data-dir', path.join(temp, 'user'), '--extensions-dir', path.join(temp, 'extensions')];
if (vsix) {
    const install = cp.spawnSync(codeCli, [...profile, '--install-extension', vsix], { encoding: 'utf8', timeout: 120000 });
    process.stdout.write(install.stdout + install.stderr);
    if (install.status !== 0) process.exit(install.status || 1);
}
const args = [
    ...profile, '--disable-gpu', '--disable-workspace-trust', '--skip-welcome', '--skip-release-notes',
    '--new-window', '--wait',
    // The tests need an extension development path; with --vsix it points
    // at an empty folder so only the installed package is loaded.
    `--extensionDevelopmentPath=${vsix ? path.join(temp, 'none') : extension}`,
    `--extensionTestsPath=${path.join(__dirname, 'index.js')}`,
    workspace
];
if (vsix) fs.mkdirSync(path.join(temp, 'none'));
if (vsix) fs.writeFileSync(path.join(temp, 'none', 'package.json'),
    JSON.stringify({ name: 'none', version: '0.0.0', engines: { vscode: '*' } }));
const result = cp.spawnSync(code, args, { stdio: 'inherit' });
fs.rmSync(temp, { recursive: true, force: true });
process.exit(result.status ?? 1);
