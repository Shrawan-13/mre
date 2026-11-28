package serve

import (
	"log"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/fsnotify/fsnotify"
)

// Watch watches the project's source directories and calls onChange after a
// short debounce. fsnotify events are a bitmask and editor saves are often a
// temp-file + rename, so we watch directories and filter by path.
func Watch(root string, onChange func()) error {
	watcher, err := fsnotify.NewWatcher()
	if err != nil {
		return err
	}

	var mu sync.Mutex
	timers := map[string]*time.Timer{}
	fire := func() {
		onChange()
	}

	go func() {
		for {
			select {
			case event, ok := <-watcher.Events:
				if !ok {
					return
				}
				if !event.Has(fsnotify.Write) && !event.Has(fsnotify.Create) &&
					!event.Has(fsnotify.Remove) && !event.Has(fsnotify.Rename) {
					continue
				}
				if ignored(event.Name) {
					continue
				}
				if event.Has(fsnotify.Create) {
					if info, err := os.Stat(event.Name); err == nil && info.IsDir() {
						addTree(watcher, event.Name)
					}
				}
				mu.Lock()
				timer := timers[event.Name]
				if timer == nil {
					timer = time.AfterFunc(120*time.Millisecond, fire)
					timers[event.Name] = timer
				} else {
					timer.Reset(120 * time.Millisecond)
				}
				mu.Unlock()
			case err, ok := <-watcher.Errors:
				if !ok {
					return
				}
				log.Printf("watch: %v", err)
			}
		}
	}()

	addTree(watcher, root)
	return nil
}

func ignored(path string) bool {
	parts := strings.Split(filepath.ToSlash(path), "/")
	for _, part := range parts {
		switch part {
		case ".git", ".mre", "target", "build", ".gradle", "node_modules":
			return true
		}
	}
	return strings.Contains(path, string(filepath.Separator)+"build"+string(filepath.Separator))
}

func addTree(watcher *fsnotify.Watcher, root string) {
	_ = filepath.WalkDir(root, func(path string, info os.DirEntry, err error) error {
		if err != nil || !info.IsDir() || ignored(path) {
			return nil
		}
		_ = watcher.Add(path)
		return nil
	})
}
