import { invoke } from '@tauri-apps/api/core';
import './style.css';

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
type Observation = { connected: boolean; revision: number; error_code: string | null };
type Project = { project: { project_id: string; name: string; root_path: string; environment_generation: number }; environment_reconcile_required: boolean; initialization_required: boolean };
type Operation = { project_id: string; operation_id: string; result: { state: string; error?: Json } };
type HostStatus = { observed: string; runtime_version: string | null };
type Session = { session_id: string; state: string; queue_depth: number };
type Environment = { ready: boolean; reconcile_required: boolean; environment_generation: number; python_executable: string | null };

async function call<T>(method: string, params: Record<string, Json> = {}): Promise<T> {
  const response = await invoke<{ protocol: number; ok: boolean; result?: T; error?: Json }>('host_call', { method, params });
  if (response.protocol !== 1) throw { summary: 'Hostとのプロトコルが一致しません。' };
  if (!response.ok) throw response.error;
  return response.result as T;
}
function el<K extends keyof HTMLElementTagNameMap>(tag: K, text = '', className = ''): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag); node.textContent = text; node.className = className; return node;
}
function button(label: string, action: () => void, className = ''): HTMLButtonElement {
  const node = el('button', label, className); node.type = 'button'; node.addEventListener('click', action); return node;
}
function errorText(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error && typeof error === 'object') {
    const e = error as { code?: string; summary?: string; action?: string };
    return [e.code, e.summary, e.action].filter(Boolean).join(' · ') || '応答を読み取れませんでした。';
  }
  return 'Hostへ接続できません。起動状態を確認してください。';
}
const root = document.querySelector<HTMLDivElement>('#app')!;
const sidebar = el('aside');
sidebar.append(el('div', 'CoKernel', 'brand'), el('p', 'Python workspace', 'muted'));
const content = el('main'); const header = el('header');
const title = el('h1', 'Projects'); const connection = el('span', 'Hostへ接続中', 'badge');
const notice = el('div', '', 'notice'); notice.setAttribute('role', 'status');
header.append(title, connection, button('Runtimeを起動', () => { void call('runtime.start').then(() => refresh()).catch((error: unknown) => { notice.textContent = errorText(error); }); }, 'secondary'), button('更新', () => { void refresh(); }, 'secondary'));
const home = el('section', '', 'summary'); const projects = el('section'); const detail = el('section');
const formSection = el('section', '', 'panel');
const form = el('form');
const mode = el('select'); mode.setAttribute('aria-label', 'Project操作');
for (const [value, label] of [['create', '新しいProject'], ['register', '既存Projectを登録']]) {
  const option = el('option', label); option.value = value; mode.append(option);
}
const input = el('input'); input.required = true; input.maxLength = 1024;
input.placeholder = 'Project名'; input.setAttribute('aria-label', 'Project名またはLinux内のパス');
const help = el('p', 'ProjectごとにPythonパッケージを管理します。', 'muted');
mode.addEventListener('change', () => {
  input.placeholder = mode.value === 'create' ? 'Project名' : '/var/lib/cokernel/projects/…';
  help.textContent = mode.value === 'create' ? 'ProjectごとにPythonパッケージを管理します。' : '管理環境内の既存Projectを登録します。Windowsファイルの取り込みは別の操作です。';
});
const submit = el('button', '作成'); submit.type = 'submit';
mode.addEventListener('change', () => { submit.textContent = mode.value === 'create' ? '作成' : '登録'; });
form.append(mode, input, submit); formSection.append(el('h2', 'Projectを追加'), form, help);
const operationStatus = el('p'); operationStatus.setAttribute('role', 'status'); formSection.append(operationStatus);
content.append(header, notice, home, projects, detail, formSection);
sidebar.append(button('Projects', () => { selected = undefined; selectionVersion++; detail.replaceChildren(); title.textContent = 'Projects'; projects.hidden = false; }, 'nav active'));
sidebar.append(el('p', 'ウィンドウを閉じても、実行中のSessionは継続します。', 'sidebar-note'));
root.append(sidebar, content);

let selected: Project | undefined;
let selectionVersion = 0;
let refreshBusy = false;
let detailBusy = false;
let pendingOperation: Operation | undefined;
let operationBusy = false;
let mutationPending = false;
let connected = false;
let lastRevision = -1;

function renderProjects(records: Project[]) {
  projects.replaceChildren(el('h2', `Projects · ${records.length}`));
  const list = el('div', '', 'project-list');
  for (const record of records) {
    const card = button('', () => { selected = record; selectionVersion++; projects.hidden = true; title.textContent = record.project.name; detail.replaceChildren(el('p', '読み込み中…')); void refreshDetail(); }, 'project');
    card.append(el('strong', record.project.name), el('span', record.project.root_path, 'path'), el('span', record.initialization_required ? '初期化が必要' : record.environment_reconcile_required ? '環境の確認が必要' : `環境 generation ${record.project.environment_generation}`, 'muted'));
    list.append(card);
  }
  if (!records.length) list.append(el('p', '最初のProjectを作成してください。', 'muted'));
  projects.append(list);
}
async function refreshDetail() {
  if (!selected || detailBusy) return;
  const version = selectionVersion; const project_id = selected.project.project_id;
  detailBusy = true;
  try {
    const results = await Promise.allSettled([
      call<Environment>('environment.get_status', { project_id }),
      call<{ total: number; paths: string[] }>('notebooks.list', { project_id, limit: 512 }),
      call<Session[]>('sessions.list', { project_id }),
    ]);
    if (version !== selectionVersion) return;
    detail.replaceChildren();
    const labels = ['Python環境', 'Notebooks', 'Sessions'];
    results.forEach((result, index) => {
      const panel = el('article', '', 'panel'); panel.append(el('h2', labels[index]));
      if (result.status === 'rejected') panel.append(el('p', errorText(result.reason), 'error'));
      else if (index === 0) {
        const env = result.value as Environment;
        panel.append(el('p', env.reconcile_required ? '環境の確認が必要です' : env.ready ? '準備完了' : '環境の準備が必要です'), el('p', env.python_executable ?? 'Python未準備', 'path'), el('p', `環境 generation ${env.environment_generation}`, 'muted'));
      } else if (index === 1) {
        const notebooks = result.value as { total: number; paths: string[] };
        for (const path of notebooks.paths) panel.append(el('p', path));
        if (!notebooks.total) panel.append(el('p', 'Notebookはまだありません。', 'muted'));
        if (notebooks.total > notebooks.paths.length) panel.append(el('p', `先頭${notebooks.paths.length}件 / 全${notebooks.total}件`, 'muted'));
      } else {
        const sessions = result.value as Session[];
        for (const session of sessions) panel.append(el('p', `${session.state} · 待機 ${session.queue_depth}${session.state === 'STALE_ENVIRONMENT' ? ' · 環境変更あり（メモリは保持）' : ''}`), el('p', session.session_id, 'path'));
        if (!sessions.length) panel.append(el('p', '実行中のSessionはありません。', 'muted'));
      }
      detail.append(panel);
    });
  } finally {
    detailBusy = false;
    if (version !== selectionVersion && selected) void refreshDetail();
  }
}
async function refresh() {
  if (refreshBusy) return;
  refreshBusy = true;
  try {
    const results = await Promise.allSettled([call<HostStatus>('runtime.get_status'), call<Project[]>('projects.list'), call<{ state: string }>('remote_access.get_status'), call<{configured: boolean; tunnel_id: string | null}>('remote_access.get_configuration')]);
    home.replaceChildren(); const errors: string[] = [];
    results.forEach((result, index) => {
      if (result.status === 'rejected') { errors.push(errorText(result.reason)); return; }
      if (index === 0) {
        const status = result.value as HostStatus;
        const card = el('article', '', 'stat'); card.append(el('span', 'Runtime', 'muted'), el('strong', status.observed), el('small', status.runtime_version ?? '未起動')); home.append(card);
      } else if (index === 1) renderProjects(result.value as Project[]);
      else if (index === 2) { const card = el('article', '', 'stat'); card.append(el('span', 'Remote AI', 'muted'), el('strong', (result.value as { state: string }).state)); home.append(card); }
      else { const configuration = result.value as {configured: boolean; tunnel_id: string | null}; configuredTunnel.textContent = configuration.configured ? `コネクタで選択するTunnel ID: ${configuration.tunnel_id}` : 'APIキー・接続設定は未登録です。'; }
    });
    notice.textContent = errors.join('\n');
    if (selected) await refreshDetail();
  } finally { refreshBusy = false; }
}
form.addEventListener('submit', (event) => {
  event.preventDefault(); if (mutationPending || pendingOperation || !connected) return;
  mutationPending = true; submit.disabled = true; operationStatus.textContent = 'Projectを準備しています…';
  const action = mode.value;
  void call<Operation>(`projects.${action}`, action === 'create' ? { name: input.value } : { path: input.value })
    .then((receipt) => { pendingOperation = receipt; operationStatus.textContent = 'Python環境を準備しています…'; void pollOperation(); })
    .catch((error: unknown) => { operationStatus.textContent = errorText(error); })
    .finally(() => { mutationPending = false; submit.disabled = !connected || !!pendingOperation; });
});
async function pollOperation() {
  if (!pendingOperation || operationBusy || !connected) return;
  operationBusy = true;
  try {
    const op = await call<Operation>('operations.get', { project_id: pendingOperation.project_id, operation_id: pendingOperation.operation_id });
    operationStatus.textContent = `環境の準備: ${op.result.state}`;
    if (op.result.state === 'SUCCEEDED' || op.result.state === 'FAILED') {
      if (op.result.error) operationStatus.textContent = errorText(op.result.error);
      pendingOperation = undefined; submit.disabled = !connected || mutationPending;
      await refresh();
    }
  } catch (error) { operationStatus.textContent = errorText(error); }
  finally { operationBusy = false; }
}
async function tick() {
  try {
    const observation = await invoke<Observation>('host_observation');
    connected = observation.connected;
    connection.textContent = connected ? 'Hostに接続済み' : 'Hostに未接続 · 自動再接続中';
    connection.classList.toggle('offline', !connected);
    submit.disabled = !connected || mutationPending || !!pendingOperation;
    configure.disabled = !connected || setupPending;
    if (!connected) notice.textContent = 'Hostの状態を確認できません。表示は最後に取得した情報です。';
    if (connected && observation.revision !== lastRevision && !refreshBusy) {
      lastRevision = observation.revision; void refresh();
    }
    void pollOperation();
  } catch (error) { connection.textContent = 'Desktop接続エラー'; notice.textContent = errorText(error); }
  finally { window.setTimeout(() => { void tick(); }, 1000); }
}
const setup = el('section', '', 'panel');
setup.append(el('h2', 'AIに接続する'), el('p', 'Tunnel IDと実行用APIキーを保存すると接続を開始します。キーはWindowsの専用画面へ入力します。'));
const configuredTunnel = el('p', '接続設定を確認しています…', 'path'); setup.append(configuredTunnel);
const setupStatus = el('p'); setupStatus.setAttribute('role','status');
let setupPending=false;
const configure=button('APIキー・接続設定',()=>{
  if(setupPending || !connected) return;
  setupPending=true; configure.disabled=true;
  void invoke<{cancelled?:boolean}>('configure_remote').then(result=>{
    setupStatus.textContent=result.cancelled ? '設定をキャンセルしました。' : '設定を保存し、接続を開始しました。Remote AIの状態を確認してください。';
    void refresh();
  }).catch((error:unknown)=>{setupStatus.textContent=errorText(error);})
  .finally(()=>{setupPending=false;configure.disabled=false;});
});
setup.append(configure,button('接続を再開',()=>{void call('remote_access.start').then(()=>refresh()).catch((error:unknown)=>{setupStatus.textContent=errorText(error);});}),button('AI接続を停止',()=>{void call('remote_access.stop').then(()=>refresh()).catch((error:unknown)=>{setupStatus.textContent=errorText(error);});}),setupStatus);
setup.append(el('p','ChatGPT側でこのトンネルをコネクタとして追加してください。接続後はAIがProject作成、依存パッケージ追加、ノートブック作成・実行を行えます。','muted'));
content.insertBefore(setup,projects);
void tick();
