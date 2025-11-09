# Gateway API samples

The manifests in this folder let you create every Gateway API object the Argon control-plane currently watches (`GatewayClass`, `Gateway`, `HTTPRoute`, `TCPRoute`, `TLSRoute`, `UDPRoute`, and `ReferenceGrant`). Apply them to any test cluster to make sure the controller reacts to Gateway API events before you wire in real traffic.

## Contents

| File | Description |
| ---- | ----------- |
| `basic-gateway.yaml` | Minimal end-to-end example that wires a single `GatewayClass` to four listeners (HTTP/TCP/TLS/UDP) and the matching route types. It also includes a stub `Service`, an echo `Deployment`, and a `ReferenceGrant` so that the routes can target a backend living in another namespace. |

## How to use the sample

1. **Pick a GatewayClass name.** Update `metadata.name` in the `GatewayClass` along with the `Gateway.spec.gatewayClassName` field so that both match the value you pass to the Argon controller (`--gateway-class` flag or config). Leave it empty only if the controller runs in wildcard mode.
2. **Set the controller name.** Adjust `spec.controllerName` so it matches the value that Argon reports (for example `argon.dev/gateway-controller`). The controller only reconciles GatewayClasses that declare the same name.
3. **Review listener ports and hostnames.** The sample uses `80`, `9000`, `9443`, and `5353` with fictional hostnames such as `http.sample.argon.local`. Change them to values that exist in your lab cluster.
4. **Re-use or swap the backend pods.** The sample ships with a tiny `Deployment` (Contour echo server) behind the `Service`. You can keep it for smoke tests or replace the selector/image/ports with your own workload.
5. Apply everything:  
   `kubectl apply -f docs/samples/gatewayapi/basic-gateway.yaml`
6. Watch the controller logs or metrics to confirm it processed the new objects.

When you are done, delete the sample namespace(s) to clean up:  
`kubectl delete -f docs/samples/gatewayapi/basic-gateway.yaml`
