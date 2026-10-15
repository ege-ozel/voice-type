importScripts('config.js');

const BRIDGE = 'http://127.0.0.1:47831';
const sleep = ms => new Promise(r => setTimeout(r, ms));

async function bridgeRequest(path, body, timeout = 7000) {
  let response;
  try {
    response = await fetch(BRIDGE + path, {
      method: body ? 'POST' : 'GET',
      headers: {'Authorization': 'Bearer ' + BRIDGE_TOKEN, 'Content-Type': 'application/json'},
      body: body ? JSON.stringify(body) : undefined,
      signal: AbortSignal.timeout(timeout)
    });
  } catch (_) {
    throw new Error('Köprüye ulaşılamadı. ChatGPT-Dikte.exe veya start.bat açık olmalı.');
  }
  const data = await response.json();
  if (!response.ok) throw new Error(data.error || 'İstek başarısız.');
  return data;
}

async function poll(client) {
  try {
    const version = chrome.runtime.getManifest().version;
    return await bridgeRequest('/poll?client=' + encodeURIComponent(client) + '&v=' + version, null, 30000);
  } catch (error) {
    // Köprü kapalıyken içerik betiği sıkı döngüye girmesin; bekleme burada (arka plan sekmesi
    // zamanlayıcıları kısıldığı için sayfada değil) yapılır.
    await sleep(3000);
    throw error;
  }
}

// Tanı izi: GET ile gönderilir, böylece POST yolu bozuk olsa bile köprü kaydına düşer.
function trace(client, msg) {
  return bridgeRequest('/trace?client=' + encodeURIComponent(client || 'bg') + '&msg=' + encodeURIComponent(msg));
}

chrome.runtime.onMessage.addListener((message, sender, respond) => {
  if (sender.id !== chrome.runtime.id) return;
  let action;
  if (message.type === 'poll') action = poll(message.client);
  else if (message.type === 'report') {
    const {client, session, event, text, error} = message;
    action = bridgeRequest('/report', {client, session, event, text, error}).catch(err => {
      trace(client, 'bildirim gönderilemedi (' + event + '): ' + err.message).catch(() => {});
      throw err;
    });
  } else if (message.type === 'trace') {
    action = trace(message.client, message.msg);
  } else if (message.type === 'status') action = bridgeRequest('/status');
  else action = Promise.reject(new Error('Geçersiz işlem.'));
  action.then(data => respond({ok: true, ...data})).catch(error => respond({ok: false, error: error.message}));
  return true;
});

// Sekme bağlama otomatik: ChatGPT sekmesi yoksa sabitlenmiş, arka planda bir tane aç;
// Chrome sekmeyi uyuttuysa yeniden yükle. Kullanıcının elle bir şey yapması gerekmez.
async function ensureChatGPTTab() {
  const tabs = await chrome.tabs.query({url: 'https://chatgpt.com/*'});
  if (!tabs.length) {
    await chrome.tabs.create({url: 'https://chatgpt.com/', pinned: true, active: false});
    return;
  }
  if (tabs.every(tab => tab.discarded)) await chrome.tabs.reload(tabs[0].id);
}

async function injectIntoOpenTabs() {
  // Eklenti kurulduğunda/güncellendiğinde açık sekmelere betiği hemen ekle (yenileme gerekmesin).
  const tabs = await chrome.tabs.query({url: 'https://chatgpt.com/*'});
  for (const tab of tabs) {
    if (!tab.discarded) chrome.scripting.executeScript({target: {tabId: tab.id}, files: ['content.js']}).catch(() => {});
  }
}

chrome.alarms.create('ensure-tab', {periodInMinutes: 1});
// Paketlenmemiş eklenti diskteki dosyalar değişince kendiliğinden yenilenmez. manifest.json'daki
// sürüm diskte artmışsa eklenti kendini yeniden yükler; güncellemelerde elle ↻ gerekmez.
async function reloadIfUpdated() {
  const onDisk = await (await fetch(chrome.runtime.getURL('manifest.json'), {cache: 'no-store'})).json();
  if (onDisk.version !== chrome.runtime.getManifest().version) chrome.runtime.reload();
}

chrome.alarms.onAlarm.addListener(alarm => {
  if (alarm.name !== 'ensure-tab') return;
  ensureChatGPTTab().catch(() => {});
  reloadIfUpdated().catch(() => {});
});
chrome.runtime.onStartup.addListener(() => ensureChatGPTTab().catch(() => {}));
chrome.runtime.onInstalled.addListener(async () => {
  await injectIntoOpenTabs().catch(() => {});
  await ensureChatGPTTab().catch(() => {});
});
