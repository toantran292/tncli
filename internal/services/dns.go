package services

import (
	"fmt"
	"os"
	"os/exec"
	"strings"
)

const (
	dnsmasqEntry = "address=/tncli.test/127.0.0.1"
	resolverPath = "/etc/resolver/tncli.test"
)

func dnsmasqConfPath() string {
	arm := "/opt/homebrew/etc/dnsmasq.conf"
	if _, err := os.Stat(arm); err == nil {
		return arm
	}
	intel := "/usr/local/etc/dnsmasq.conf"
	if _, err := os.Stat(intel); err == nil {
		return intel
	}
	return arm
}

func IsDnsmasqInstalled() bool {
	return exec.Command("brew", "list", "dnsmasq").Run() == nil
}

func IsDnsmasqConfigured() bool {
	data, err := os.ReadFile(dnsmasqConfPath())
	if err != nil {
		return false
	}
	return strings.Contains(string(data), dnsmasqEntry)
}

func IsResolverConfigured() bool {
	_, err := os.Stat(resolverPath)
	return err == nil
}

func IsDnsmasqRunning() bool {
	out, err := exec.Command("brew", "services", "info", "dnsmasq", "--json").Output()
	if err != nil {
		return false
	}
	return strings.Contains(string(out), `"running"`)
}

type DNSStatus struct {
	Installed  bool
	Configured bool
	Running    bool
	Resolver   bool
}

func (s DNSStatus) IsReady() bool {
	return s.Installed && s.Configured && s.Running && s.Resolver
}

func GetDNSStatus() DNSStatus {
	return DNSStatus{
		Installed:  IsDnsmasqInstalled(),
		Configured: IsDnsmasqConfigured(),
		Running:    IsDnsmasqRunning(),
		Resolver:   IsResolverConfigured(),
	}
}

func SetupDnsmasq() ([]string, error) {
	var actions []string

	if !IsDnsmasqInstalled() {
		fmt.Fprintln(os.Stderr, "Installing dnsmasq via brew...")
		if err := exec.Command("brew", "install", "dnsmasq").Run(); err != nil {
			return nil, fmt.Errorf("failed to install dnsmasq")
		}
		actions = append(actions, "installed dnsmasq")
	}

	if !IsDnsmasqConfigured() {
		conf := dnsmasqConfPath()
		content, _ := os.ReadFile(conf)
		s := string(content)
		if s != "" && !strings.HasSuffix(s, "\n") {
			s += "\n"
		}
		s += "# tncli proxy\n" + dnsmasqEntry + "\n"
		if err := os.WriteFile(conf, []byte(s), 0o644); err != nil {
			return nil, err
		}
		actions = append(actions, fmt.Sprintf("added %s to %s", dnsmasqEntry, conf))
	}

	if !IsResolverConfigured() {
		if err := exec.Command("sudo", "mkdir", "-p", "/etc/resolver").Run(); err != nil {
			return nil, fmt.Errorf("failed to create /etc/resolver (sudo)")
		}
		cmd := fmt.Sprintf("echo 'nameserver 127.0.0.1' > %s", resolverPath)
		if err := exec.Command("sudo", "sh", "-c", cmd).Run(); err != nil {
			return nil, fmt.Errorf("failed to create %s (sudo)", resolverPath)
		}
		actions = append(actions, "created "+resolverPath)
	}

	if IsDnsmasqRunning() {
		_ = exec.Command("sudo", "brew", "services", "restart", "dnsmasq").Run()
	} else {
		_ = exec.Command("sudo", "brew", "services", "start", "dnsmasq").Run()
	}
	actions = append(actions, "dnsmasq service started")

	return actions, nil
}

func VerifyResolution() bool {
	out, err := exec.Command("dscacheutil", "-q", "host", "-a", "name", "test.tncli.test").Output()
	if err != nil {
		return false
	}
	return strings.Contains(string(out), "127.0.0.1")
}
