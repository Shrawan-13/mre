package scaffold

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/Shrawan-13/mre/cli/internal/project"
)

func TestCreateAndAddPlugin(t *testing.T) {
	root := filepath.Join(t.TempDir(), "demo")
	cfg := project.Default("demo")
	if err := Create(root, cfg); err != nil {
		t.Fatal(err)
	}
	for _, file := range []string{
		"mre.toml", "src/main.lua", "framework/init.lua", "framework/nav.lua", ".gitignore",
		"android/app/build.gradle",
		"android/app/src/dev/java/dev/mre/dev/DevPairActivity.java",
	} {
		if _, err := os.Stat(filepath.Join(root, filepath.FromSlash(file))); err != nil {
			t.Errorf("missing generated file %s: %v", file, err)
		}
	}
	manifest, err := os.ReadFile(filepath.Join(root, "mre.toml"))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(manifest), `name = "demo"`) {
		t.Fatalf("manifest was not rendered: %s", manifest)
	}

	loaded, err := project.Load(filepath.Join(root, "mre.toml"))
	if err != nil {
		t.Fatal(err)
	}
	if err := AddPlugin(root, loaded, "battery"); err != nil {
		t.Fatal(err)
	}
	plugins, err := os.ReadFile(filepath.Join(root, "android", "app", "src", "main", "java", "dev", "mre", "Plugins.java"))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(plugins), "BatteryPlugin") {
		t.Fatalf("plugin was not registered: %s", plugins)
	}
	loaded, err = project.Load(filepath.Join(root, "mre.toml"))
	if err != nil {
		t.Fatal(err)
	}
	if len(loaded.Plugins) != 1 || loaded.Plugins[0] != "battery" {
		t.Fatalf("manifest plugins = %v", loaded.Plugins)
	}
}
