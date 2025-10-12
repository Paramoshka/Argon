package grpc

import (
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/pem"
	"math/big"
	"net"
	"time"
)

type Bundle struct {
	CACertPEM     []byte
	CAKeyPEM      []byte
	ServerCertPEM []byte
	ServerKeyPEM  []byte
}

func NewGRPCServerCerts(commonName string, dnsSANs []string, ipSANs []net.IP) (*Bundle, error) {
	// === 1) CA ===
	caTmpl := &x509.Certificate{
		SerialNumber:          randSerial(),
		Subject:               pkix.Name{Organization: []string{"Company, INC."}, CommonName: "Local Dev CA"},
		NotBefore:             time.Now().Add(-5 * time.Minute),
		NotAfter:              time.Now().AddDate(3, 0, 0),
		IsCA:                  true,
		KeyUsage:              x509.KeyUsageCertSign | x509.KeyUsageCRLSign,
		BasicConstraintsValid: true,
	}
	caKey, _ := rsa.GenerateKey(rand.Reader, 4096)
	caDER, _ := x509.CreateCertificate(rand.Reader, caTmpl, caTmpl, &caKey.PublicKey, caKey)

	caCertPEM := pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: caDER})
	caKeyPEM := pem.EncodeToMemory(&pem.Block{Type: "RSA PRIVATE KEY", Bytes: x509.MarshalPKCS1PrivateKey(caKey)})

	// === 2) Server leaf ===
	srvTmpl := &x509.Certificate{
		SerialNumber:          randSerial(),
		Subject:               pkix.Name{Organization: []string{"Company, INC."}, CommonName: commonName},
		NotBefore:             time.Now().Add(-5 * time.Minute),
		NotAfter:              time.Now().AddDate(1, 0, 0),
		ExtKeyUsage:           []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth, x509.ExtKeyUsageClientAuth},
		KeyUsage:              x509.KeyUsageDigitalSignature | x509.KeyUsageKeyEncipherment,
		BasicConstraintsValid: true,
		DNSNames:              dnsSANs,
		IPAddresses:           ipSANs,
	}
	srvKey, _ := rsa.GenerateKey(rand.Reader, 2048)
	srvDER, _ := x509.CreateCertificate(rand.Reader, srvTmpl, caTmpl, &srvKey.PublicKey, caKey)

	srvCertPEM := pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: srvDER})
	srvKeyPEM := pem.EncodeToMemory(&pem.Block{Type: "RSA PRIVATE KEY", Bytes: x509.MarshalPKCS1PrivateKey(srvKey)})

	return &Bundle{
		CACertPEM:     caCertPEM,
		CAKeyPEM:      caKeyPEM,
		ServerCertPEM: srvCertPEM,
		ServerKeyPEM:  srvKeyPEM,
	}, nil
}

func randSerial() *big.Int {
	limit := new(big.Int).Lsh(big.NewInt(1), 128)
	n, _ := rand.Int(rand.Reader, limit)
	return n
}
