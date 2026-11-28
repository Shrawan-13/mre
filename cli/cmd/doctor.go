package cmd

import (
	"fmt"
	"os/exec"

	"github.com/Shrawan-13/mre/cli/internal/engine"
	"github.com/Shrawan-13/mre/cli/internal/project"
	"github.com/spf13/cobra"
)

var doctorCmd = &cobra.Command{
	Use:   "doctor",
	Short: "Check the local toolchain and project manifest",
	Args:  cobra.NoArgs,
	RunE: func(cmd *cobra.Command, _ []string) error {
		out := cmd.OutOrStdout()
		failed := false
		for _, tool := range []string{"cargo", "gradle", "adb", "java"} {
			path, err := exec.LookPath(tool)
			if err != nil {
				fmt.Fprintf(out, "MISSING %-8s %v\n", tool, err)
				failed = true
			} else {
				fmt.Fprintf(out, "OK      %-8s %s\n", tool, path)
			}
		}
		manifest, err := project.Find(".")
		if err != nil {
			fmt.Fprintf(out, "MISSING project %v\n", err)
			failed = true
		} else {
			cfg, err := project.Load(manifest)
			if err != nil {
				fmt.Fprintf(out, "BAD     manifest %v\n", err)
				failed = true
			} else {
				fmt.Fprintf(out, "OK      project %s (engine %s)\n", cfg.Name, cfg.Engine)
			}
			engineDir, err := engine.Find(dirOf(manifest))
			if err != nil {
				fmt.Fprintf(out, "MISSING engine %v\n", err)
				failed = true
			} else {
				fmt.Fprintf(out, "OK      engine %s\n", engineDir)
			}
		}
		if _, err := engine.AndroidNDK(); err != nil {
			fmt.Fprintf(out, "MISSING NDK    %v\n", err)
			failed = true
		} else {
			fmt.Fprintln(out, "OK      NDK")
		}
		if failed {
			return fmt.Errorf("doctor found problems")
		}
		fmt.Fprintln(out, "Everything looks ready. Run `mre dev`.")
		return nil
	},
}
