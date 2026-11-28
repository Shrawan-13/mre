package cmd

import (
	"fmt"
	"path/filepath"

	"github.com/Shrawan-13/mre/cli/internal/project"
	"github.com/Shrawan-13/mre/cli/internal/scaffold"
	"github.com/Shrawan-13/mre/cli/internal/version"
	"github.com/spf13/cobra"
)

var newCmd = &cobra.Command{
	Use:   "new <name>",
	Short: "Create a new MRE project",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		name := args[0]
		dir, _ := cmd.Flags().GetString("dir")
		if dir == "" {
			dir = name
		}
		cfg := project.Default(name)
		cfg.Engine = version.Engine
		cfg.AndroidAppID = "dev.mre." + sanitize(name)
		cfg.AndroidLabel = name
		if err := scaffold.Create(dir, cfg); err != nil {
			return err
		}
		manifest, err := filepath.Abs(filepath.Join(dir, "mre.toml"))
		if err != nil {
			return err
		}
		fmt.Fprintf(cmd.OutOrStdout(), "Created %s\n\n", manifest)
		fmt.Fprintln(cmd.OutOrStdout(), "Next:")
		fmt.Fprintf(cmd.OutOrStdout(), "  cd %s\n  mre dev\n", dir)
		return nil
	},
}

func sanitize(name string) string {
	out := make([]rune, 0, len(name))
	for _, r := range name {
		switch {
		case r >= 'a' && r <= 'z', r >= '0' && r <= '9':
			out = append(out, r)
		case r >= 'A' && r <= 'Z':
			out = append(out, r+('a'-'A'))
		default:
			out = append(out, '_')
		}
	}
	if len(out) == 0 {
		return "app"
	}
	return string(out)
}

func init() {
	newCmd.Flags().String("dir", "", "target directory (defaults to <name>)")
}
