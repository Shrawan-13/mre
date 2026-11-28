package cmd

import (
	"fmt"
	"os/exec"

	"github.com/Shrawan-13/mre/cli/internal/engine"
	"github.com/Shrawan-13/mre/cli/internal/project"
	"github.com/spf13/cobra"
)

var buildCmd = &cobra.Command{
	Use:       "build <android|desktop>",
	Short:     "Build an installable app",
	Args:      cobra.MatchAll(cobra.ExactArgs(1), cobra.OnlyValidArgs),
	ValidArgs: []string{"android", "desktop"},
	RunE: func(cmd *cobra.Command, args []string) error {
		manifest, err := project.Find(".")
		if err != nil {
			return err
		}
		if _, err := project.Load(manifest); err != nil {
			return err
		}
		root := dirOf(manifest)
		engineDir, err := engine.Find(root)
		if err != nil {
			return err
		}
		switch args[0] {
		case "android":
			prod, _ := cmd.Flags().GetBool("prod")
			apk, err := engine.BuildAndroid(engineDir, root, prod)
			if err != nil {
				return err
			}
			fmt.Fprintf(cmd.OutOrStdout(), "APK: %s\n", apk)
			fmt.Fprintln(cmd.OutOrStdout(), "Install with: adb install -r "+apk)
		case "desktop":
			cargo, err := exec.LookPath("cargo")
			if err != nil {
				return err
			}
			build := exec.Command(cargo, "build", "--release", "-p", "mre-window")
			build.Dir = engineDir
			build.Stdout, build.Stderr = cmd.OutOrStdout(), cmd.ErrOrStderr()
			return build.Run()
		}
		return nil
	},
}

func init() {
	buildCmd.Flags().Bool("prod", false, "build the release APK instead of the QR dev client")
}
