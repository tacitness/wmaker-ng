(function () {
  "use strict";

  const adapter = globalThis.WmakerBrowserAdapter;
  if (!adapter || !globalThis.chrome || !chrome.runtime) {
    return;
  }

  function snapshot() {
    return {
      schema_version: 1,
      kind: "browser.summary",
      url: location.href,
      title: document.title,
      viewport: {
        width: globalThis.innerWidth,
        height: globalThis.innerHeight
      },
      controls: adapter.collectControls(document)
    };
  }

  chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
    if (!message || message.kind !== "wmaker.collect_controls") {
      return false;
    }
    sendResponse(snapshot());
    return true;
  });

  chrome.runtime.sendMessage(snapshot()).catch(() => {
    // The service worker may not be awake yet; explicit collect requests still work.
  });
})();
