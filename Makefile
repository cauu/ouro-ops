SHELL := /bin/bash

.DEFAULT_GOAL := help

.PHONY: help fmt test check ci e2e status

help: ## Show available commands
	@grep -E '^[a-zA-Z0-9_.-]+:.*## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*## "}; {printf "\033[36m%-18s\033[0m %s\n", $$1, $$2}'

fmt: ## Format Rust code
	cargo fmt

test: ## Run Rust tests
	cargo test

check: ## Run Rust compile checks
	cargo check

ci: ## Run L2 integration suite
	bash ci/l2-integration.sh

e2e: ## Run harness-style end-to-end flow
	bash ci/harness-e2e.sh

status: ## Show concise git status
	git status --short
