package templates

import (
	"io/fs"
	"os"
	"path/filepath"
	"testing"
)

// The repository's android/ directory is the project CI builds. Keep it byte
// identical to the embedded starter so `mre new` projects build the same way.
func TestAndroidTemplateMatchesRepositoryTemplate(t *testing.T) {
	files := []string{
		"android/build.gradle",
		"android/settings.gradle",
		"android/app/build.gradle",
		"android/app/src/main/AndroidManifest.xml",
		"android/app/src/main/java/dev/mre/MreActivity.java",
		"android/app/src/main/java/dev/mre/MrePlugin.java",
		"android/app/src/main/java/dev/mre/MrePluginRegistry.java",
		"android/app/src/main/java/dev/mre/Plugins.java",
		"android/app/src/main/res/values/themes.xml",
		"android/app/src/dev/AndroidManifest.xml",
		"android/app/src/dev/java/dev/mre/dev/DevPairActivity.java",
		"android/app/src/dev/res/xml/network_security_config.xml",
		"android/app/src/prod/AndroidManifest.xml",
		"framework/init.lua",
		"framework/nav.lua",
	}
	repo := filepath.Join("..", "..", "..")
	for _, file := range files {
		want, err := os.ReadFile(filepath.Join(repo, filepath.FromSlash(file)))
		if err != nil {
			t.Fatalf("repository template %s: %v", file, err)
		}
		got, err := fs.ReadFile(FS, Prefix+"/"+file)
		if err != nil {
			t.Fatalf("embedded template %s: %v", file, err)
		}
		if string(got) != string(want) {
			t.Errorf("%s drifted from the repository template", file)
		}
	}
}
