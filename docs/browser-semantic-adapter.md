# Browser semantic adapter design

Issue: #36

## Decision

Build a wmaker-owned browser semantic adapter using a minimal Chromium/Brave
extension plus a local native-messaging host. Treat BrowserMCP-style tools as
optional experiments, not a required v1 dependency. Avoid Chrome DevTools
Protocol remote-debugging ports as the default production lane.

This keeps the default agent observation stack object-first:

```text
ai-mcp observe
  -> desktop/window scene
  -> browser adapter summary when active window is a supported browser
  -> optional crop/screenshot fallback only when requested
```

## Why This Shape

Chrome/Chromium native messaging lets an extension exchange JSON messages with
a native application over stdin/stdout. Chrome starts the host as a separate
process, and the host must be registered explicitly for the extension. That is
a good fit for `wmaker-ai`: the browser remains sandboxed, while the native
host can bridge semantic browser state back to `ai-mcp` over a local, auditable
IPC surface.

BrowserMCP is useful prior art: it uses a browser extension plus an MCP server
to automate an existing browser profile locally. It already exposes browser
actions such as navigation, clicking, typing, screenshots, and accessibility
snapshots. The tradeoff is ownership: depending on it would put a core v1
semantic lane behind a third-party extension/server lifecycle.

CDP is powerful, but a remote debugging port is the wrong default boundary for
daily-driver browser sessions. It is appropriate for disposable test sessions,
not for the normal model-facing observation path.

Pure screen/OCR remains a fallback. It is too lossy and expensive to be the
primary semantic browser lane because it reconstructs information the browser
already knows.

## Target Data Contract

The adapter should report:

- active tab URL and title
- tab list for the current browser profile/window
- visible DOM/ARIA controls with role, label/name, enabled state, checked or
  selected state where available, and viewport bounds
- forms, buttons, links, selected text, and scroll position
- stable element handles for `click`, `type`, `select`, `focus`, and
  `scroll_into_view`
- optional element-crop references for vision fallback, without embedding
  pixels in the default observation

The `observe` MCP tool should remain the preferred model-facing entry point.
When the focused window has a browser adapter, `observe` can include a compact
browser summary and a list of semantic element handles.

## Architecture

```text
Brave/Chromium extension
  content script:
    - scans visible/interactable DOM
    - computes labels from DOM/ARIA attributes
    - captures viewport-relative bounds
    - executes element actions by stable handle
  service worker:
    - owns native messaging connection
    - scopes tab permissions
    - relays requests/responses

Native messaging host
  - launched by browser for the extension
  - speaks length-prefixed JSON on stdio
  - forwards structured summaries/actions to ai-mcp over a local Unix socket

ai-mcp
  - exposes browser summaries through observe
  - exposes explicit browser action tools after adapter handshake
  - keeps screenshot/dirty-region tools opt-in
```

## Approach Comparison

| Approach | Strengths | Risks | Decision |
| --- | --- | --- | --- |
| Extension + native messaging | Owned boundary, no remote debug port, works with real profile when explicitly installed, can provide DOM/ARIA handles | Requires extension packaging, native host registration, permission UX | Preferred native lane |
| BrowserMCP | Existing extension/server, useful comparison target, likely quick to try in disposable sessions | Third-party dependency, profile mutation, version/lifecycle risk | Optional spike only (#35) |
| Chrome DevTools Protocol | Rich browser/debug data, strong for test automation and diagnostics | Remote debugging port, too broad for daily-driver sessions | Disposable test lane only |
| Pure screen/OCR | Browser-agnostic fallback, works when no extension exists | Loses DOM semantics, higher model/context cost, fragile labels/bounds | Fallback only |

## Security Model

- Default-off: no browser semantic adapter is active unless the user installs
  the extension/native host for that profile.
- Profile boundary: seeded/disposable profiles are preferred for automation.
  Personal/live profiles require explicit opt-in.
- Host allowlist: the native messaging manifest must allow only the wmaker
  extension ID.
- Origin scope: extension permissions should start narrow and request per-site
  access where possible.
- No secrets in payloads: adapter payloads should avoid cookies, local storage,
  authorization headers, password fields, and hidden form values.
- Sensitive fields: password inputs are reported only as redacted controls with
  type/label/bounds/action capability, not values.
- Local IPC: native host to `ai-mcp` should use a Unix socket under the runtime
  directory with owner-only permissions.
- Auditability: adapter messages should be schema-versioned and logged at
  metadata level without page content by default.

## Implementation Tickets

- #52 Extension manifest and content-script prototype for visible DOM/ARIA
  controls.
- #48 Native messaging host registration and local Unix-socket bridge.
- #49 `ai-mcp` browser summary/action schemas exposed through `observe`.
- #51 Disposable-profile install/smoke test for Brave/Chromium in
  `wmaker-ai-browser`.

## Native Messaging Bridge

The #48 bridge lands as an `ai-mcp browser-host` subcommand so packaging only
has to ship one executable. Browser stdio uses Chromium native messaging
framing: four little-endian length bytes followed by a JSON document. The host
validates the adapter payload and forwards it to the running `ai-mcp` server
over newline-delimited JSON on an owner-only Unix socket.

Default socket:

```text
$XDG_RUNTIME_DIR/wmaker-ai/browser-adapter.sock
```

Override it with `WMAKER_AI_BROWSER_SOCKET` on both sides. Set
`WMAKER_AI_BROWSER_SOCKET=0` only when starting `ai-mcp` without the browser
adapter socket.

Every forwarded message uses schema version `1`:

```json
{
  "schema_version": 1,
  "kind": "browser.summary",
  "extension_id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "tab_id": 1,
  "payload": {
    "url": "https://example.com/",
    "title": "Example Domain",
    "controls": []
  }
}
```

The host requires an explicit extension allowlist. Use one of:

```bash
ai-mcp browser-host --allow-extension-id aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
WMAKER_AI_BROWSER_ALLOWED_EXTENSION_IDS=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa ai-mcp browser-host
```

`--allow-any-extension` exists only for disposable smoke tests. It should not be
used with a live browser profile.

The manifest and wrapper templates live under
`sandbox/browser/native-messaging/`:

- `wmaker-ai-browser-host.sh.in`
- `wmaker_ai_browser.json.in`

Render `__AI_MCP_PATH__` in the wrapper to the installed `ai-mcp` executable,
render `__EXTENSION_ID__` to the packed extension ID, make the wrapper
executable, then render `__BROWSER_HOST_PATH__` in the manifest to that wrapper.
Chrome/Chromium native messaging manifests cannot pass command arguments
directly, so the wrapper execs `ai-mcp browser-host`.

Chrome/Chromium search paths are:

```text
~/.config/google-chrome/NativeMessagingHosts/wmaker_ai_browser.json
~/.config/chromium/NativeMessagingHosts/wmaker_ai_browser.json
```

Brave uses:

```text
~/.config/BraveSoftware/Brave-Browser/NativeMessagingHosts/wmaker_ai_browser.json
```

## References

- BrowserMCP project: https://github.com/BrowserMCP/mcp
- BrowserMCP overview: https://browsermcp.io/
- Chrome native messaging documentation:
  https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging
- Chrome accessibility tree reference:
  https://developer.chrome.com/docs/devtools/accessibility/reference
