package commands

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

var Version = "0.5.0" // set by main

func Update() error {
	fmt.Printf("%sChecking for updates...%s\n", Bold, NC)

	out, err := exec.Command("curl", "-sL", "https://api.github.com/repos/toantran292/tncli/releases/latest").Output()
	if err != nil {
		return fmt.Errorf("could not fetch latest version")
	}

	latest := ""
	for _, line := range strings.Split(string(out), "\n") {
		if strings.Contains(line, `"tag_name"`) {
			parts := strings.Split(line, `"`)
			if len(parts) >= 4 {
				latest = strings.TrimPrefix(parts[3], "v")
			}
		}
	}
	if latest == "" {
		return fmt.Errorf("could not fetch latest version")
	}
	if latest == Version {
		fmt.Printf("%sAlready up to date: v%s%s\n", Green, Version, NC)
		return nil
	}

	fmt.Printf("Current: v%s → Latest: v%s\n", Version, latest)
	fmt.Printf("%s>>>%s Downloading update...\n", Blue, NC)

	osName := detectOS()
	arch := detectArch()

	url := fmt.Sprintf("https://github.com/toantran292/tncli/releases/download/v%s/tncli-%s-%s.tar.gz", latest, osName, arch)
	tmpdir := filepath.Join(os.TempDir(), "tncli-update")
	_ = os.MkdirAll(tmpdir, 0o755)
	tarPath := filepath.Join(tmpdir, "tncli.tar.gz")

	if exec.Command("curl", "-sL", "-o", tarPath, url).Run() != nil {
		return fmt.Errorf("download failed")
	}
	if exec.Command("tar", "xzf", tarPath, "-C", tmpdir).Run() != nil {
		return fmt.Errorf("extract failed")
	}

	binary := filepath.Join(tmpdir, fmt.Sprintf("tncli-%s-%s", osName, arch))
	if _, err := os.Stat(binary); os.IsNotExist(err) {
		return fmt.Errorf("binary not found in archive")
	}

	if osName == "darwin" {
		_ = exec.Command("xattr", "-rd", "com.apple.quarantine", binary).Run()
	}

	home, _ := os.UserHomeDir()
	installDir := filepath.Join(home, ".local/bin")
	installPath := filepath.Join(installDir, "tncli")
	_ = os.MkdirAll(installDir, 0o755)

	if exec.Command("cp", binary, installPath).Run() != nil {
		return fmt.Errorf("failed to copy binary to %s", installPath)
	}
	_ = exec.Command("chmod", "+x", installPath).Run()
	if osName == "darwin" {
		_ = exec.Command("codesign", "-s", "-", "--force", installPath).Run()
		_ = exec.Command("xattr", "-rd", "com.apple.quarantine", installPath).Run()
	}

	ensurePath(home, installDir)

	oldPath := "/usr/local/bin/tncli"
	if _, err := os.Stat(oldPath); err == nil {
		fmt.Printf("%s>>>%s Removing old binary from %s...\n", Blue, NC, oldPath)
		_ = exec.Command("sudo", "rm", oldPath).Run()
	}

	_ = os.RemoveAll(tmpdir)
	fmt.Printf("\n%sv%s installed to %s%s\n", Green, latest, installPath, NC)
	return nil
}

func detectOS() string {
	out, _ := exec.Command("uname").Output()
	if strings.Contains(strings.ToLower(string(out)), "darwin") {
		return "darwin"
	}
	return "linux"
}

func detectArch() string {
	out, _ := exec.Command("uname", "-m").Output()
	s := string(out)
	if strings.Contains(s, "arm64") || strings.Contains(s, "aarch64") {
		return "arm64"
	}
	return "amd64"
}

func ensurePath(home, installDir string) {
	pathEnv := os.Getenv("PATH")
	if strings.Contains(pathEnv, installDir) {
		return
	}
	zshrc := filepath.Join(home, ".zshrc")
	content, _ := os.ReadFile(zshrc)
	if strings.Contains(string(content), ".local/bin") {
		return
	}
	f, err := os.OpenFile(zshrc, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o644)
	if err != nil {
		return
	}
	defer f.Close()
	fmt.Fprintf(f, "\n# tncli\nexport PATH=\"$HOME/.local/bin:$PATH\"\n")
	fmt.Printf("\n%sAdded ~/.local/bin to PATH in ~/.zshrc%s\n", Yellow, NC)
}
