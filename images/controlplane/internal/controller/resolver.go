package controller

import (
	"context"
	"net"
	"net/netip"
)

type DNSNamedResolver struct {
	resolver *net.Resolver
}

func NewDNSResolver() *DNSNamedResolver {
	return &DNSNamedResolver{
		resolver: net.DefaultResolver,
	}
}

func (d *DNSNamedResolver) ResolveNamed(ctx context.Context, name string) ([]netip.Addr, error) {
	return d.resolver.LookupNetIP(ctx, "ip", name)
}
