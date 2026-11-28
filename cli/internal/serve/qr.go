package serve

import (
	"fmt"
	"net"
	"net/url"

	qrcode "github.com/skip2/go-qrcode"
)

// LANAddresses returns the addresses a phone on the same network can reach.
func LANAddresses() ([]string, error) {
	interfaces, err := net.Interfaces()
	if err != nil {
		return nil, err
	}
	var out []string
	seen := map[string]bool{}
	for _, iface := range interfaces {
		if iface.Flags&net.FlagUp == 0 || iface.Flags&net.FlagLoopback != 0 {
			continue
		}
		addresses, err := iface.Addrs()
		if err != nil {
			continue
		}
		for _, address := range addresses {
			var ip net.IP
			switch value := address.(type) {
			case *net.IPNet:
				ip = value.IP.To4()
			case *net.IPAddr:
				ip = value.IP.To4()
			}
			if ip == nil || seen[ip.String()] {
				continue
			}
			seen[ip.String()] = true
			out = append(out, ip.String())
		}
	}
	return out, nil
}

// PairingURL is the payload scanned by the dev client.
func PairingURL(host string, port int, token, project string) string {
	query := url.Values{}
	query.Set("host", host)
	query.Set("port", fmt.Sprint(port))
	query.Set("token", token)
	query.Set("project", project)
	return "mre://dev?" + query.Encode()
}

// QRPNG renders a scannable PNG. RecoveryLevel Medium tolerates a logo or a
// slightly dirty phone camera without making the modules too dense.
func QRPNG(payload string, size int) ([]byte, error) {
	return qrcode.Encode(payload, qrcode.Medium, size)
}

// QRTerminal renders a compact QR for terminals that support block characters.
func QRTerminal(payload string) (string, error) {
	code, err := qrcode.New(payload, qrcode.Medium)
	if err != nil {
		return "", err
	}
	return code.ToSmallString(false), nil
}
