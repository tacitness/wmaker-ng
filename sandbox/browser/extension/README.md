# wmaker-ai Browser Adapter Extension

Prototype extension for #52. It reports visible/interactable DOM and ARIA
controls from the focused page and relays summaries to the native messaging
host named `wmaker_ai_browser`.

Files:

- `manifest.json` declares the MV3 extension, content scripts, service worker,
  `nativeMessaging`, and active-tab permissions.
- `dom-controls.js` extracts visible controls with role, label/name, enabled
  state, checked/selected state, viewport bounds, and stable page-version
  handles.
- `content-script.js` answers `wmaker.collect_controls` requests and pushes a
  page summary when loaded.
- `service-worker.js` forwards summaries to the #48 native messaging host.
- `dom-controls.test.js` is a dependency-free Node fixture test for the
  extraction rules.

Password inputs and common secret fields are redacted by design: the adapter
reports type, label, bounds, and action capability, but never field values.

Run the fixture test:

```bash
node sandbox/browser/extension/dom-controls.test.js
```
