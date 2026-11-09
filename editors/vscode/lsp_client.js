'use strict';
// Минимальный клиент LSP для `iso-scene lsp`: JSON-RPC с заголовком
// Content-Length поверх stdio. Не зависит от API VS Code, поэтому
// проверяется в Node (см. test/lsp_client.test.js).
const cp = require('child_process');
const { EventEmitter } = require('events');

class LspClient extends EventEmitter {
    /**
     * @param {string[]} candidates пути к CLI по порядку; первый найденный
     *   запускается как `<cli> lsp`.
     * @param {{cwd?: string, rootUri?: string}} options
     */
    constructor(candidates, options = {}) {
        super();
        this.candidates = candidates;
        this.options = options;
        this.child = undefined;
        this.nextId = 1;
        this.pending = new Map();
        this.buffer = Buffer.alloc(0);
        this.stopping = false;
    }

    /** Запускает сервер и выполняет initialize; resolve — capabilities. */
    async start() {
        this.child = await this.spawn();
        this.child.stdout.on('data', (chunk) => this.receive(chunk));
        this.child.stderr.on('data', (chunk) => this.emit('log', chunk.toString()));
        // A server that exits early closes the pipe; `exit` rejects what waits.
        this.child.stdin.on('error', (error) => this.emit('log', 'iso-scene lsp: ' + error.message));
        this.child.on('exit', (code, signal) => {
            for (const { reject } of this.pending.values()) {
                reject(new Error('iso-scene lsp завершился'));
            }
            this.pending.clear();
            this.child = undefined;
            this.emit('exit', code, signal, this.stopping);
        });
        const result = await this.request('initialize', {
            processId: process.pid,
            rootUri: this.options.rootUri ?? null,
            capabilities: {}
        });
        this.notify('initialized', {});
        return result.capabilities || {};
    }

    spawn() {
        return new Promise((resolve, reject) => {
            const attempt = (index) => {
                if (index >= this.candidates.length) {
                    reject(new Error('iso-scene CLI не найден'));
                    return;
                }
                const child = cp.spawn(this.candidates[index], ['lsp'], {
                    cwd: this.options.cwd, windowsHide: true
                });
                child.once('spawn', () => resolve(child));
                child.once('error', (error) => {
                    if (error.code === 'ENOENT') attempt(index + 1);
                    else reject(error);
                });
            };
            attempt(0);
        });
    }

    receive(chunk) {
        this.buffer = Buffer.concat([this.buffer, chunk]);
        for (;;) {
            const end = this.buffer.indexOf('\r\n\r\n');
            if (end < 0) return;
            const header = this.buffer.subarray(0, end).toString('ascii');
            const match = /content-length:\s*(\d+)/i.exec(header);
            if (!match) {
                // Испорченный поток: дальше ему верить нельзя.
                this.buffer = Buffer.alloc(0);
                this.emit('log', 'iso-scene lsp: заголовок без Content-Length');
                return;
            }
            const length = Number(match[1]);
            const start = end + 4;
            if (this.buffer.length < start + length) return;
            const body = this.buffer.subarray(start, start + length).toString('utf8');
            this.buffer = this.buffer.subarray(start + length);
            let message;
            try {
                message = JSON.parse(body);
            } catch (error) {
                this.emit('log', 'iso-scene lsp: не JSON: ' + error.message);
                continue;
            }
            this.dispatch(message);
        }
    }

    dispatch(message) {
        if (message.id !== undefined && message.method === undefined) {
            const waiter = this.pending.get(message.id);
            if (!waiter) return;
            this.pending.delete(message.id);
            if (message.error) waiter.reject(new Error(message.error.message));
            else waiter.resolve(message.result);
        } else if (message.method !== undefined && message.id === undefined) {
            this.emit('notification', message.method, message.params);
        }
        // Запросы от сервера к клиенту iso-scene не шлёт.
    }

    send(message) {
        if (!this.child) throw new Error('iso-scene lsp не запущен');
        const body = Buffer.from(JSON.stringify(message), 'utf8');
        this.child.stdin.write(`Content-Length: ${body.length}\r\n\r\n`);
        this.child.stdin.write(body);
    }

    request(method, params) {
        const id = this.nextId++;
        return new Promise((resolve, reject) => {
            this.pending.set(id, { resolve, reject });
            try {
                this.send({ jsonrpc: '2.0', id, method, params });
            } catch (error) {
                this.pending.delete(id);
                reject(error);
            }
        });
    }

    notify(method, params) {
        if (this.child) this.send({ jsonrpc: '2.0', method, params });
    }

    /** shutdown + exit; если сервер молчит, процесс убивается через секунду. */
    async stop() {
        if (!this.child) return;
        this.stopping = true;
        const child = this.child;
        const killer = setTimeout(() => child.kill(), 1000);
        try {
            await this.request('shutdown', null);
            this.notify('exit', null);
        } catch (error) {
            child.kill();
        }
        await new Promise((resolve) => {
            if (!this.child) resolve();
            else child.once('exit', resolve);
        });
        clearTimeout(killer);
    }
}

module.exports = { LspClient };
