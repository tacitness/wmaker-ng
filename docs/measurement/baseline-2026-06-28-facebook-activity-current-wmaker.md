# 2026-06-28 Facebook Activity Baseline

This is baseline 2 for the current Window Maker / OpenClaw screen-driving path:
a bounded, repeatable browser activity scenario rather than an idle screenshot
sample.

## Environment

- Host: `tacitbot`
- Window manager: `WindowMaker 0.96.0`
- Current binary: `/usr/local/bin/wmaker.real`
- Display: `:9`
- Resolution: `1920x1080`
- Browser: existing Brave session on X11
- Driver stack: `wmctrl`, `xdotool`, and full-screen `ImageMagick import`
- Duration: `60s`
- Raw screenshots stored: `false`
- Feed post text stored: `false`
- Private messages opened: `false`

## S3 Dataset

```text
s3://tacitsoft-agent-artifacts-494111853453-us-west-2/wmaker-ng/browser-activity-baselines/20260628T133852Z-tacitbot-9/
```

Files:

- `summary.json`
- `actions.jsonl`
- `captures.jsonl`
- `process-samples.jsonl`
- `display.txt`
- `windows.txt`
- `report.md`

## Scenario

The run used the logged-in browser session already open on display `:9`.

1. Activate the existing Brave/Facebook window.
2. Navigate to `https://facebook.com/`.
3. Dismiss transient prompt with `Escape` if present.
4. Observe the top of the home feed.
5. Scroll the feed with `Page_Down` three times, observing after each scroll.
6. Navigate to Facebook search for `CV-25-0070-PR`.
7. Observe the top of the search results.
8. Hold until the 60-second timer completes while sampling process resources.

This covers a legitimate browsing pass through the first visible feed items and
two to three pages of scroll, plus a fixed search. The final visual check showed
Facebook search results for `CV-25-0070-PR` loaded successfully.

## Aggregate Measurements

| Metric | Value |
| --- | ---: |
| Actions | `7` |
| Observations | `5` |
| Average PNG payload | `529,062.4` bytes |
| Total PNG payload | `2,645,312` bytes |
| Average base64 payload | `705,418.4` bytes |
| Total base64 payload | `3,527,092` bytes |
| Average capture wall time | `0.9040s` |
| Total capture CPU time | `2.84s` |
| Legacy high-detail vision tokens | `5,525` total |
| OpenAI image-input high-fidelity tokens | `32,815` total |

## Observation Payloads

| Observation | PNG bytes | Base64 bytes | Wall time | CPU time |
| --- | ---: | ---: | ---: | ---: |
| `home-feed-top` | `435,994` | `581,328` | `0.75s` | `0.48s` |
| `feed-scroll-1` | `684,295` | `912,396` | `0.69s` | `0.58s` |
| `feed-scroll-2` | `697,489` | `929,988` | `0.97s` | `0.69s` |
| `feed-scroll-3` | `531,897` | `709,196` | `1.07s` | `0.62s` |
| `search-results-top` | `295,637` | `394,184` | `1.04s` | `0.47s` |

## Process Resource Snapshot

Maximum RSS by command during the 60-second run:

| Command | Max RSS |
| --- | ---: |
| `brave` | `932,912 KiB` |
| `codex` | `2,559,608 KiB` |
| `node` | `376,464 KiB` |
| `Xvnc` | `52,752 KiB` |
| `Xorg` | `35,948 KiB` |
| `wmaker.real` | `8,116 KiB` |

Average sampled CPU percent by command:

| Command | Avg CPU |
| --- | ---: |
| `codex` | `11.10%` |
| `brave` | `2.90%` |
| `node` | `1.95%` |
| `Xorg` | `1.50%` |
| `Xvnc` | `0.30%` |
| `wmaker.real` | `0.00%` |

## Interpretation

This is the control for a real browsing task. The current method spent about
`3.53 MB` of base64-equivalent observation payload and `32,815` estimated
high-fidelity image-input tokens for five full-screen observations in one
minute. The capture tool alone consumed `2.84s` CPU across those observations.

For `wmaker-ng` + `wmaker-ai`, compare this exact scenario against:

- fewer full-screen observations;
- XDamage dirty-region payload bytes;
- region-level token estimates instead of full-screen token estimates;
- capture CPU and latency from direct XShm/delta tooling;
- successful action count and elapsed wall time.

The key comparison unit for the whitepaper lane should be:

```text
bytes per completed browsing scenario
tokens per completed browsing scenario
capture CPU seconds per completed browsing scenario
observations per completed browsing scenario
```
