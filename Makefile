SHELL := /bin/bash

.DEFAULT_GOAL := help

.PHONY: help fmt test python-test check ci status site-build site-serve release-candidate e2e-s0027-deploy

help: ## Show available commands
	@grep -E '^[a-zA-Z0-9_.-]+:.*## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*## "}; {printf "\033[36m%-18s\033[0m %s\n", $$1, $$2}'

fmt: ## Format Rust code
	cargo fmt

test: ## Run Rust tests
	cargo test

python-test: ## Run maintained Python tests
	@python3 -c 'import jsonschema, yaml, pytest' || { echo 'missing Python test deps: python3 -m pip install -r requirements-dev.txt' >&2; exit 2; }
	@set -e; for test_file in tests/test_*.py; do python3 "$$test_file"; done

check: ## Run Rust compile checks
	cargo check

ci: ## Run the current integration suite
	bash ci/l2-integration.sh

site-build: ## Build the production-form local website
	./web/onboarding/build.sh

site-serve: ## Build and serve the site on http://127.0.0.1:4173
	./web/onboarding/serve-local.sh 4173

release-candidate: ## Build/check the paired macOS CLI + Linux/x86_64 runner without publishing
	./packaging/release-candidate.sh

e2e-s0027-deploy: ## Run S0027 against two operator-provided fresh Ubuntu hosts
	./fixtures/e2e/s0027/run.sh run

status: ## Show concise git status
	git status --short
