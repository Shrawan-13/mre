package serve

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"

	"github.com/Shrawan-13/mre/cli/internal/project"
)

func TestBundleRoundTripAndDeterministicHash(t *testing.T) {
	root := t.TempDir()
	cfg := project.Default("demo")
	write(t, root, "src/main.lua", "return mre\n")
	write(t, root, "framework/init.lua", "mre.tokens { accent = '#7C3AED' }\n")

	first, err := Build(root, cfg, 7)
	if err != nil {
		t.Fatal(err)
	}
	second, err := Build(root, cfg, 99)
	if err != nil {
		t.Fatal(err)
	}
	if first.Hash != second.Hash {
		t.Fatalf("hash depends on revision: %s != %s", first.Hash, second.Hash)
	}

	var encoded bytes.Buffer
	if err := Encode(&encoded, first); err != nil {
		t.Fatal(err)
	}
	decoded, err := Decode(bytes.NewReader(encoded.Bytes()))
	if err != nil {
		t.Fatal(err)
	}
	if decoded.Revision != 7 {
		t.Fatalf("revision = %d", decoded.Revision)
	}
	for name, source := range first.Modules {
		if decoded.Modules[name] != source {
			t.Errorf("module %s changed across the wire", name)
		}
	}
}

func write(t *testing.T, root, path, content string) {
	t.Helper()
	target := filepath.Join(root, filepath.FromSlash(path))
	if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(target, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}
}
