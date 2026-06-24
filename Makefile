# ============================================================================
# Makefile — wmaker-ng (Rust companion workspace)
# ----------------------------------------------------------------------------
# House style (PLAN §7, mirrors tsctl):
#   * Version lives ONLY in git tags — never stored in this file.
#   * Quality gates: fmt-check · clippy (deny warnings) · audit · deny · test.
#   * Third-party CI Actions are pinned by commit SHA (see .github/workflows).
# ============================================================================

.DEFAULT_GOAL := help
SHELL         := /bin/bash

ROOT_DIR := $(shell git rev-parse --show-toplevel 2>/dev/null || pwd)
CARGO    := cargo

# ── Version ──────────────────────────────────────────────────────────────────
# Version lives ONLY in git tags — never stored in this file.
# To release: make bump-patch | bump-minor | bump-major
VERSION := $(shell git describe --tags --always --dirty 2>/dev/null || echo "v0.0.0-unknown")
COMMIT  := $(shell git rev-parse --short HEAD 2>/dev/null || echo "none")

# Injected into the build so binaries can report a tag-derived version without
# the number ever living in Cargo.toml (which carries a 0.0.0 placeholder).
export WMAKER_NG_VERSION := $(VERSION)
export WMAKER_NG_COMMIT  := $(COMMIT)

# ── Packaging (nfpm) ──────────────────────────────────────────────────────────
_GIT_EXACT   := $(shell git describe --exact-match --tags HEAD 2>/dev/null)
_GIT_TAG     := $(shell git describe --tags --abbrev=0 2>/dev/null | sed 's/^v//')
_BASE_VER    := $(if $(_GIT_TAG),$(_GIT_TAG),0.0.0)
# Exact tag → clean semver; otherwise a dev pre-release that sorts below it.
export PKG_VERSION := $(if $(_GIT_EXACT),$(_BASE_VER),$(_BASE_VER)~dev.g$(COMMIT))
PKG_DIR  := $(ROOT_DIR)/packaging/nfpm
DIST_DIR := $(ROOT_DIR)/dist

# ── Help ──────────────────────────────────────────────────────────────────────
.PHONY: help
help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}'

# ── Build / test ────────────────────────────────────────────────────────────
.PHONY: build
build: ## Build the whole workspace (debug)
	$(CARGO) build --workspace

.PHONY: release
release: ## Build the whole workspace (release, optimized for size)
	$(CARGO) build --workspace --release

.PHONY: test
test: ## Run all tests
	$(CARGO) test --workspace

# ── Quality gates ─────────────────────────────────────────────────────────────
.PHONY: fmt
fmt: ## Format all crates
	$(CARGO) fmt --all

.PHONY: fmt-check
fmt-check: ## Verify formatting is clean (CI parity)
	$(CARGO) fmt --all --check

.PHONY: clippy
clippy: ## Lint with warnings denied (CI parity)
	$(CARGO) clippy --workspace --all-targets -- -D warnings

.PHONY: lint
lint: clippy ## Clippy + shellcheck on the git hooks
	@command -v shellcheck >/dev/null 2>&1 && shellcheck .githooks/* || \
		echo "shellcheck not installed — skipping hook lint"

.PHONY: audit
audit: ## Scan dependencies for RUSTSEC advisories
	$(CARGO) audit

.PHONY: deny
deny: ## Supply-chain / license / source gating
	$(CARGO) deny check

.PHONY: secret-scan
secret-scan: ## Block obvious secrets in staged changes
	@./.githooks/secret-scan.sh

.PHONY: pre-commit
pre-commit: fmt-check clippy secret-scan ## Fast gate run by the pre-commit hook

.PHONY: pre-push
pre-push: build test ## Gate run by the pre-push hook

.PHONY: ci-local
ci-local: fmt-check clippy build test audit deny ## Full local parity with the validate workflow

# ── Git hooks ─────────────────────────────────────────────────────────────────
.PHONY: hooks
hooks: ## Install the repo-managed git hooks (.githooks/)
	git config core.hooksPath .githooks
	@echo "Configured core.hooksPath=.githooks"

.PHONY: install-dev-tools
install-dev-tools: ## Install cargo-audit + cargo-deny locally
	$(CARGO) install cargo-audit --locked
	$(CARGO) install cargo-deny --locked

.PHONY: setup
setup: install-dev-tools hooks ## Bootstrap local dev tools + git hooks

# ── Packaging (nfpm → deb/rpm/apk → repos.tacitsoft.dev) ──────────────────────
.PHONY: package-deb package-rpm package-apk packages
package-deb: ## Build .deb packages via nfpm
	@mkdir -p $(DIST_DIR)
	nfpm package --config $(PKG_DIR)/wmaker-ng.yaml --packager deb --target $(DIST_DIR)
	nfpm package --config $(PKG_DIR)/wmaker-ai.yaml --packager deb --target $(DIST_DIR)

package-rpm: ## Build .rpm packages via nfpm
	@mkdir -p $(DIST_DIR)
	nfpm package --config $(PKG_DIR)/wmaker-ng.yaml --packager rpm --target $(DIST_DIR)
	nfpm package --config $(PKG_DIR)/wmaker-ai.yaml --packager rpm --target $(DIST_DIR)

package-apk: ## Build .apk packages via nfpm
	@mkdir -p $(DIST_DIR)
	nfpm package --config $(PKG_DIR)/wmaker-ng.yaml --packager apk --target $(DIST_DIR)
	nfpm package --config $(PKG_DIR)/wmaker-ai.yaml --packager apk --target $(DIST_DIR)

packages: package-deb package-rpm package-apk ## Build all package formats

# ── Release (tag-only versioning) ─────────────────────────────────────────────
_VER_MAJOR := $(shell echo $(_BASE_VER) | cut -d. -f1)
_VER_MINOR := $(shell echo $(_BASE_VER) | cut -d. -f2)
_VER_PATCH := $(shell echo $(_BASE_VER) | cut -d. -f3)

.PHONY: bump-patch
bump-patch: ## Tag next patch release and push — no file edits
	$(eval _NEXT := $(_VER_MAJOR).$(_VER_MINOR).$(shell expr $(_VER_PATCH) + 1))
	@git tag v$(_NEXT) && git push origin v$(_NEXT)
	@echo "Released v$(_NEXT)"

.PHONY: bump-minor
bump-minor: ## Tag next minor release and push — no file edits
	$(eval _NEXT := $(_VER_MAJOR).$(shell expr $(_VER_MINOR) + 1).0)
	@git tag v$(_NEXT) && git push origin v$(_NEXT)
	@echo "Released v$(_NEXT)"

.PHONY: bump-major
bump-major: ## Tag next major release and push — no file edits
	$(eval _NEXT := $(shell expr $(_VER_MAJOR) + 1).0.0)
	@git tag v$(_NEXT) && git push origin v$(_NEXT)
	@echo "Released v$(_NEXT)"

.PHONY: version
version: ## Print the tag-derived version
	@echo "version: $(VERSION)  commit: $(COMMIT)  pkg: $(PKG_VERSION)"

.PHONY: clean
clean: ## Remove build + dist artifacts
	$(CARGO) clean
	rm -rf $(DIST_DIR)
