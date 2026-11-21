// Evaluated by native code only for an explicit close/quit request.
// Use the same cancellable event as browser navigation; do not synthesize a save.
(() => {
 const requestId = __REQUEST_ID__;
 const event = new Event('beforeunload', {cancelable: true});
 window.dispatchEvent(event);
 const allowed = document.documentElement.dataset.unCloseGuardReady === 'true' && !event.defaultPrevented;
 const root = document.documentElement;
 const wasInert = root.__unCloseReview?.wasInert ?? root.inert;
 root.__unCloseReview = {requestId, wasInert};
 const release = () => {
  if (root.__unCloseReview?.requestId !== requestId) return false;
  root.inert = wasInert;
  delete root.__unCloseReview;
  return true;
 };
 if (allowed) root.inert = true;
 else {
  release();
  window.dispatchEvent(new Event('un-avatar-close-blocked'));
 }
 Promise.resolve().then(() => window.__TAURI_INTERNALS__.invoke('resolve_supervisor_close', {requestId, allowed}))
  .then(closing => { if (!closing) release(); })
  .catch(error => {
   if (!allowed || !release()) return;
   console.error('Native close review failed', error);
   window.dispatchEvent(new Event('un-avatar-close-failed'));
  });
})();
