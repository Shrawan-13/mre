// Package version is the single source of truth for the CLI version and the
// engine release it scaffolds against.
package version

// CLI is the semantic version of the mre command-line tool.
const CLI = "0.1.0"

// Engine is the engine release written into generated mre.toml files.
const Engine = "0.1.0"

// Protocol is the dev-server wire protocol revision enforced on both sides.
const Protocol = "mre-dev/1"
