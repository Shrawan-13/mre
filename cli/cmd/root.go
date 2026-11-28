// Package cmd wires the mre command tree. It follows the Cobra user guide:
// `main` does nothing but call Execute, commands live in their own files, and
// errors are printed exactly once by the root so usage is not dumped for a
// runtime failure.
package cmd

import (
	"fmt"
	"os"

	"github.com/Shrawan-13/mre/cli/internal/version"
	"github.com/spf13/cobra"
)

var verbose bool

var rootCmd = &cobra.Command{
	Use:   "mre",
	Short: "Build, run and hot-reload MRE apps",
	Long: `mre is the toolchain for MRE, a Lua-first UI framework with a Rust engine.

Start with:

    mre new my-app
    cd my-app
    mre dev

Scan the QR with the MRE dev client APK to see changes on your phone as you
save Lua files. Release apps can be produced with:

    mre build android`,
	Version:       version.CLI,
	SilenceUsage:  true,
	SilenceErrors: true,
}

// Execute runs the CLI and reports errors once.
func Execute() {
	if err := rootCmd.Execute(); err != nil {
		fmt.Fprintln(os.Stderr, "mre:", err)
		os.Exit(1)
	}
}

func init() {
	rootCmd.PersistentFlags().BoolVarP(&verbose, "verbose", "V", false, "log detailed diagnostics")
	rootCmd.AddCommand(
		newCmd,
		devCmd,
		runCmd,
		buildCmd,
		testCmd,
		devicesCmd,
		logCmd,
		doctorCmd,
		pluginCmd,
		completionCmd,
		versionCmd,
	)
}

var versionCmd = &cobra.Command{
	Use:   "version",
	Short: "Print CLI and engine versions",
	Args:  cobra.NoArgs,
	Run: func(cmd *cobra.Command, _ []string) {
		fmt.Fprintf(cmd.OutOrStdout(), "mre %s (engine %s, dev protocol %s)\n",
			version.CLI, version.Engine, version.Protocol)
	},
}

var completionCmd = &cobra.Command{
	Use:       "completion [bash|zsh|fish|powershell]",
	Short:     "Generate a shell completion script",
	Args:      cobra.MatchAll(cobra.ExactArgs(1), cobra.OnlyValidArgs),
	ValidArgs: []string{"bash", "zsh", "fish", "powershell"},
	RunE: func(cmd *cobra.Command, args []string) error {
		out := cmd.OutOrStdout()
		switch args[0] {
		case "bash":
			return rootCmd.GenBashCompletionV2(out, true)
		case "zsh":
			return rootCmd.GenZshCompletion(out)
		case "fish":
			return rootCmd.GenFishCompletion(out, true)
		case "powershell":
			return rootCmd.GenPowerShellCompletionWithDesc(out)
		}
		return nil
	},
}
