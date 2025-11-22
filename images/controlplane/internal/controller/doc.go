// Package controller watches Gateway API resources (Gateway, GatewayClass, and
// Route types) and builds in-memory configuration snapshots consumed by the
// Argon data plane. The main entry point is ArgonConfigReconcilerGatewayAPI,
// which filters Gateways by the configured GatewayClass, resolves addresses,
// parses listeners (including TLS settings), and assembles TLS secrets.
package controller

