package cmd

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/Shrawan-13/mre/cli/internal/project"
)

func TestLuaFilesIncludesFrameworkAndPlugins(t *testing.T) {
	root := t.TempDir()
	files := []string{
		"src/main.lua",
		"framework/init.lua",
		"framework/nav.lua",
		"plugins/example.lua",
		"tests/app.lua",
		"README.md",
	}
	for _, name := range files {
		path := filepath.Join(root, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte("return true\n"), 0o644); err != nil {
			t.Fatal(err)
		}
	}

	found, err := luaFiles(root, project.Config{Entry: "src/main.lua"})
	if err != nil {
		t.Fatal(err)
	}
	set := make(map[string]bool, len(found))
	for _, path := range found {
		relative, err := filepath.Rel(root, path)
		if err != nil {
			t.Fatal(err)
		}
		set[filepath.ToSlash(relative)] = true
	}
	for _, want := range []string{
		"src/main.lua",
		"framework/init.lua",
		"framework/nav.lua",
		"plugins/example.lua",
		"tests/app.lua",
	} {
		if !set[want] {
			t.Errorf("luaFiles missed %s", want)
		}
	}
	if set["README.md"] {
		t.Error("luaFiles included a non-Lua file")
	}
}
