// Exercise the release exe before packaging it. Only this isolated CI process
// receives the WebView2 debugging port. No Host, Vite server or GPU is required.
import { spawn } from 'node:child_process';
import { mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

const exe = resolve('target/release/cokernel-desktop.exe');
const profile = await mkdtemp(join(tmpdir(), 'cokernel-webview-'));
const port = 19427;
const app = spawn(exe, [], { env: { ...process.env,
  WEBVIEW2_USER_DATA_FOLDER: profile,
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=' + port,
} });
let pageUrl = '';
async function rendered(wsUrl) {
  const socket = new WebSocket(wsUrl);
  try {
    return await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(Error('WebView inspection timed out')), 3000);
      socket.onopen = () => socket.send(JSON.stringify({ id: 1, method: 'Runtime.evaluate',
        params: { expression: "document.querySelector('#app main h1')?.textContent === 'Projects' && [...document.querySelectorAll('#app button')].some(b => b.textContent === 'APIキー・接続設定')", returnByValue: true } }));
      socket.onerror = () => { clearTimeout(timer); reject(Error('WebView inspection failed')); };
      socket.onmessage = ({data}) => {
        const reply = JSON.parse(data);
        if (reply.id === 1) { clearTimeout(timer); resolve(reply.result?.result?.value === true); }
      };
    });
  } finally { socket.close(); }
}
try {
  const deadline = Date.now() + 60000;
  while (Date.now() < deadline) {
    if (app.exitCode !== null) throw Error('Desktop exited: ' + app.exitCode);
    const targets = await fetch('http://127.0.0.1:' + port + '/json/list', {signal: AbortSignal.timeout(1500)})
      .then(r => r.json()).catch(() => []);
    for (const target of targets.filter(t => t.type === 'page')) {
      pageUrl = target.url;
      if (/^https?:\/\/tauri\.localhost(?:\/|$)/.test(pageUrl) && await rendered(target.webSocketDebuggerUrl)) {
        console.log('Bundled Projects UI and API key control rendered at ' + pageUrl);
        process.exitCode = 0;
        break;
      }
    }
    if (process.exitCode === 0) break;
    await delay(300);
  }
  if (process.exitCode !== 0) throw Error('Bundled Projects UI did not render. Last URL: ' + pageUrl);
} finally { app.kill(); }
