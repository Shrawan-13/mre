package cmd

import (
	"github.com/Shrawan-13/mre/cli/internal/project"
	"github.com/spf13/cobra"
)

// `mre check` is the fast, device-free Lua gate. It is the same compile pass
// `mre test` runs before the Rust suite, exposed on its own so it is obvious
// that app code can be validated without a phone or a Rust rebuild.
var checkCmd = &cobra.Command{
	Use:   "check",
	Short: "Compile-check every Lua file in the project",
	Args:  cobra.NoArgs,
	RunE: func(cmd *cobra.Command, _ []string) error {
		manifest, err := project.Find(".")
		if err != nil {
			return err
		}
		cfg, err := project.Load(manifest)
		if err != nil {
			return err
		}
		return checkLua(cmd, dirOf(manifest), cfg)
	},
}
