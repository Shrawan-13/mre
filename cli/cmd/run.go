package cmd

import (
	"os/exec"
	"path/filepath"

	"github.com/Shrawan-13/mre/cli/internal/engine"
	"github.com/Shrawan-13/mre/cli/internal/project"
	"github.com/spf13/cobra"
)

var runCmd = &cobra.Command{
	Use:   "run",
	Short: "Run the app in a desktop window",
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
		root := dirOf(manifest)
		engineDir, err := engine.Find(root)
		if err != nil {
			return err
		}
		cargo, err := exec.LookPath("cargo")
		if err != nil {
			return err
		}
		entry := cfg.Entry
		if !filepath.IsAbs(entry) {
			entry = filepath.Join(root, filepath.FromSlash(entry))
		}
		run := exec.Command(cargo, "run", "-p", "mre-window", "--bin", "mre", "--", entry)
		run.Dir = engineDir
		run.Stdin, run.Stdout, run.Stderr = cmd.InOrStdin(), cmd.OutOrStdout(), cmd.ErrOrStderr()
		return run.Run()
	},
}
