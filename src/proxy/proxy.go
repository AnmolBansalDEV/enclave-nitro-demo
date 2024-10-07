package main

// Code mostly taken from:
// https://github.com/containers/gvisor-tap-vsock/blob/main/cmd/vm/main_linux.go

import (
	"encoding/binary"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"os"
	"time"

	"github.com/containers/gvisor-tap-vsock/pkg/transport"
	"github.com/milosgajdos/tenus"
	"github.com/songgao/water"
	"github.com/vishvananda/netlink"
)

var (
	frameLen     = 0xffff
	frameSizeLen = 2
	elog      = log.New(os.Stderr, "proxy: ", log.Ldate|log.Ltime|log.LUTC|log.Lshortfile)
	parentCID = 3
	mac          = "ba:aa:ad:c0:ff:ee"
	ifaceTap     = "tap0"
	defaultGw    = "192.168.127.1"
	addrTap      = "192.168.127.2/24"
	HostProxyPort = 1024
)

var ourWaterParams = water.PlatformSpecificParams{
	Name:       ifaceTap,
	MultiQueue: true,
}

func main()  {
	elog.Println("Starting the proxy...")
	// var defaultCfg = Config{
	// 	FQDN:          "holonym.id",
	// 	ExtPubPort:    50000,
	// 	ExtPrivPort:   50001,
	// 	IntPort:       50002,
	// 	HostProxyPort: 1024,
	// 	UseACME:       false,
	// 	Debug:         true,
	// 	FdCur:         1024,
	// 	FdMax:         4096,
	// 	WaitForApp:    true,
	// }
	
	runNetworking(make(chan  struct{}))
}

// runNetworking calls the function that sets up our networking environment.
// If anything fails, we try again after a brief wait period.
func runNetworking(stop chan struct{}) {
	var err error
	for {
		if err = setupNetworking(stop); err == nil {
			return
		}
		elog.Println("error", err)
		time.Sleep(time.Second)
	}
}

// setupNetworking sets up the enclave's networking environment.  In
// particular, this function:
//
//  1. Creates a TAP device.
//  2. Set up networking links.
//  3. Establish a connection with the proxy running on the host.
//  4. Spawn goroutines to forward traffic between the TAP device and the proxy
//     running on the host.
func setupNetworking(stop chan struct{}) error {
	// Establish connection with the proxy running on the EC2 host.
	endpoint := fmt.Sprintf("vsock://%d:%d/connect", parentCID, HostProxyPort)
	conn, path, err := transport.Dial(endpoint)
	if err != nil {
		return fmt.Errorf("failed to connect to host: %w", err)
	}
	defer conn.Close()
	elog.Println("Established connection with EC2 host.")

	req, err := http.NewRequest(http.MethodPost, path, nil)
	if err != nil {
		return fmt.Errorf("failed to create POST request: %w", err)
	}
	if err := req.Write(conn); err != nil {
		return fmt.Errorf("failed to send POST request to host: %w", err)
	}
	elog.Println("Sent HTTP request to EC2 host.")

	// Create a TAP interface.
	tap, err := water.New(water.Config{
		DeviceType:             water.TAP,
		PlatformSpecificParams: ourWaterParams,
	})
	if err != nil {
		return fmt.Errorf("failed to create tap device: %w", err)
	}
	defer tap.Close()
	elog.Println("Created TAP device.")

	// Configure IP address, MAC address, MTU, default gateway, and DNS.
	if err = configureTapIface(); err != nil {
		return fmt.Errorf("failed to configure tap interface: %w", err)
	}
	if err = writeResolvconf(); err != nil {
		return fmt.Errorf("failed to create resolv.conf: %w", err)
	}

	// Set up networking links.
	if err := linkUp(); err != nil {
		return fmt.Errorf("failed to set MAC address: %w", err)
	}
	elog.Println("Created networking link.")

	// Spawn goroutines that forward traffic.
	errCh := make(chan error, 1)
	go tx(conn, tap, errCh)
	go rx(conn, tap, errCh)
	elog.Println("Started goroutines to forward traffic.")
	select {
	case err := <-errCh:
		return err
	case <-stop:
		elog.Printf("Shutting down networking.")
		return nil
	}
}

// writeResolvconf creates our resolv.conf and adds a nameserver.
func writeResolvconf() error {
	// A Nitro Enclave's /etc/resolv.conf is a symlink to
	// /run/resolvconf/resolv.conf.  As of 2022-11-21, the /run/ directory
	// exists but not its resolvconf/ subdirectory.
	dir := "/run/resolvconf/"
	file := dir + "resolv.conf"

	if err := os.MkdirAll(dir, 0755); err != nil {
		return fmt.Errorf("failed to create directories: %w", err)
	}

	// Our default gateway -- gvproxy -- also operates a DNS resolver.
	c := fmt.Sprintf("nameserver %s\n", defaultGw)
	if err := os.WriteFile(file, []byte(c), 0644); err != nil {
		return fmt.Errorf("failed to write file: %w", err)
	}

	return nil
}

// configureTapIface configures our TAP interface by assigning it a MAC
// address, IP address, and link MTU.  We could have used DHCP instead but that
// brings with it unnecessary complexity and attack surface.
func configureTapIface() error {
	l, err := tenus.NewLinkFrom(ifaceTap)
	if err != nil {
		return fmt.Errorf("failed to retrieve link: %w", err)
	}

	addr, network, err := net.ParseCIDR(addrTap)
	if err != nil {
		return fmt.Errorf("failed to parse CIDR: %w", err)
	}
	if err = l.SetLinkIp(addr, network); err != nil {
		return fmt.Errorf("failed to set link address: %w", err)
	}

	if err := l.SetLinkMTU(1500); err != nil {
		return fmt.Errorf("failed to set link MTU: %w", err)
	}

	if err := l.SetLinkMacAddress(mac); err != nil {
		return fmt.Errorf("failed to set MAC address: %w", err)
	}

	if err := l.SetLinkUp(); err != nil {
		return fmt.Errorf("failed to bring up link: %w", err)
	}

	gw := net.ParseIP(defaultGw)
	if err := l.SetLinkDefaultGw(&gw); err != nil {
		return fmt.Errorf("failed to set default gateway: %w", err)
	}

	return nil
}

func linkUp() error {
	link, err := netlink.LinkByName(ifaceTap)
	if err != nil {
		return err
	}
	if mac == "" {
		return netlink.LinkSetUp(link)
	}
	hw, err := net.ParseMAC(mac)
	if err != nil {
		return err
	}
	if err := netlink.LinkSetHardwareAddr(link, hw); err != nil {
		return err
	}
	return netlink.LinkSetUp(link)
}

func rx(conn io.Writer, tap io.Reader, errCh chan error) {
	elog.Println("Waiting for frames from enclave application.")
	buf := make([]byte, frameSizeLen+frameLen) // Two bytes for the frame length plus the frame itself

	for {
		n, err := tap.Read([]byte(buf[frameSizeLen:]))
		if err != nil {
			errCh <- fmt.Errorf("failed to read payload from enclave application: %w", err)
			return
		}

		binary.LittleEndian.PutUint16(buf[:frameSizeLen], uint16(n))
		m, err := conn.Write(buf[:frameSizeLen+n])
		if err != nil {
			errCh <- fmt.Errorf("failed to write payload to host: %w", err)
			return
		}
		m = m - frameSizeLen
		if m != n {
			errCh <- fmt.Errorf("wrote %d instead of %d bytes to host", m, n)
			return
		}
	}
}

func tx(conn io.Reader, tap io.Writer, errCh chan error) {
	elog.Println("Waiting for frames from host.")
	buf := make([]byte, frameSizeLen+frameLen) // Two bytes for the frame length plus the frame itself

	for {
		n, err := io.ReadFull(conn, buf[:frameSizeLen])
		if err != nil {
			errCh <- fmt.Errorf("failed to read length from host: %w", err)
			return
		}
		if n != frameSizeLen {
			errCh <- fmt.Errorf("received unexpected length %d", n)
			return
		}
		size := int(binary.LittleEndian.Uint16(buf[:frameSizeLen]))

		n, err = io.ReadFull(conn, buf[frameSizeLen:size+frameSizeLen])
		if err != nil {
			errCh <- fmt.Errorf("failed to read payload from host: %w", err)
			return
		}
		if n == 0 || n != size {
			errCh <- fmt.Errorf("expected payload of size %d but got %d", size, n)
			return
		}

		m, err := tap.Write(buf[frameSizeLen : n+frameSizeLen])
		if err != nil {
			errCh <- fmt.Errorf("failed to write payload to enclave application: %w", err)
			return
		}
		if m != n {
			errCh <- fmt.Errorf("wrote %d instead of %d bytes to host", m, n)
			return
		}
	}
}
