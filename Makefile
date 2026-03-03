SHELL := /bin/bash

.DEFAULT_GOAL := help

.PHONY: help install dev build preview tauri-dev tauri-build \
	fmt fmt-rust fmt-rust-check \
	test test-rust test-rust-quiet \
	check check-rust \
	phase1-verify phase2-verify \
	status

help: ## Show available commands
	@grep -E '^[a-zA-Z0-9_.-]+:.*## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*## "}; {printf "\033[36m%-18s\033[0m %s\n", $$1, $$2}'

install: ## Install frontend dependencies
	pnpm install

dev: ## Run frontend dev server
	pnpm dev

build: ## Build frontend assets
	pnpm build

preview: ## Preview frontend build
	pnpm preview

tauri-dev: ## Run Tauri app in dev mode
	pnpm tauri dev

tauri-build: ## Build Tauri app
	pnpm tauri build

fmt: fmt-rust ## Format code

fmt-rust: ## Format Rust code
	cd src-tauri && cargo fmt

fmt-rust-check: ## Check Rust formatting without changing files
	cd src-tauri && cargo fmt -- --check

test: test-rust ## Run default tests

test-rust: ## Run Rust tests
	cd src-tauri && cargo test

test-rust-quiet: ## Run Rust tests with concise output
	cd src-tauri && cargo test -q

check: check-rust ## Run default checks

check-rust: ## Run Rust compile checks
	cd src-tauri && cargo check

phase1-verify: test-rust ## Verify Phase 1 baseline tests

phase2-verify: test-rust ## Verify Phase 2 backend tests

status: ## Show concise git status
	git status --short
