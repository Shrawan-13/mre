// Package project reads and writes the mre.toml project manifest.
//
// The manifest is deliberately tiny. It is configuration, not a build system:
// the CLI owns the conventions and the manifest only records the choices a
// user actually makes.
package project

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
)

// Config is a parsed mre.toml.
type Config struct {
	Name         string
	Engine       string
	Entry        string
	Assets       []string
	AndroidAppID string
	AndroidLabel string
	Plugins      []string
	DevPort      int
}

// Default returns the manifest used by `mre new`.
func Default(name string) Config {
	return Config{
		Name:         name,
		Engine:       "0.1.0",
		Entry:        "src/main.lua",
		Assets:       []string{"assets"},
		AndroidAppID: "dev.mre.app",
		AndroidLabel: "MRE App",
		DevPort:      8080,
	}
}

// Find walks up from dir looking for mre.toml.
func Find(dir string) (string, error) {
	abs, err := filepath.Abs(dir)
	if err != nil {
		return "", err
	}
	for {
		candidate := filepath.Join(abs, "mre.toml")
		if _, err := os.Stat(candidate); err == nil {
			return candidate, nil
		}
		parent := filepath.Dir(abs)
		if parent == abs {
			return "", errors.New("no mre.toml found (run `mre new <name>` first)")
		}
		abs = parent
	}
}

// Load reads a manifest from an explicit path.
func Load(path string) (Config, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return Config{}, err
	}
	cfg, err := parse(string(data))
	if err != nil {
		return Config{}, fmt.Errorf("%s: %w", path, err)
	}
	return cfg, nil
}

func parse(src string) (Config, error) {
	cfg := Config{Engine: "0.1.0", Entry: "src/main.lua", DevPort: 8080}
	section := ""
	for i, raw := range strings.Split(src, "\n") {
		line := strings.TrimSpace(stripComment(raw))
		if line == "" {
			continue
		}
		if strings.HasPrefix(line, "[") && strings.HasSuffix(line, "]") {
			section = strings.TrimSpace(line[1 : len(line)-1])
			continue
		}
		key, value, ok := strings.Cut(line, "=")
		if !ok {
			return cfg, fmt.Errorf("line %d: expected key = value", i+1)
		}
		key = strings.TrimSpace(key)
		value = strings.Trim(strings.TrimSpace(value), `"'`)
		switch section {
		case "":
			switch key {
			case "name":
				cfg.Name = value
			case "engine":
				cfg.Engine = value
			case "entry":
				cfg.Entry = value
			}
		case "android":
			switch key {
			case "applicationId":
				cfg.AndroidAppID = value
			case "label":
				cfg.AndroidLabel = value
			case "plugins":
				for _, item := range strings.Split(value, ",") {
					if item = strings.TrimSpace(item); item != "" {
						cfg.Plugins = append(cfg.Plugins, item)
					}
				}
			}
		case "dev":
			switch key {
			case "port":
				n, err := strconv.Atoi(value)
				if err != nil {
					return cfg, fmt.Errorf("dev.port: %w", err)
				}
				cfg.DevPort = n
			case "assets":
				for _, item := range strings.Split(value, ",") {
					if item = strings.TrimSpace(item); item != "" {
						cfg.Assets = append(cfg.Assets, item)
					}
				}
			}
		}
	}
	if cfg.Name == "" {
		cfg.Name = "app"
	}
	return cfg, nil
}

func stripComment(line string) string {
	if i := strings.Index(line, "#"); i >= 0 {
		return line[:i]
	}
	return line
}

// String renders a deterministic manifest. Keys are written in a fixed order
// so regenerating a project never produces a meaningless diff.
func (c Config) String() string {
	var b strings.Builder
	fmt.Fprintf(&b, "# MRE project manifest — https://github.com/Shrawan-13/mre\n")
	fmt.Fprintf(&b, "name = %q\nengine = %q\nentry = %q\n", c.Name, c.Engine, c.Entry)
	b.WriteString("\n[android]\n")
	fmt.Fprintf(&b, "applicationId = %q\nlabel = %q\n", c.AndroidAppID, c.AndroidLabel)
	plugins := append([]string(nil), c.Plugins...)
	sort.Strings(plugins)
	fmt.Fprintf(&b, "plugins = %q\n", strings.Join(plugins, ", "))
	b.WriteString("\n[dev]\n")
	fmt.Fprintf(&b, "port = %d\nassets = %q\n", c.DevPort, strings.Join(c.Assets, ", "))
	return b.String()
}
