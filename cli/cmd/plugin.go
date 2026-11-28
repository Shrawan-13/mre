package cmd

import (
	"fmt"

	"github.com/Shrawan-13/mre/cli/internal/project"
	"github.com/Shrawan-13/mre/cli/internal/scaffold"
	"github.com/spf13/cobra"
)

var pluginCmd = &cobra.Command{
	Use:   "plugin",
	Short: "Manage native plugins",
	Args:  cobra.NoArgs,
}

var pluginNewCmd = &cobra.Command{
	Use:   "new <name>",
	Short: "Scaffold a native plugin (Java on Android, Lua wrapper)",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		manifest, err := project.Find(".")
		if err != nil {
			return err
		}
		cfg, err := project.Load(manifest)
		if err != nil {
			return err
		}
		root := dirOf(manifest)
		if err := scaffold.AddPlugin(root, cfg, args[0]); err != nil {
			return err
		}
		fmt.Fprintf(cmd.OutOrStdout(), "Created plugin %q.\n", args[0])
		fmt.Fprintf(cmd.OutOrStdout(), "  Lua:    plugins/%s/init.lua\n", args[0])
		fmt.Fprintf(cmd.OutOrStdout(), "  Java:   plugins/%s/android/dev/mre/plugin/%sPlugin.java\n",
			args[0], scaffold.Export(args[0]))
		fmt.Fprintln(cmd.OutOrStdout(), "Call it with mre.platform_call(name, method, args).")
		return nil
	},
}

func init() {
	pluginCmd.AddCommand(pluginNewCmd)
}
