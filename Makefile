VERSION = $(shell grep '^const version' cmd/tncli/root.go | cut -d'"' -f2)
MAJOR = $(word 1,$(subst ., ,$(VERSION)))
MINOR = $(word 2,$(subst ., ,$(VERSION)))
PATCH = $(word 3,$(subst ., ,$(VERSION)))

BINARY = tncli
LDFLAGS = -s -w

.PHONY: build dev release install clean test vet patch minor major

# --- Development ---

build:
	go build -o $(BINARY) ./cmd/tncli/

dev:
	go build -o $(BINARY) ./cmd/tncli/ && ./$(BINARY) $(ARGS)

test:
	go test ./...

vet:
	go vet ./...

# --- Local release (install to /usr/local/bin) ---

release:
	go build -ldflags "$(LDFLAGS)" -o $(BINARY) ./cmd/tncli/
	codesign -s - --force --options runtime $(BINARY)

install: release
	sudo cp $(BINARY) /usr/local/bin/$(BINARY)
	@echo "Installed tncli v$(VERSION) to /usr/local/bin/tncli"

# --- Version bump + release to GitHub ---
# make patch  ->  0.5.0 -> 0.5.1 -> tag -> push -> CI builds
# make minor  ->  0.5.0 -> 0.6.0 -> tag -> push -> CI builds
# make major  ->  0.5.0 -> 1.0.0 -> tag -> push -> CI builds

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
	@echo "$(VERSION) -> $(NEW_VERSION)"
	sed -i '' 's/const version = "$(VERSION)"/const version = "$(NEW_VERSION)"/' cmd/tncli/root.go
	git add cmd/tncli/root.go
	git commit -m "release: v$(NEW_VERSION)"
	git tag v$(NEW_VERSION)
	git push origin main v$(NEW_VERSION)
	@echo ""
	@echo "v$(NEW_VERSION) released! CI building at:"
	@echo "https://github.com/toantran292/tncli/actions"

clean:
	rm -f $(BINARY)
	rm -rf dist/
