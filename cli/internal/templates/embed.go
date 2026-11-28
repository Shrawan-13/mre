// Package templates embeds the complete `mre new` project starter.
//
// The embedded copy is the canonical starter. `cli/internal/templates/app_test.go`
// compares the Android sources against the repository's android/ directory so
// the CI APK and generated user projects cannot silently drift apart.
package templates

import "embed"

//go:embed all:app
var FS embed.FS

// Prefix is the directory name inside FS that holds the starter project.
const Prefix = "app"
