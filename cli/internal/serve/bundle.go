// Package serve implements the mre dev server and its wire format.
//
// The format is deliberately length-prefixed plain text rather than JSON: the
// Android client parses it with std::net and no serde dependency, which keeps
// the engine binary small and the protocol trivial to verify by hand.
package serve

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"

	"github.com/Shrawan-13/mre/cli/internal/project"
	"github.com/Shrawan-13/mre/cli/internal/version"
)

// Bundle is one immutable revision of the app's Lua modules.
type Bundle struct {
	Revision uint64
	Modules  map[string]string
	Hash     string
}

// Build scans a project and produces a deterministic bundle.
func Build(root string, cfg project.Config, revision uint64) (Bundle, error) {
	modules := map[string]string{}

	entry := filepath.Join(root, filepath.FromSlash(cfg.Entry))
	source, err := os.ReadFile(entry)
	if err != nil {
		return Bundle{}, fmt.Errorf("entry %s: %w", cfg.Entry, err)
	}
	// Android always loads assets/main.lua; the loader resolves the other
	// module names from the same map.
	modules["main.lua"] = string(source)

	for _, dir := range []string{"framework", "plugins"} {
		base := filepath.Join(root, dir)
		err := filepath.WalkDir(base, func(path string, info os.DirEntry, err error) error {
			if err != nil {
				if os.IsNotExist(err) {
					return nil
				}
				return err
			}
			if info.IsDir() {
				return nil
			}
			if filepath.Ext(path) != ".lua" {
				return nil
			}
			rel, err := filepath.Rel(root, path)
			if err != nil {
				return err
			}
			data, err := os.ReadFile(path)
			if err != nil {
				return err
			}
			modules[filepath.ToSlash(rel)] = string(data)
			return nil
		})
		if err != nil {
			return Bundle{}, err
		}
	}

	names := make([]string, 0, len(modules))
	for name := range modules {
		names = append(names, name)
	}
	sort.Strings(names)

	h := sha256.New()
	_, _ = io.WriteString(h, version.Protocol+"\n")
	for _, name := range names {
		_, _ = io.WriteString(h, name)
		_, _ = h.Write([]byte{0})
		_, _ = io.WriteString(h, modules[name])
		_, _ = h.Write([]byte{0})
	}
	bundle := Bundle{Revision: revision, Modules: modules, Hash: hex.EncodeToString(h.Sum(nil))}
	return bundle, nil
}

// Encode writes the bundle in the format `crates/mre-android/src/dev.rs` reads.
func Encode(w io.Writer, b Bundle) error {
	if _, err := fmt.Fprintf(w, "MRE/1 %d %d\n", b.Revision, len(b.Modules)); err != nil {
		return err
	}
	names := make([]string, 0, len(b.Modules))
	for name := range b.Modules {
		names = append(names, name)
	}
	sort.Strings(names)
	for _, name := range names {
		body := b.Modules[name]
		if _, err := fmt.Fprintf(w, "%d %s %d\n", len(name), name, len(body)); err != nil {
			return err
		}
		if _, err := io.WriteString(w, body); err != nil {
			return err
		}
		if _, err := io.WriteString(w, "\n"); err != nil {
			return err
		}
	}
	return nil
}

// Decode is the Go-side counterpart used by tests to verify the format the
// Android client receives.
func Decode(r io.Reader) (Bundle, error) {
	var b Bundle
	header, err := readLine(r)
	if err != nil {
		return b, err
	}
	parts := strings.Fields(header)
	if len(parts) != 3 || parts[0] != "MRE/1" {
		return b, fmt.Errorf("bad bundle header %q", header)
	}
	b.Revision, err = strconv.ParseUint(parts[1], 10, 64)
	if err != nil {
		return b, err
	}
	count, err := strconv.Atoi(parts[2])
	if err != nil {
		return b, err
	}
	b.Modules = make(map[string]string, count)
	for i := 0; i < count; i++ {
		line, err := readLine(r)
		if err != nil {
			return b, err
		}
		fields := strings.Fields(line)
		if len(fields) != 3 {
			return b, fmt.Errorf("bad module header %q", line)
		}
		nameLen, err := strconv.Atoi(fields[0])
		if err != nil {
			return b, err
		}
		if len(fields[1]) != nameLen {
			return b, fmt.Errorf("name length mismatch for %q", fields[1])
		}
		size, err := strconv.Atoi(fields[2])
		if err != nil {
			return b, err
		}
		body := make([]byte, size)
		if _, err := io.ReadFull(r, body); err != nil {
			return b, err
		}
		if _, err := readLine(r); err != nil {
			return b, err
		}
		b.Modules[fields[1]] = string(body)
	}
	return b, nil
}

func readLine(r io.Reader) (string, error) {
	var line strings.Builder
	buf := make([]byte, 1)
	for {
		_, err := io.ReadFull(r, buf)
		if err != nil {
			return "", err
		}
		if buf[0] == '\n' {
			return strings.TrimSuffix(line.String(), "\r"), nil
		}
		line.WriteByte(buf[0])
	}
}
