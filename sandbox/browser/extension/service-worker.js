"use strict";

const HOST_NAME = "wmaker_ai_browser";
let nativePort = null;

function extensionId() {
  return chrome.runtime.id;
}

function connectNative() {
  if (nativePort) {
    return nativePort;
  }
  nativePort = chrome.runtime.connectNative(HOST_NAME);
  nativePort.onDisconnect.addListener(() => {
    nativePort = null;
  });
  return nativePort;
}

function forwardSummary(summary, sender) {
  const tabId = sender && sender.tab ? sender.tab.id : summary.tab_id;
  connectNative().postMessage({
    schema_version: 1,
    kind: "browser.summary",
    extension_id: extensionId(),
    tab_id: tabId,
    payload: {
      url: summary.url,
      title: summary.title,
      viewport: summary.viewport,
      controls: summary.controls || []
    }
  });
}

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (!message || message.kind !== "browser.summary") {
    return false;
  }
  try {
    forwardSummary(message, sender);
    sendResponse({ ok: true });
  } catch (error) {
    sendResponse({ ok: false, error: String(error && error.message ? error.message : error) });
  }
  return true;
});
