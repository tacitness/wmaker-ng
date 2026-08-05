# Voice and Command Control

`ai-mcp` now exposes a fixture-friendly command lane:

- `route_command`
- `list_app_skills`
- `app_skill_acquisition_plan`

The first lane is intentionally deterministic. Obvious commands route locally;
unknown commands return `ask_clarification` instead of guessing.

Supported fixture commands:

- `open browser to example dot com`
- `open terminal`
- `open chrome`
- `open blender`
- `open libreoffice`
- `open gimp`
- `open inkscape`
- `focus window 0x1234`
- `move window left`
- `move window right`
- `move window top`
- `move window bottom`
- `maximize window`
- `switch workspace 2` (planned only until EWMH workspace switching lands)
- `make a cylinder in Blender and render it`

Live audio capture/transcription remains out of this pass. The `source` field
supports `transcript_fixture` and `push_to_talk_asr` so the same command path is
ready once ASR is validated.

Safety defaults:

- local navigation/app launch/window movement runs without confirmation
- local artifact creation through the trusted Blender fixture runs without
  confirmation
- untrusted or learned skills require confirmation before execution
- external send/post/payment/destructive system actions are deliberately not
  represented as executable actions yet

Blender fixture output defaults to:

```text
/tmp/wmaker-ng/blender/cylinder.png
/tmp/wmaker-ng/blender/cylinder.blend
```

Packaged installs look for the render script at
`/usr/share/wmaker-ng/blender-cylinder.py`. Override with
`WMAKER_NG_BLENDER_CYLINDER_SCRIPT` for source-tree testing.
