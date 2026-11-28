package cmd

import (
	"os/exec"

	"github.com/spf13/cobra"
)

var devicesCmd = &cobra.Command{
	Use:   "devices",
	Short: "List connected Android devices",
	Args:  cobra.NoArgs,
	RunE: func(cmd *cobra.Command, _ []string) error {
		adb, err := exec.LookPath("adb")
		if err != nil {
			return err
		}
		run := exec.Command(adb, "devices", "-l")
		run.Stdout, run.Stderr = cmd.OutOrStdout(), cmd.ErrOrStderr()
		return run.Run()
	},
}
