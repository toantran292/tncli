VERSION = $(shell grep '^const VERSION' src/main.rs | cut -d'"' -f2)

.PHONY: build release tag clean

# --- Development ---

build:
	cargo build
	cp target/debug/tncli ./tncli

# --- Release (local, current arch) ---

release:
	cargo build --release
	cp target/release/tncli ./tncli

# --- Tag + push → GitHub Actions builds all platforms ---

tag:
	@echo "Releasing v$(VERSION)..."
	git tag v$(VERSION)
	git push origin v$(VERSION)
	@echo ""
	@echo "GitHub Actions will build + release:"
	@echo "  - macOS arm64 (Apple Silicon)"
	@echo "  - macOS amd64 (Intel)"
	@echo "  - Linux amd64"
	@echo "  - Linux arm64"
	@echo ""
	@echo "Track: https://github.com/toantran292/tncli/actions"

clean:
	cargo clean
	rm -rf ./tncli dist/
