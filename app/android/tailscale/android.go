//go:build android

package main

//#include <errno.h>
import "C"

import (
	"context"
	"encoding/json"
	"net"
	"os"
	"sync"
	"tailscale.com/ipn"
	"tailscale.com/net/netmon"
	"tailscale.com/types/logger"
	"time"
)

// Android invokes the in-memory LocalAPI, keeping authentication credentials
// inside tsnet rather than exposing its administrative HTTP interface to Java.
//
//export tailscale_android_login
func tailscale_android_login(sd C.int) C.int {
	s := getServer(sd)
	if s == nil {
		return C.EBADF
	}
	lc, err := s.s.LocalClient()
	if err != nil {
		return s.recErr(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	return s.recErr(lc.StartLoginInteractive(ctx))
}

//export tailscale_android_logout
func tailscale_android_logout(sd C.int) C.int {
	s := getServer(sd)
	if s == nil {
		return C.EBADF
	}
	lc, err := s.s.LocalClient()
	if err != nil {
		return s.recErr(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	return s.recErr(lc.Logout(ctx))
}

// Android app UIDs cannot use Go's netlink-based interface enumeration.
// Java supplies the same addresses through the supported SDK API.
var androidInterfaces struct {
	sync.RWMutex
	list []netmon.Interface
}

func init() {
	netmon.RegisterInterfaceGetter(func() ([]netmon.Interface, error) {
		androidInterfaces.RLock()
		defer androidInterfaces.RUnlock()
		return append([]netmon.Interface(nil), androidInterfaces.list...), nil
	})
}

//export tailscale_android_interfaces
func tailscale_android_interfaces(value *C.char) C.int {
	var entries []struct {
		Name              string
		Index, MTU, Flags int
		Addresses         []string
	}
	if json.Unmarshal([]byte(C.GoString(value)), &entries) != nil {
		return C.EINVAL
	}
	list := make([]netmon.Interface, 0, len(entries))
	for _, entry := range entries {
		addrs := make([]net.Addr, 0, len(entry.Addresses))
		for _, value := range entry.Addresses {
			ip, subnet, err := net.ParseCIDR(value)
			if err != nil {
				return C.EINVAL
			}
			subnet.IP = ip
			addrs = append(addrs, subnet)
		}
		list = append(list, netmon.Interface{Interface: &net.Interface{Name: entry.Name, Index: entry.Index, MTU: entry.MTU, Flags: net.Flags(entry.Flags)}, AltAddrs: addrs})
	}
	androidInterfaces.Lock()
	androidInterfaces.list = list
	androidInterfaces.Unlock()
	return 0
}

//export tailscale_android_name
func tailscale_android_name(sd C.int, value *C.char) C.int {
	s := getServer(sd)
	if s == nil {
		return C.EBADF
	}
	lc, err := s.s.LocalClient()
	if err != nil {
		return s.recErr(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	_, err = lc.EditPrefs(ctx, &ipn.MaskedPrefs{Prefs: ipn.Prefs{Hostname: C.GoString(value)}, HostnameSet: true})
	return s.recErr(err)
}

//export tailscale_android_prepare
func tailscale_android_prepare(sd C.int) C.int {
	s := getServer(sd)
	if s == nil {
		return C.EBADF
	}
	s.s.UserLogf = logger.Discard // Auth URLs are delivered only to the login UI.
	return s.recErr(os.Setenv("TS_LOGS_DIR", s.s.Dir))
}

//export tailscale_android_poll
func tailscale_android_poll(sd C.int) {
	s := getServer(sd)
	if s != nil && s.started {
		s.s.Sys().NetMon.Get().Poll()
	}
}
