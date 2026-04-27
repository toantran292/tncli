VERSION = $(shell grep '^const VERSION' src/main.rs | cut -d'"' -f2)
MAJOR = $(word 1,$(subst ., ,$(VERSION)))
MINOR = $(word 2,$(subst ., ,$(VERSION)))
PATCH = $(word 3,$(subst ., ,$(VERSION)))

.PHONY: build release patch minor major tag clean

# --- Development ---

build:
	cargo build
	cp target/debug/tncli ./tncli

release:
	cargo build --release
	cp target/release/tncli ./tncli

# --- Version bump + release ---
# make patch  →  0.1.0 → 0.1.1 → tag → push → CI builds
# make minor  →  0.1.0 → 0.2.0 → tag → push → CI builds
# make major  →  0.1.0 → 1.0.0 → tag → push → CI builds

patch:
	$(eval NEW_VERSION := $(MAJOR).$(MINOR).$(shell echo $$(($(PATCH)+1))))
	@$(MAKE) _release NEW_VERSION=$(NEW_VERSION)

minor:
	$(eval NEW_VERSION := $(MAJOR).$(shell echo $$(($(MINOR)+1))).0)
	@$(MAKE) _release NEW_VERSION=$(NEW_VERSION)

major:
	$(eval NEW_VERSION := $(shell echo $$(($(MAJOR)+1))).0.0)
	@$(MAKE) _release NEW_VERSION=$(NEW_VERSION)

_release:
	@echo "$(VERSION) → $(NEW_VERSION)"
	sed -i '' 's/const VERSION: \&str = "$(VERSION)"/const VERSION: \&str = "$(NEW_VERSION)"/' src/main.rs
	sed -i '' 's/^version = "$(VERSION)"/version = "$(NEW_VERSION)"/' Cargo.toml
	cargo check 2>/dev/null
	git add src/main.rs Cargo.toml Cargo.lock
	git commit -m "release: v$(NEW_VERSION)"
	git tag v$(NEW_VERSION)
	git push origin main v$(NEW_VERSION)
	@echo ""
	@echo "v$(NEW_VERSION) released! CI building at:"
	@echo "https://github.com/toantran292/tncli/actions"

clean:
	cargo clean
	rm -rf ./tncli dist/
