package cmd

import (
	"context"
	"fmt"
	"net"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"

	"github.com/Shrawan-13/mre/cli/internal/project"
	"github.com/Shrawan-13/mre/cli/internal/serve"
	"github.com/spf13/cobra"
)

var devCmd = &cobra.Command{
	Use:   "dev",
	Short: "Serve the app with hot reload and print a pairing QR",
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
		root := filepath.Dir(manifest)
		port, _ := cmd.Flags().GetInt("port")
		if cmd.Flags().Changed("port") {
			cfg.DevPort = port
		} else {
			port = cfg.DevPort
		}
		bind, _ := cmd.Flags().GetString("bind")
		token, err := serve.NewToken()
		if err != nil {
			return err
		}
		server, err := serve.New(root, cfg, token)
		if err != nil {
			return err
		}
		secretDir := filepath.Join(root, ".mre")
		if err := os.MkdirAll(secretDir, 0o700); err != nil {
			return err
		}
		if err := os.WriteFile(filepath.Join(secretDir, "dev.token"), []byte(token), 0o600); err != nil {
			return err
		}

		addresses, err := serve.LANAddresses()
		if err != nil {
			return err
		}
		if len(addresses) == 0 {
			addresses = []string{"127.0.0.1"}
		}
		advertised, _ := cmd.Flags().GetString("host")
		if advertised == "" {
			advertised = addresses[0]
		}
		payload := serve.PairingURL(advertised, port, token, cfg.Name)
		out := cmd.OutOrStdout()
		fmt.Fprintf(out, "MRE dev · %s · revision %d\n", cfg.Name, server.Revision())
		fmt.Fprintf(out, "Advertising: %s (override with --host)\n", advertised)
		fmt.Fprintf(out, "Open on phone: %s\n", payload)
		if len(addresses) > 1 {
			fmt.Fprintf(out, "Other interfaces: %v\n", addresses[1:])
		}
		if qr, err := serve.QRTerminal(payload); err == nil {
			fmt.Fprintln(out, qr)
		}
		qrPath := filepath.Join(root, "mre-dev-qr.png")
		if png, err := serve.QRPNG(payload, 512); err == nil {
			if err := os.WriteFile(qrPath, png, 0o644); err == nil {
				fmt.Fprintf(out, "QR image: %s\n", qrPath)
			}
		}
		fmt.Fprintln(out, "Watching src/, framework/, plugins/ — save a file to reload.")

		ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
		defer cancel()
		if err := serve.Watch(root, func() {
			bundle, err := serve.Build(root, cfg, server.Revision()+1)
			if err != nil {
				fmt.Fprintf(out, "reload failed: %v\n", err)
				return
			}
			revision := server.Update(bundle)
			fmt.Fprintf(out, "reload → revision %d (%s)\n", revision, bundle.Hash[:12])
		}); err != nil {
			return err
		}
		addr := net.JoinHostPort(bind, fmt.Sprint(port))
		return server.Serve(ctx, addr)
	},
}

func init() {
	devCmd.Flags().IntP("port", "p", 0, "HTTP port (overrides mre.toml)")
	devCmd.Flags().String("bind", "0.0.0.0", "address to bind")
	devCmd.Flags().String("host", "", "host put in the QR (default: first LAN address)")
}
