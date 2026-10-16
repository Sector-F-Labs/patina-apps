.DEFAULT_GOAL := help

.PHONY: help test check

help: ## Show this help
	@echo "Usage: make <target>"
	@echo
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  \033[1m%-14s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

test: ## Run the tests
	cargo test --all-features

check: test ## Run the tests and clippy
	cargo clippy --all-features --all-targets -- -D warnings
