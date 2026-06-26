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
install-dev-tools: ## Install cargo-audit + cargo-deny + gitleaks locally
	$(CARGO) install cargo-audit --locked
	$(CARGO) install cargo-deny --locked
	@if command -v gitleaks >/dev/null 2>&1; then echo "gitleaks already installed"; \
	elif command -v go >/dev/null 2>&1; then go install github.com/gitleaks/gitleaks/v8@latest; \
	else echo "Install gitleaks for secret scanning: https://github.com/gitleaks/gitleaks#installing"; fi

.PHONY: install-cross-tools
install-cross-tools: ## Install the cross-build toolchain (cargo-zigbuild; needs zig on PATH)
	$(CARGO) install cargo-zigbuild --locked
	@echo "Also install zig (https://ziglang.org) and 'go install github.com/goreleaser/nfpm/v2/cmd/nfpm@latest'"

.PHONY: setup
setup: install-dev-tools hooks ## Bootstrap local dev tools + git hooks

# ── Cross-build matrix (cargo-zigbuild) ───────────────────────────────────────
# glibc binaries are pinned to the EL9 floor (glibc 2.34) so one build runs on
# EL9/EL10, Fedora, Debian 12/13, Ubuntu 22.04/24.04. musl binaries are static
# (Alpine + anywhere). Two libc × two arch = four binary sets.
GNU_FLOOR := 2.34

.PHONY: build-amd64-gnu build-arm64-gnu build-amd64-musl build-arm64-musl cross-build
build-amd64-gnu:  ## Cross-build glibc/x86_64 binaries (EL9 floor)
	scripts/build.sh x86_64-unknown-linux-gnu.$(GNU_FLOOR)  amd64-gnu
build-arm64-gnu:  ## Cross-build glibc/aarch64 binaries (EL9 floor)
	scripts/build.sh aarch64-unknown-linux-gnu.$(GNU_FLOOR) arm64-gnu
build-amd64-musl: ## Cross-build musl-static/x86_64 binaries
	scripts/build.sh x86_64-unknown-linux-musl  amd64-musl
build-arm64-musl: ## Cross-build musl-static/aarch64 binaries
	scripts/build.sh aarch64-unknown-linux-musl arm64-musl
cross-build: build-amd64-gnu build-arm64-gnu build-amd64-musl build-arm64-musl ## Build the full matrix

# ── Packaging (nfpm → deb/rpm/apk) ────────────────────────────────────────────
# deb/rpm come from the glibc stage; apk from the musl stage.
.PHONY: packages
packages: ## Build deb+rpm (glibc) and apk (musl) for both arches → dist/pkg
	scripts/package.sh deb amd64 amd64-gnu
	scripts/package.sh rpm amd64 amd64-gnu
	scripts/package.sh deb arm64 arm64-gnu
	scripts/package.sh rpm arm64 arm64-gnu
	scripts/package.sh apk amd64 amd64-musl
	scripts/package.sh apk arm64 arm64-musl

.PHONY: tarballs
tarballs: ## Package staged binaries into portable tarballs → dist/tarballs
	scripts/tarball.sh amd64 gnu  amd64-gnu
	scripts/tarball.sh arm64 gnu  arm64-gnu
	scripts/tarball.sh amd64 musl amd64-musl
	scripts/tarball.sh arm64 musl arm64-musl

# ── Repository assembly + signing ─────────────────────────────────────────────
# Signing is keyed off GPG_KEY_ID (apt/rpm) and ABUILD_KEY (apk); unset = local
# unsigned build. apk assembly needs an Alpine host (apk + abuild-sign).
.PHONY: repo-apt repo-rpm repo-apk repos
repo-apt: ## Assemble (and sign) the APT repo → dist/repo/apt
	scripts/repo-apt.sh $(DIST_DIR)/repo/apt $(DIST_DIR)/pkg
repo-rpm: ## Assemble (and sign) the RPM repo → dist/repo/rpm
	scripts/repo-rpm.sh $(DIST_DIR)/repo/rpm $(DIST_DIR)/pkg
repo-apk: ## Assemble (and sign) the APK repo → dist/repo/apk (Alpine only)
	scripts/repo-apk.sh $(DIST_DIR)/repo/apk $(DIST_DIR)/pkg
repos: repo-apt repo-rpm repo-apk ## Assemble all repositories

.PHONY: publish
publish: ## rsync the assembled repos → repos.tacitsoft.dev (needs deploy key)
	scripts/publish.sh $(DIST_DIR)/repo

.PHONY: release-local
release-local: cross-build packages tarballs ## Full release build, no publish (CI parity sans signing)

# ── Sandbox image (#18): Xvfb + wmaker + ai-mcp ───────────────────────────────
.PHONY: sandbox-image
sandbox-image: ## Build the wmaker-ai sandbox image (needs wmaker-crm:headless base)
	$(CARGO) build --release -p ai-mcp
	install -m 0755 $(ROOT_DIR)/target/release/ai-mcp $(ROOT_DIR)/sandbox/ai-mcp
	docker build -t wmaker-ai-sandbox $(ROOT_DIR)/sandbox
	rm -f $(ROOT_DIR)/sandbox/ai-mcp
	@echo "Built wmaker-ai-sandbox (base: wmaker-crm:headless)"

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
