package cmd

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sort"

	"github.com/Shrawan-13/mre/cli/internal/engine"
	"github.com/Shrawan-13/mre/cli/internal/project"
	"github.com/spf13/cobra"
)

var testCmd = &cobra.Command{
	Use:   "test",
	Short: "Compile-check Lua files and run the engine test suite",
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
		files, err := luaFiles(root, cfg)
		if err != nil {
			return err
		}
		engineDir, err := engine.Find(root)
		if err != nil {
			return err
		}
		cargo, err := exec.LookPath("cargo")
		if err != nil {
			return err
		}
		check := exec.Command(cargo, "run", "-q", "-p", "mre-lua", "--bin", "mre-luac", "--")
		check.Args = append(check.Args, files...)
		check.Dir = engineDir
		check.Stdout, check.Stderr = cmd.OutOrStdout(), cmd.ErrOrStderr()
		if err := check.Run(); err != nil {
			return fmt.Errorf("Lua compile check failed: %w", err)
		}
		fmt.Fprintf(cmd.OutOrStdout(), "Lua: %d files OK\n", len(files))
		if skip, _ := cmd.Flags().GetBool("no-engine"); skip {
			return nil
		}
		tests := exec.Command(cargo, "test", "--workspace")
		tests.Dir = engineDir
		tests.Stdout, tests.Stderr = cmd.OutOrStdout(), cmd.ErrOrStderr()
		return tests.Run()
	},
}

func luaFiles(root string, cfg project.Config) ([]string, error) {
	var files []string
	files = append(files, filepath.Join(root, filepath.FromSlash(cfg.Entry)))
	for _, dir := range []string{"framework", "plugins", "tests"} {
		base := filepath.Join(root, dir)
		_ = filepath.WalkDir(base, func(path string, info os.DirEntry, err error) error {
			if err != nil || info.IsDir() || filepath.Ext(path) != ".lua" {
				return nil
			}
			files = append(files, path)
			return nil
		})
	}
	sort.Strings(files)
	return files, nil
}

func init() {
	testCmd.Flags().Bool("no-engine", false, "only compile-check project Lua files")
}
