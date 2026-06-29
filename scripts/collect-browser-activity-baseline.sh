#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: collect-browser-activity-baseline.sh [--display DISPLAY] [--duration SEC] [--out-dir DIR] [--s3-uri S3_URI]

Collect a replayable browser-driving baseline on the current X11 / Window Maker
desktop. The scenario drives an already-open browser to Facebook, scrolls the
home feed, runs a fixed search, and records measurements without storing raw
screenshots.

Environment:
  DISPLAY       Default X display when --display is not supplied.
  WMAKER_BASELINE_S3_URI
                Optional S3 URI used when --s3-uri is not supplied.
EOF
}

display="${DISPLAY:-:9}"
duration_sec=60
out_dir=""
s3_uri="${WMAKER_BASELINE_S3_URI:-}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --display)
      display="${2:?missing display}"
      shift 2
      ;;
    --duration)
      duration_sec="${2:?missing duration}"
      shift 2
      ;;
    --out-dir)
      out_dir="${2:?missing output directory}"
      shift 2
      ;;
    --s3-uri)
      s3_uri="${2:?missing S3 URI}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if ! [[ "$duration_sec" =~ ^[0-9]+$ ]] || [[ "$duration_sec" -lt 20 ]]; then
  echo "--duration must be an integer >= 20" >&2
  exit 2
fi

for bin in awk date identify import jq ps sha256sum stat uname wmctrl xdotool xdpyinfo; do
  if ! command -v "$bin" >/dev/null 2>&1; then
    echo "required command not found: $bin" >&2
    exit 1
  fi
done

started_utc="$(date -u +%Y%m%dT%H%M%SZ)"
host="$(hostname -f 2>/dev/null || hostname)"
if [[ -z "$out_dir" ]]; then
  out_dir="out/browser-activity-baselines/${started_utc}-${host}-${display#:}"
fi
mkdir -p "$out_dir/tmp"

summary_json="$out_dir/summary.json"
captures_jsonl="$out_dir/captures.jsonl"
actions_jsonl="$out_dir/actions.jsonl"
process_samples_jsonl="$out_dir/process-samples.jsonl"
display_txt="$out_dir/display.txt"
windows_txt="$out_dir/windows.txt"
report_md="$out_dir/report.md"

scenario_name="facebook-feed-search-60s"
search_query="CV-25-0070-PR"
scenario_start_epoch="$(date +%s)"
deadline_epoch="$((scenario_start_epoch + duration_sec))"

json_string() {
  jq -Rsa . <<<"${1:-}"
}

base64_size() {
  local bytes="$1"
  awk -v b="$bytes" 'BEGIN { printf "%d", 4 * int((b + 2) / 3) }'
}

estimate_legacy_high_tokens() {
  local width="$1"
  local height="$2"
  awk -v w="$width" -v h="$height" '
    function ceil(x) { return int(x) == x ? x : int(x) + 1 }
    BEGIN {
      if (w > 2048 || h > 2048) {
        if (w >= h) { h = h * 2048 / w; w = 2048 } else { w = w * 2048 / h; h = 2048 }
      }
      if (w >= h) { w = w * 768 / h; h = 768 } else { h = h * 768 / w; w = 768 }
      tiles = ceil(w / 512) * ceil(h / 512)
      printf "%d", 85 + (170 * tiles)
    }'
}

estimate_openai_image_input_tokens() {
  local width="$1"
  local height="$2"
  awk -v w="$width" -v h="$height" '
    function ceil(x) { return int(x) == x ? x : int(x) + 1 }
    BEGIN {
      short = w < h ? w : h
      long = w > h ? w : h
      if (short > 512) { long = long * 512 / short; short = 512 }
      if (long > 2048) { short = short * 2048 / long; long = 2048 }
      tiles = ceil(short / 512) * ceil(long / 512)
      fidelity = (w == h) ? 4160 : 6240
      printf "%d", 65 + (129 * tiles) + fidelity
    }'
}

emit_action() {
  local name="$1"
  local detail="${2:-}"
  local now
  now="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  jq -nc \
    --arg ts "$now" \
    --arg name "$name" \
    --arg detail "$detail" \
    --arg elapsed "$(( $(date +%s) - scenario_start_epoch ))" \
    '{ts_utc:$ts, elapsed_sec:($elapsed|tonumber), action:$name, detail:$detail}' >>"$actions_jsonl"
}

capture_observation() {
  local label="$1"
  local png="$out_dir/tmp/${label}.png"
  local timing="$out_dir/tmp/${label}.time"
  local now
  now="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

  if ! /usr/bin/time -f $'elapsed_sec=%e\nuser_sec=%U\nsys_sec=%S\nmaxrss_kib=%M' \
    import -display "$display" -window root "$png" 2>"$timing"; then
    jq -nc \
      --arg ts "$now" \
      --arg lbl "$label" \
      --arg error "$(sed ':a;N;$!ba;s/\n/; /g' "$timing" 2>/dev/null || true)" \
      '{ts_utc:$ts, "label":$lbl, ok:false, error:$error}' >>"$captures_jsonl"
    rm -f "$png" "$timing"
    return
  fi

  local identify_line
  identify_line="$(identify -format '%w %h %z' "$png")"
  read -r width height depth <<<"$identify_line"
  local bytes sha elapsed user_sec sys_sec maxrss raw_rgba b64 legacy_high image_input_high
  bytes="$(stat -c '%s' "$png")"
  sha="$(sha256sum "$png" | awk '{print $1}')"
  elapsed="$(awk -F= '/elapsed_sec=/{print $2}' "$timing")"
  user_sec="$(awk -F= '/user_sec=/{print $2}' "$timing")"
  sys_sec="$(awk -F= '/sys_sec=/{print $2}' "$timing")"
  maxrss="$(awk -F= '/maxrss_kib=/{print $2}' "$timing")"
  raw_rgba="$((width * height * 4))"
  b64="$(base64_size "$bytes")"
  legacy_high="$(estimate_legacy_high_tokens "$width" "$height")"
  image_input_high="$(estimate_openai_image_input_tokens "$width" "$height")"

  jq -nc \
    --arg ts "$now" \
    --arg lbl "$label" \
    --arg display "$display" \
    --arg width "$width" \
    --arg height "$height" \
    --arg depth "$depth" \
    --arg png_bytes "$bytes" \
    --arg base64_bytes "$b64" \
    --arg raw_rgba_bytes "$raw_rgba" \
    --arg elapsed_sec "$elapsed" \
    --arg user_sec "$user_sec" \
    --arg sys_sec "$sys_sec" \
    --arg maxrss_kib "$maxrss" \
    --arg sha256 "$sha" \
    --arg legacy_high_tokens "$legacy_high" \
    --arg openai_image_input_high_tokens "$image_input_high" \
    '{
      ts_utc:$ts,
      "label":$lbl,
      display:$display,
      ok:true,
      width:($width|tonumber),
      height:($height|tonumber),
      depth_bits:($depth|tonumber),
      png_bytes:($png_bytes|tonumber),
      base64_bytes:($base64_bytes|tonumber),
      raw_rgba_bytes:($raw_rgba_bytes|tonumber),
      capture_elapsed_sec:($elapsed_sec|tonumber),
      capture_user_sec:($user_sec|tonumber),
      capture_sys_sec:($sys_sec|tonumber),
      capture_maxrss_kib:($maxrss_kib|tonumber),
      sha256:$sha256,
      token_estimates:{
        legacy_vision_high_tokens:($legacy_high_tokens|tonumber),
        openai_image_input_high_fidelity_tokens:($openai_image_input_high_tokens|tonumber)
      }
    }' >>"$captures_jsonl"

  rm -f "$png" "$timing"
}

sample_processes_once() {
  local now elapsed
  now="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  elapsed="$(( $(date +%s) - scenario_start_epoch ))"
  ps -eo pid,ppid,user,comm,%cpu,%mem,rss,vsz,etime,args --sort=pid \
    | awk -v ts="$now" -v elapsed="$elapsed" '
      NR > 1 && $4 ~ /^(wmaker\.real|Xorg|Xvnc|brave|brave-browser-s|node|codex)$/ && $5 ~ /^[0-9.]+$/ && $6 ~ /^[0-9.]+$/ && $7 ~ /^[0-9]+$/ && $8 ~ /^[0-9]+$/ {
        args=""
        for (i=10; i<=NF; i++) args=args (i==10 ? "" : " ") $i
        printf "{\"ts_utc\":\"%s\",\"elapsed_sec\":%d,\"pid\":%s,\"ppid\":%s,\"user\":%s,\"comm\":%s,\"cpu_percent\":%s,\"mem_percent\":%s,\"rss_kib\":%s,\"vsz_kib\":%s,\"process_elapsed\":%s,\"args\":%s}\n", \
          ts, elapsed, $1, $2, json($3), json($4), $5, $6, $7, $8, json($9), json(args)
      }
      function json(s, t) {
        t=s
        gsub(/\\/,"\\\\",t)
        gsub(/"/,"\\\"",t)
        return "\"" t "\""
      }'
}

sampler_pid=""
start_sampler() {
  (
    while [[ "$(date +%s)" -le "$deadline_epoch" ]]; do
      sample_processes_once >>"$process_samples_jsonl"
      sleep 1
    done
  ) &
  sampler_pid="$!"
}

stop_sampler() {
  if [[ -n "$sampler_pid" ]] && kill -0 "$sampler_pid" >/dev/null 2>&1; then
    kill "$sampler_pid" >/dev/null 2>&1 || true
    wait "$sampler_pid" 2>/dev/null || true
  fi
}
trap stop_sampler EXIT

xdpyinfo -display "$display" >"$display_txt" 2>&1 || true
DISPLAY="$display" wmctrl -lxG >"$windows_txt" 2>&1 || true
: >"$actions_jsonl"
: >"$captures_jsonl"
: >"$process_samples_jsonl"

start_sampler

emit_action "activate_browser" "wmctrl activates existing Brave/Facebook window"
DISPLAY="$display" wmctrl -xa brave-browser.Brave-browser || DISPLAY="$display" wmctrl -xa brave.Brave || true
sleep 1

emit_action "navigate_home" "address bar -> https://facebook.com/"
DISPLAY="$display" xdotool key --clearmodifiers ctrl+l
sleep 0.2
DISPLAY="$display" xdotool type --delay 10 "https://facebook.com/"
DISPLAY="$display" xdotool key Return
sleep 7

emit_action "dismiss_login_prompt" "Escape closes transient browser/site prompt if present"
DISPLAY="$display" xdotool key Escape || true
sleep 1
capture_observation "home-feed-top"

for page in 1 2 3; do
  emit_action "scroll_feed_page_${page}" "Page_Down through Facebook feed"
  DISPLAY="$display" xdotool key Page_Down
  sleep 4
  capture_observation "feed-scroll-${page}"
done

emit_action "facebook_search" "search query: ${search_query}"
DISPLAY="$display" xdotool key --clearmodifiers ctrl+l
sleep 0.2
DISPLAY="$display" xdotool type --delay 10 "https://www.facebook.com/search/top?q=${search_query}"
DISPLAY="$display" xdotool key Return
sleep 8
capture_observation "search-results-top"

while [[ "$(date +%s)" -lt "$deadline_epoch" ]]; do
  sleep 1
done

stop_sampler
ended_utc="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

jq -s \
  --arg started_utc "$started_utc" \
  --arg ended_utc "$ended_utc" \
  --arg host "$host" \
  --arg display "$display" \
  --arg scenario "$scenario_name" \
  --arg search_query "$search_query" \
  --arg duration_sec "$duration_sec" \
  --slurpfile actions "$actions_jsonl" \
  --slurpfile process_samples "$process_samples_jsonl" \
  '{
    schema:"wmaker-ng.browser-activity-baseline.v1",
    started_utc:$started_utc,
    ended_utc:$ended_utc,
    host:$host,
    display:$display,
    scenario:$scenario,
    duration_sec:($duration_sec|tonumber),
    search_query:$search_query,
    privacy:{
      raw_screenshots_stored:false,
      post_text_stored:false,
      private_messages_opened:false
    },
    driver_stack:"X11 xdotool/wmctrl plus full-screen ImageMagick observations",
    actions:$actions,
    captures:.,
    aggregates:{
      action_count:($actions | length),
      observation_count:([.[] | select(.ok == true)] | length),
      avg_png_bytes:([.[] | select(.ok == true) | .png_bytes] | if length == 0 then null else add / length end),
      total_png_bytes:([.[] | select(.ok == true) | .png_bytes] | if length == 0 then 0 else add end),
      avg_base64_bytes:([.[] | select(.ok == true) | .base64_bytes] | if length == 0 then null else add / length end),
      total_base64_bytes:([.[] | select(.ok == true) | .base64_bytes] | if length == 0 then 0 else add end),
      avg_capture_elapsed_sec:([.[] | select(.ok == true) | .capture_elapsed_sec] | if length == 0 then null else add / length end),
      total_capture_cpu_sec:([.[] | select(.ok == true) | (.capture_user_sec + .capture_sys_sec)] | if length == 0 then 0 else add end),
      avg_legacy_vision_high_tokens:([.[] | select(.ok == true) | .token_estimates.legacy_vision_high_tokens] | if length == 0 then null else add / length end),
      total_legacy_vision_high_tokens:([.[] | select(.ok == true) | .token_estimates.legacy_vision_high_tokens] | if length == 0 then 0 else add end),
      avg_openai_image_input_high_fidelity_tokens:([.[] | select(.ok == true) | .token_estimates.openai_image_input_high_fidelity_tokens] | if length == 0 then null else add / length end),
      total_openai_image_input_high_fidelity_tokens:([.[] | select(.ok == true) | .token_estimates.openai_image_input_high_fidelity_tokens] | if length == 0 then 0 else add end),
      max_process_rss_kib_by_comm:(
        $process_samples
        | sort_by(.comm)
        | group_by(.comm)
        | map({key:.[0].comm, value:(map(.rss_kib) | max)})
        | from_entries
      ),
      avg_process_cpu_percent_by_comm:(
        $process_samples
        | sort_by(.comm)
        | group_by(.comm)
        | map({key:.[0].comm, value:(map(.cpu_percent) | add / length)})
        | from_entries
      )
    }
  }' "$captures_jsonl" >"$summary_json"

cat >"$report_md" <<EOF
# Browser Activity Baseline

- Schema: \`wmaker-ng.browser-activity-baseline.v1\`
- Scenario: \`$scenario_name\`
- Started UTC: \`$started_utc\`
- Ended UTC: \`$ended_utc\`
- Host: \`$host\`
- Display: \`$display\`
- Duration: \`${duration_sec}s\`
- Search query: \`$search_query\`
- Driver stack: X11 \`xdotool\` / \`wmctrl\` with full-screen ImageMagick observations
- Raw screenshots stored: \`false\`
- Post text stored: \`false\`
- Private messages opened: \`false\`

## Scenario Steps

1. Activate the existing Brave/Facebook window.
2. Navigate to \`https://facebook.com/\`.
3. Dismiss transient prompt with \`Escape\` if present.
4. Capture the top of the home feed.
5. Scroll the feed with \`Page_Down\` three times, capturing after each scroll.
6. Navigate to Facebook search for \`CV-25-0070-PR\`.
7. Capture the top of the search results.
8. Hold the run open until the 60-second timer completes while sampling process resources.

## Aggregates

\`\`\`json
$(jq '.aggregates' "$summary_json")
\`\`\`

## Files

- \`summary.json\`
- \`actions.jsonl\`
- \`captures.jsonl\`
- \`process-samples.jsonl\`
- \`display.txt\`
- \`windows.txt\`
EOF

rmdir "$out_dir/tmp" 2>/dev/null || true

if [[ -n "$s3_uri" ]]; then
  if ! command -v aws >/dev/null 2>&1; then
    echo "aws CLI not found; leaving local artifacts in $out_dir" >&2
  else
    aws s3 cp "$out_dir" "$s3_uri/${started_utc}-${host}-${display#:}/" --recursive --only-show-errors
  fi
fi

echo "$out_dir"
