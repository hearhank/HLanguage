#!/usr/bin/env node

/**
 * Simple LSP client test script
 * Tests if hc-lsp server can start and respond to basic LSP requests
 */

const { spawn } = require('child_process');
const path = require('path');

// Path to hc-lsp binary
const hcLspPath = path.join(__dirname, '..', 'target', 'release', 'hc-lsp.exe');

console.log('Testing H Language LSP Server...');
console.log('LSP binary:', hcLspPath);
console.log();

// Start LSP server
const server = spawn(hcLspPath, [], {
    stdio: ['pipe', 'pipe', 'pipe']
});

let buffer = '';

// Handle server output
server.stdout.on('data', (data) => {
    buffer += data.toString();
    
    // Try to parse LSP responses
    const lines = buffer.split('\r\n');
    for (const line of lines) {
        if (line.startsWith('Content-Length:')) {
            // LSP header
            continue;
        }
        if (line.trim() === '') {
            // Empty line, next line should be content
            continue;
        }
        if (line.startsWith('{')) {
            // JSON content
            try {
                const response = JSON.parse(line);
                console.log('Received response:', JSON.stringify(response, null, 2));
            } catch (e) {
                // Not valid JSON, ignore
            }
        }
    }
});

server.stderr.on('data', (data) => {
    console.error('Server error:', data.toString());
});

server.on('close', (code) => {
    console.log('Server exited with code:', code);
});

// Send initialize request
function sendRequest(method, params, id) {
    const request = {
        jsonrpc: '2.0',
        id: id,
        method: method,
        params: params
    };
    
    const content = JSON.stringify(request);
    const header = `Content-Length: ${content.length}\r\n\r\n`;
    
    server.stdin.write(header + content);
}

// Wait a bit for server to start, then send requests
setTimeout(() => {
    console.log('Sending initialize request...');
    sendRequest('initialize', {
        processId: process.pid,
        capabilities: {},
        rootUri: null
    }, 1);
}, 1000);

// Send shutdown request after 3 seconds
setTimeout(() => {
    console.log('Sending shutdown request...');
    sendRequest('shutdown', null, 2);
}, 3000);

// Exit after 4 seconds
setTimeout(() => {
    console.log('Sending exit notification...');
    sendRequest('exit', null);
    server.kill();
    console.log('Test completed!');
    process.exit(0);
}, 4000);
