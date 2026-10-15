const status = document.getElementById('status');
const phases = {idle: 'Hazır', starting: 'Başlatılıyor…', listening: 'Dinliyor', transcribing: 'Yazıya dökülüyor…'};
chrome.runtime.sendMessage({type: 'status'}).then(result => {
  if (!result.ok) throw new Error(result.error);
  status.textContent = result.connected
    ? '● ' + phases[result.phase]
    : 'Köprü açık, ama ChatGPT sekmesi bağlı değil. chatgpt.com sekmesini açın veya yenileyin.';
}).catch(error => { status.textContent = error.message; });
