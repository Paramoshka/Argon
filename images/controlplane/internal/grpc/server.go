package grpc

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"fmt"
	"net"
	"sort"
	"strings"

	argonpb "argon/internal/gen/argonpb/argon"
	"argon/internal/model"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/keepalive"
	"sigs.k8s.io/controller-runtime/pkg/log"
)

type Server struct {
	argonpb.UnimplementedConfigDiscoveryServer
	Hub       *StreamHub
	addr      string
	Bundle    *Bundle
	tlsConfig *tls.Config
}

func NewServer(h *StreamHub, addr, commonName string) (*Server, error) {
	dnsSANs := buildDNSSANs(commonName)
	bundle, err := NewGRPCServerCerts(commonName, dnsSANs, []net.IP{net.ParseIP("127.0.0.1")})
	if err != nil {
		return nil, err
	}

	cert, err := tls.X509KeyPair(bundle.ServerCertPEM, bundle.ServerKeyPEM)
	if err != nil {
		return nil, err
	}

	clientCAPool := x509.NewCertPool()
	clientCAPool.AppendCertsFromPEM(bundle.CACertPEM)

	tlsCfg := &tls.Config{
		Certificates: []tls.Certificate{cert},
		MinVersion:   tls.VersionTLS12,
	}

	tlsCfg.ClientCAs = clientCAPool
	tlsCfg.ClientAuth = tls.RequireAndVerifyClientCert

	return &Server{
		Hub:       h,
		addr:      addr,
		Bundle:    bundle,
		tlsConfig: tlsCfg,
	}, nil
}

func buildDNSSANs(commonName string) []string {
	seen := map[string]struct{}{}
	add := func(name string) {
		if name == "" {
			return
		}
		if _, ok := seen[name]; ok {
			return
		}
		seen[name] = struct{}{}
	}

	add(commonName)

	if strings.Contains(commonName, ".") {
		parts := strings.Split(commonName, ".")
		if len(parts) >= 2 {
			svc := parts[0]
			ns := parts[1]
			add(svc)
			add(fmt.Sprintf("%s.%s.svc", svc, ns))
			add(fmt.Sprintf("%s.%s.svc.cluster.local", svc, ns))
		}
	}

	result := make([]string, 0, len(seen))
	for name := range seen {
		result = append(result, name)
	}

	sort.Strings(result)

	return result
}

func (s *Server) Watch(req *argonpb.WatchRequest, stream argonpb.ConfigDiscovery_WatchServer) error {
	id, ch, last := s.Hub.Add()
	defer s.Hub.Remove(id)

	if last.Version != "" {
		if err := stream.Send(toPbSnapshot(last)); err != nil {
			return err
		}
	}

	ctx := stream.Context()
	for {
		select {
		case snap := <-ch:
			if err := stream.Send(toPbSnapshot(snap)); err != nil {
				return err
			}
		case <-ctx.Done():
			return ctx.Err()
		}
	}
}

func (s *Server) RunGRPC(ctx context.Context) error {

	lis, err := net.Listen("tcp", s.addr)
	if err != nil {
		return err
	}

	ka := keepalive.ServerParameters{
		Time:    30_000_000_000, // 30s
		Timeout: 10_000_000_000, // 10s
	}

	creds := grpc.Creds(credentials.NewTLS(s.tlsConfig))
	server := grpc.NewServer(creds, grpc.KeepaliveParams(ka))
	argonpb.RegisterConfigDiscoveryServer(server, s)

	go func() {
		<-ctx.Done()
		server.GracefulStop()
	}()

	return server.Serve(lis)
}

// ===== model -> pb =====

func toPbSnapshot(in model.Snapshot) *argonpb.Snapshot {
	logger := log.FromContext(context.Background()) // ctrl-runtime logger

	pb := &argonpb.Snapshot{
		Version:            in.Version,
		ControllerId:       in.ControllerID,
		IngressClassName:   in.IngressClassName,
		GeneratedAtUnixSec: in.GeneratedAtUnixSec,
		ResourceVersions:   in.ResourceVersions,
		Routes:             make([]*argonpb.Route, 0, len(in.Routes)),
		Clusters:           make([]*argonpb.Cluster, 0, len(in.Clusters)),
		ServerTls:          make([]*argonpb.ServerTlsBundle, 0),
	}
	for _, r := range in.Routes {
		pb.Routes = append(pb.Routes, &argonpb.Route{
			Host: r.Host, Path: r.Path, PathType: string(*r.PathType),
			Cluster: r.Cluster, Priority: int32(r.Priority),
		})
	}

	for _, c := range in.Clusters {
		pc := &argonpb.Cluster{
			Name: c.Name, LbPolicy: string(c.LBPolicy),
			TimeoutMs: c.TimeoutMs, Retries: c.Retries, BackendProtocol: c.BackendProtocol,
		}
		for _, e := range c.Endpoints {
			pc.Endpoints = append(pc.Endpoints, &argonpb.Endpoint{
				Address: e.Address, Port: e.Port, Weight: e.Weight,
				Zone: e.Zone, Region: e.Region,
			})
		}
		for _, rw := range c.RewriteHeaders {
			pc.RequestHeaders = append(pc.RequestHeaders, &argonpb.HeaderRewrite{
				Name:  rw.Name,
				Mode:  string(rw.Mode),
				Value: rw.Value,
			})
		}
		pb.Clusters = append(pb.Clusters, pc)
	}

	for _, cert := range in.TLS {
		pb.ServerTls = append(pb.ServerTls, &argonpb.ServerTlsBundle{
			Name:         cert.Name,
			Sni:          cert.Sni,
			CertPem:      cert.CertPem,
			KeyPem:       cert.KeyPem,
			NotAfterUnix: cert.NotAfterUnix.Unix(),
			Version:      cert.Version,
		})

		logger.V(1).Info("TLS certs in gRPC", "name: ", cert.Name, "SNI: ", cert.Sni)
	}

	logger.V(1).Info("count TLS certs", "count", len(pb.ServerTls))
	return pb
}
