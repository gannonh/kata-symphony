SHELL := /bin/bash

.PHONY: lint harness docs-check check

lint:
	bash scripts/harness/lint_repo.sh

harness:
	bash scripts/harness/check_repo_contract.sh
	bash scripts/harness/check_markdown_freshness.sh

docs-check:
	bash scripts/harness/check_docs_sync.sh

check: lint harness docs-check

