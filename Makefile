VERSION = $(shell grep '^const VERSION' src/main.rs | cut -d'"' -f2)
SIGN_IDENTITY = "Developer ID Application: Toan Tran (D97BTN4C2U)"
APPLE_ID = flightst679@gmail.com
TEAM_ID = D97BTN4C2U

.PHONY: build release release-all clean notarize github-release

# --- Development ---

build:
	cargo build
	cp target/debug/tncli ./tncli
	codesign -s $(SIGN_IDENTITY) --force --options runtime ./tncli

# --- Release (current arch) ---

release:
	cargo build --release
	cp target/release/tncli ./tncli
	codesign -s $(SIGN_IDENTITY) --force --options runtime ./tncli

# --- Release (both architectures) ---

release-all: release-arm64 release-amd64

release-arm64:
	cargo build --release --target aarch64-apple-darwin
	mkdir -p dist
	cp target/aarch64-apple-darwin/release/tncli dist/tncli-darwin-arm64
	codesign -s $(SIGN_IDENTITY) --force --options runtime dist/tncli-darwin-arm64

release-amd64:
	cargo build --release --target x86_64-apple-darwin
	mkdir -p dist
	cp target/x86_64-apple-darwin/release/tncli dist/tncli-darwin-amd64
	codesign -s $(SIGN_IDENTITY) --force --options runtime dist/tncli-darwin-amd64

# --- Notarize both binaries ---

notarize: release-all
	cd dist && zip tncli-darwin-arm64.zip tncli-darwin-arm64
	cd dist && zip tncli-darwin-amd64.zip tncli-darwin-amd64
	xcrun notarytool submit dist/tncli-darwin-arm64.zip --keychain-profile "tncli-notarize" --wait
	xcrun notarytool submit dist/tncli-darwin-amd64.zip --keychain-profile "tncli-notarize" --wait
	rm -f dist/*.zip
	@echo "Both binaries notarized."

# --- GitHub Release ---

github-release: notarize
	cd dist && tar czf tncli-darwin-arm64.tar.gz tncli-darwin-arm64
	cd dist && tar czf tncli-darwin-amd64.tar.gz tncli-darwin-amd64
	cd dist && shasum -a 256 *.tar.gz > checksums.txt
	gh release create v$(VERSION) \
		dist/tncli-darwin-arm64.tar.gz \
		dist/tncli-darwin-amd64.tar.gz \
		dist/checksums.txt \
		--title "v$(VERSION)" \
		--notes "tncli v$(VERSION) — tmux-based service launcher"
	@echo "Released v$(VERSION) on GitHub."

clean:
	cargo clean
	rm -rf ./tncli dist/
