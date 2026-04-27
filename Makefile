SIGN_IDENTITY = "Developer ID Application: Toan Tran (D97BTN4C2U)"
APPLE_ID = flightst679@gmail.com
TEAM_ID = D97BTN4C2U

.PHONY: build release clean notarize

build:
	cargo build
	cp target/debug/tncli ./tncli
	codesign -s $(SIGN_IDENTITY) --force --options runtime ./tncli

release:
	cargo build --release
	cp target/release/tncli ./tncli
	codesign -s $(SIGN_IDENTITY) --force --options runtime ./tncli

notarize: release
	zip -j tncli.zip ./tncli
	xcrun notarytool submit tncli.zip --keychain-profile "tncli-notarize" --wait
	rm tncli.zip
	@echo "Notarized! Binary is ready to distribute."

clean:
	cargo clean
	rm -f ./tncli tncli.zip
