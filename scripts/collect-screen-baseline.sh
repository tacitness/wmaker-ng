#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: collect-screen-baseline.sh [--display DISPLAY] [--samples N] [--out-dir DIR] [--s3-uri S3_URI]

Collect a current Window Maker / X11 desktop-driving baseline without storing
raw screenshots. The script records screenshot dimensions, bytes, timing,
process resource snapshots, and image-token estimates.

Environment:
  DISPLAY       Default X display when --display is not supplied.
  WMAKER_BASELINE_S3_URI
                Optional S3 URI used when --s3-uri is not supplied.
EOF
}

display="${DISPLAY:-:0}"
samples=3
out_dir=""
s3_uri="${WMAKER_BASELINE_S3_URI:-}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --display)
      display="${2:?missing display}"
      shift 2
      ;;
    --samples)
      samples="${2:?missing sample count}"
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

if ! [[ "$samples" =~ ^[0-9]+$ ]] || [[ "$samples" -lt 1 ]]; then
  echo "--samples must be a positive integer" >&2
  exit 2
fi

for bin in jq import identify stat sha256sum awk sed ps date uname; do
  if ! command -v "$bin" >/dev/null 2>&1; then
    echo "required command not found: $bin" >&2
    exit 1
  fi
done

started_utc="$(date -u +%Y%m%dT%H%M%SZ)"
host="$(hostname -f 2>/dev/null || hostname)"
if [[ -z "$out_dir" ]]; then
  out_dir="out/screen-baselines/${started_utc}-${host}-${display#:}"
fi
mkdir -p "$out_dir/tmp"

summary_json="$out_dir/summary.json"
captures_jsonl="$out_dir/captures.jsonl"
processes_json="$out_dir/processes.json"
display_txt="$out_dir/display.txt"
report_md="$out_dir/report.md"

estimate_legacy_high_tokens() {
  local width="$1"
  local height="$2"
  awk -v w="$width" -v h="$height" '
    function ceil(x) { return int(x) == x ? x : int(x) + 1 }
    BEGIN {
      if (w > 2048 || h > 2048) {
        if (w >= h) {
          h = h * 2048 / w
          w = 2048
        } else {
          w = w * 2048 / h
          h = 2048
        }
      }
      if (w >= h) {
        w = w * 768 / h
        h = 768
      } else {
        h = h * 768 / w
        w = 768
      }
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
      if (short > 512) {
        long = long * 512 / short
        short = 512
      }
      if (long > 2048) {
        short = short * 2048 / long
        long = 2048
      }
      tiles = ceil(short / 512) * ceil(long / 512)
      fidelity = (w == h) ? 4160 : 6240
      printf "%d", 65 + (129 * tiles) + fidelity
    }'
}

base64_size() {
  local bytes="$1"
  awk -v b="$bytes" 'BEGIN { printf "%d", 4 * int((b + 2) / 3) }'
}

collect_processes() {
  ps -eo pid,ppid,user,comm,%cpu,%mem,rss,vsz,etime,args --sort=pid \
    | awk '
      BEGIN { print "["; first=1 }
      NR > 1 && $4 ~ /^(wmaker\.real|Xorg|Xvnc|brave|node|codex)$/ {
        line=$0
        args=""
        for (i=10; i<=NF; i++) args=args (i==10 ? "" : " ") $i
        if (!first) print ","
        first=0
        printf "{\"pid\":%s,\"ppid\":%s,\"user\":%s,\"comm\":%s,\"cpu_percent\":%s,\"mem_percent\":%s,\"rss_kib\":%s,\"vsz_kib\":%s,\"elapsed\":%s,\"args\":%s}", \
          $1, $2, json($3), json($4), $5, $6, $7, $8, json($9), json(args)
      }
      END { print "\n]" }
      function json(s, t) {
        t=s
        gsub(/\\/,"\\\\",t)
        gsub(/"/,"\\\"",t)
        return "\"" t "\""
      }'
}

xdpyinfo -display "$display" >"$display_txt" 2>&1 || true
collect_processes | jq . >"$processes_json"

for sample in $(seq 1 "$samples"); do
  png="$out_dir/tmp/sample-${sample}.png"
  timing="$out_dir/tmp/sample-${sample}.time"
  if ! /usr/bin/time -f $'elapsed_sec=%e\nuser_sec=%U\nsys_sec=%S\nmaxrss_kib=%M' \
    import -display "$display" -window root "$png" 2>"$timing"; then
    jq -n \
      --arg sample "$sample" \
      --arg display "$display" \
      --arg error "$(sed ':a;N;$!ba;s/\n/; /g' "$timing" 2>/dev/null || true)" \
      '{sample:($sample|tonumber), display:$display, ok:false, error:$error}' >>"$captures_jsonl"
    continue
  fi

  identify_line="$(identify -format '%w %h %z' "$png")"
  read -r width height depth <<<"$identify_line"
  bytes="$(stat -c '%s' "$png")"
  sha="$(sha256sum "$png" | awk '{print $1}')"
  elapsed="$(awk -F= '/elapsed_sec=/{print $2}' "$timing")"
  user_sec="$(awk -F= '/user_sec=/{print $2}' "$timing")"
  sys_sec="$(awk -F= '/sys_sec=/{print $2}' "$timing")"
  maxrss="$(awk -F= '/maxrss_kib=/{print $2}' "$timing")"
  raw_rgba_bytes="$((width * height * 4))"
  b64_bytes="$(base64_size "$bytes")"
  legacy_high="$(estimate_legacy_high_tokens "$width" "$height")"
  image_input_high="$(estimate_openai_image_input_tokens "$width" "$height")"

  jq -n \
    --arg sample "$sample" \
    --arg display "$display" \
    --arg width "$width" \
    --arg height "$height" \
    --arg depth "$depth" \
    --arg png_bytes "$bytes" \
    --arg base64_bytes "$b64_bytes" \
    --arg raw_rgba_bytes "$raw_rgba_bytes" \
    --arg elapsed_sec "$elapsed" \
    --arg user_sec "$user_sec" \
    --arg sys_sec "$sys_sec" \
    --arg maxrss_kib "$maxrss" \
    --arg sha256 "$sha" \
    --arg legacy_high_tokens "$legacy_high" \
    --arg openai_image_input_high_tokens "$image_input_high" \
    '{
      sample:($sample|tonumber),
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
done
rmdir "$out_dir/tmp" 2>/dev/null || true

jq -s \
  --arg started_utc "$started_utc" \
  --arg host "$host" \
  --arg display "$display" \
  --slurpfile processes "$processes_json" \
  '{
    schema:"wmaker-ng.screen-baseline.v1",
    started_utc:$started_utc,
    host:$host,
    display:$display,
    capture_tool:"ImageMagick import -window root",
    raw_screenshots_stored:false,
    capture_count:length,
    captures:.,
    aggregates:{
      ok_count:([.[] | select(.ok == true)] | length),
      avg_png_bytes:([.[] | select(.ok == true) | .png_bytes] | if length == 0 then null else add / length end),
      avg_base64_bytes:([.[] | select(.ok == true) | .base64_bytes] | if length == 0 then null else add / length end),
      avg_raw_rgba_bytes:([.[] | select(.ok == true) | .raw_rgba_bytes] | if length == 0 then null else add / length end),
      avg_capture_elapsed_sec:([.[] | select(.ok == true) | .capture_elapsed_sec] | if length == 0 then null else add / length end),
      avg_capture_cpu_sec:([.[] | select(.ok == true) | (.capture_user_sec + .capture_sys_sec)] | if length == 0 then null else add / length end),
      avg_legacy_vision_high_tokens:([.[] | select(.ok == true) | .token_estimates.legacy_vision_high_tokens] | if length == 0 then null else add / length end),
      avg_openai_image_input_high_fidelity_tokens:([.[] | select(.ok == true) | .token_estimates.openai_image_input_high_fidelity_tokens] | if length == 0 then null else add / length end)
    },
    process_snapshot:$processes[0]
  }' "$captures_jsonl" >"$summary_json"

cat >"$report_md" <<EOF
# Screen-Driving Baseline

- Schema: \`wmaker-ng.screen-baseline.v1\`
- Started UTC: \`$started_utc\`
- Host: \`$host\`
- Display: \`$display\`
- Capture tool: \`ImageMagick import -window root\`
- Raw screenshots stored: \`false\`

## Aggregates

\`\`\`json
$(jq '.aggregates' "$summary_json")
\`\`\`

## Notes

- This baseline stores screenshot hashes and byte counts, not the screenshots.
- \`png_bytes\` and \`base64_bytes\` measure transport/data payload.
- \`raw_rgba_bytes\` is the uncompressed frame equivalent used to compare against
  future XShm/XDamage paths.
- Token estimates are estimator fields, not provider billing records.

## Files

- \`summary.json\`
- \`captures.jsonl\`
- \`processes.json\`
- \`display.txt\`
EOF

if [[ -n "$s3_uri" ]]; then
  if ! command -v aws >/dev/null 2>&1; then
    echo "aws CLI not found; leaving local artifacts in $out_dir" >&2
  else
    aws s3 cp "$out_dir" "$s3_uri/${started_utc}-${host}-${display#:}/" --recursive --only-show-errors
  fi
fi

echo "$out_dir"
