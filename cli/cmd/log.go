package cmd

import (
	"bufio"
	"fmt"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strconv"
	"time"

	"github.com/Shrawan-13/mre/cli/internal/project"
	"github.com/spf13/cobra"
)

var logCmd = &cobra.Command{
	Use:   "log",
	Short: "Stream events from a connected dev client",
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
		tokenBytes, err := os.ReadFile(filepath.Join(root, ".mre", "dev.token"))
		if err != nil {
			return fmt.Errorf("no dev session: run `mre dev` first")
		}
		host, _ := cmd.Flags().GetString("host")
		port, _ := cmd.Flags().GetInt("port")
		if port == 0 {
			port = cfg.DevPort
		}
		since := 0
		client := &http.Client{Timeout: 30 * time.Second}
		for {
			query := url.Values{"since": {strconv.Itoa(since)}, "token": {string(tokenBytes)}}
			endpoint := fmt.Sprintf("http://%s:%d/api/logs?%s", host, port, query.Encode())
			request, err := http.NewRequest(http.MethodGet, endpoint, nil)
			if err != nil {
				return err
			}
			request.Header.Set("X-Mre-Token", string(tokenBytes))
			response, err := client.Do(request)
			if err != nil {
				return err
			}
			if response.StatusCode == http.StatusNoContent {
				response.Body.Close()
				continue
			}
			if response.StatusCode != http.StatusOK {
				response.Body.Close()
				return fmt.Errorf("dev server: %s", response.Status)
			}
			scanner := bufio.NewScanner(response.Body)
			for scanner.Scan() {
				line := scanner.Text()
				if index, err := strconv.Atoi(firstField(line)); err == nil {
					since = index
				}
				fmt.Fprintln(cmd.OutOrStdout(), line)
			}
			response.Body.Close()
			if err := scanner.Err(); err != nil {
				return err
			}
		}
	},
}

func firstField(line string) string {
	for i, r := range line {
		if r == ' ' {
			return line[:i]
		}
	}
	return line
}

func init() {
	logCmd.Flags().String("host", "127.0.0.1", "dev server host")
	logCmd.Flags().Int("port", 0, "dev server port (defaults to mre.toml)")
}
