package grpc

import (
	"context"
	"net"

	argonpb "argon.github.io/ingress/internal/gen/argonpb/argon"
	. "argon.github.io/ingress/internal/model"
	"google.golang.org/grpc"
	"google.golang.org/grpc/keepalive"
)

type Server struct {
	argonpb.UnimplementedConfigDiscoveryServer
	Hub *StreamHub
}

func NewServer(h *StreamHub) *Server { return &Server{Hub: h} }

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

func RunGRPC(ctx context.Context, addr string, hub *StreamHub) error {
	lis, err := net.Listen("tcp", addr)
	if err != nil {
		return err
	}

	ka := keepalive.ServerParameters{
		Time:    30_000_000_000, // 30s
		Timeout: 10_000_000_000, // 10s
	}

	s := grpc.NewServer(grpc.KeepaliveParams(ka))
	argonpb.RegisterConfigDiscoveryServer(s, NewServer(hub))

	go func() {
		<-ctx.Done()
		s.GracefulStop()
	}()

	return s.Serve(lis)
}

// ===== model -> pb =====

func toPbSnapshot(in Snapshot) *argonpb.Snapshot {
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
			TimeoutMs: c.TimeoutMs, Retries: c.Retries,
		}
		for _, e := range c.Endpoints {
			pc.Endpoints = append(pc.Endpoints, &argonpb.Endpoint{
				Address: e.Address, Port: e.Port, Weight: e.Weight,
				Zone: e.Zone, Region: e.Region,
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
	}

	return pb
}
