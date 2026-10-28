.PHONY: format-rust format-shell format-toml

# Pass only changed files of the target language: make format-rust FILES="a.rs b.rs".
format-rust:
	@test -n "$(FILES)" || (echo "Set FILES to changed Rust files"; exit 1)
	rustfmt --edition 2021 $(FILES)

format-shell:
	@test -n "$(FILES)" || (echo "Set FILES to changed shell files"; exit 1)
	$(HOME)/.local/bin/shfmt -w $(FILES)

format-toml:
	@test -n "$(FILES)" || (echo "Set FILES to changed TOML files"; exit 1)
	$(HOME)/.cargo/bin/taplo format $(FILES)
