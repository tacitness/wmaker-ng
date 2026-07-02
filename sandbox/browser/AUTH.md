# Browser Auth Strategy (#21)

`wmaker-ai-browser` supports three auth modes. None stores credentials in the
image or repository.

## 1. Local Demo Profile

For one-off local demos, bind-mount a browser profile:

```bash
python3 scripts/drive-browser.py \
  --profile ~/.config/BraveSoftware/Brave-Browser \
  --clear-singleton \
  --url https://example.com
```

The host browser must be closed. This is convenient, but it carries the
operator's live session material into an agent-controllable container and is not
the repeatable automation path.

## 2. Disposable Seeded Profile

For repeatable runs, seed a throwaway profile from a secret-managed tarball:

```bash
docker run -i --rm \
  -e DISPOSABLE_PROFILE=1 \
  -e PROFILE_SEED_TARBALL=/run/secrets/browser-profile.tar.gz \
  -e AUTH_ALLOWED_DOMAINS=example.com,github.com \
  -e START_URL=https://example.com \
  -v /secure/runtime/browser-profile.tar.gz:/run/secrets/browser-profile.tar.gz:ro \
  wmaker-ai-browser
```

The launcher extracts the seed into `/tmp/wmaker-ai-browser-profile`, runs Brave
against that disposable profile, and discards it with the container.

The seed tarball is produced outside this repository by the operator's secrets
system. For TacitSoft infrastructure, the expected production source of truth is
AWS Secrets Manager, retrieved by the launcher/orchestrator and mounted into the
container as a read-only secret file.

## 3. Login Automation

A future login automation step may create the disposable profile at runtime from
username/password, OAuth refresh tokens, or per-site cookies retrieved from a
vault. That automation must run under the same controls:

- one disposable profile per run;
- domain allowlist through `AUTH_ALLOWED_DOMAINS`;
- no session material persisted back to the host;
- no secrets printed to logs;
- no credentials stored in git, image layers, or CI variables.

## Container Controls

| Env | Meaning |
| --- | --- |
| `DISPOSABLE_PROFILE=1` | Use `/tmp/wmaker-ai-browser-profile` instead of the mounted `/profile`. |
| `PROFILE_SEED_TARBALL=/path/to/profile.tar.gz` | Extract a secret-managed profile seed before browser launch. |
| `AUTH_ALLOWED_DOMAINS=host1,host2` | Refuse to open `START_URL` unless its host is listed. |
| `CLEAR_SINGLETON=1` | Remove stale `Singleton*` locks from the chosen profile. |

`CLEAR_SINGLETON` mutates the profile directory and should be used only for
local demos or disposable profiles.
