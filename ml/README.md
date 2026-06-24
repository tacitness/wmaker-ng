# ml/ — Phase-4 ML tooling

Python is **quarantined here** (PLAN §3, §7). This subtree holds the data
wrangling and model-distillation toolchain for the distilled ~1B vision model
that natively speaks the screen-diff protocol — explicitly the **last** thing
built (PLAN §6), fed by traces captured while driving the containerized sandbox
with off-the-shelf models.

No model code yet — toolchain skeleton only.

## Tooling

- **uv** — packaging and virtualenv management.
- **ruff** — lint + format.
- **mypy** — strict type checking.

## Setup

```bash
cd ml
uv venv
uv pip install -e ".[dev]"
```

## Gates

```bash
uv run ruff check .
uv run ruff format --check .
uv run mypy
```
