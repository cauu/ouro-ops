SHELL := /bin/bash

.DEFAULT_GOAL := help

.PHONY: help fmt test check ci e2e status e2e-build-base e2e-bed-up e2e-bed-down e2e-provision e2e-t2 e2e-clean

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

# Repo root + E2E paths, independent of the caller's CWD.
REPO_ROOT := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
E2E_COMPOSE := $(REPO_ROOT)/fixtures/e2e/compose.yaml

e2e-build-base: ## Compile ouro + build the shared E2E base image (ouro-e2e-base:local)
	docker build -f "$(REPO_ROOT)/fixtures/e2e/Dockerfile.base" -t ouro-e2e-base:local "$(REPO_ROOT)"

e2e-bed-up: e2e-build-base ## Build + start the S0015 E2E container bed (waits for healthy)
	docker compose -f "$(E2E_COMPOSE)" up -d --build --wait

e2e-provision: ## Provision SSH keys/creds/spec into the running bed (run after e2e-bed-up)
	bash "$(REPO_ROOT)/fixtures/e2e/provision.sh"

e2e-t2: e2e-build-base ## Run the deterministic T2 container E2E suite (build, up, provision, assert, teardown)
	bash "$(REPO_ROOT)/fixtures/e2e/e2e-t2.sh"

e2e-bed-down: ## Tear down the S0015 E2E bed (incl. volumes/networks)
	docker compose -f "$(E2E_COMPOSE)" down -v --remove-orphans

e2e-clean: ## Remove ALL S0015 test artifacts (containers, our images, build cache, /tmp); preserves your pre-existing images
	bash "$(REPO_ROOT)/fixtures/e2e/clean.sh"
