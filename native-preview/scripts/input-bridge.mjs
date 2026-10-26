// Input only: load serve-sim's pinned native HID addon without starting capture or encoders.
import { createServer } from 'node:http';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { realpathSync } from 'node:fs';

const [device, executable] = process.argv.slice(2);
if (!device || !executable) throw new Error('Usage: node input-bridge.mjs <udid> <serve-sim-executable>');
const packageRoot = dirname(dirname(realpathSync(executable)));
const addon = createRequire(import.meta.url)(join(packageRoot, 'dist/native/serve-sim-native.node'));
const hid = new addon.SimHID(device, join(packageRoot, 'dist/simduo/serve-sim-duo-hid'));
const inputURL = 'http://127.0.0.1:43220/input';
let pending = Promise.resolve();
let received = 0;
let lastPacket = null;

const server = createServer(async (request, response) => {
  if (request.method === 'GET' && request.url === '/api') {
    response.writeHead(200, { 'Content-Type': 'application/json' });
    response.end(JSON.stringify({ pid: process.pid, device, inputURL, mode: 'input-only', received, lastPacket }));
    return;
  }
  if (request.method !== 'POST' || request.url !== '/input') {
    response.writeHead(404); response.end(); return;
  }
  try {
    const chunks = [];
    let length = 0;
    for await (const chunk of request) {
      length += chunk.length;
      if (length > 4096) throw new Error('Input packet too large');
      chunks.push(chunk);
    }
    const packet = Buffer.concat(chunks);
    const value = JSON.parse(packet.subarray(1).toString('utf8'));
    received += 1; lastPacket = {tag:packet[0],...value};
    let operation;
    if (packet[0] === 3 && ['begin', 'move', 'end'].includes(value.type)
        && [value.x, value.y].every(n => Number.isFinite(n) && n >= 0 && n <= 1)
        && [value.width, value.height].every(n => Number.isFinite(n) && n > 0 && n <= 10000)) {
      operation = () => hid.touch(value.type, value.x, value.y, value.width, value.height, 0);
    } else if (packet[0] === 6 && ['down', 'up'].includes(value.type)
        && Number.isInteger(value.usage) && value.usage >= 0 && value.usage <= 255) {
      operation = () => hid.key(value.type, value.usage);
    } else {
      throw new Error('Invalid input packet');
    }
    const current = pending.then(operation);
    pending = current.catch(() => {});
    await current; // Acknowledge native injection before the next focus/input operation.
    response.writeHead(204); response.end();
  } catch (error) {
    response.writeHead(400, { 'Content-Type': 'application/json' });
    response.end(JSON.stringify({ error: String(error.message ?? error) }));
  }
});
server.listen(43220, '127.0.0.1', () => console.log(`Rndr input-only bridge: ${inputURL}`));
server.on('error', error => { console.error(error.message); process.exit(1); });
for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, async () => {
  server.close(); server.closeAllConnections();
  await hid.close();
  process.exit(0);
});
