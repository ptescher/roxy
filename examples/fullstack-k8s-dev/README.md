# Fullstack Kubernetes Development Example

A complete microservices example for demonstrating Roxy's Kubernetes integration features.

## Services

| Service | Namespace | Port | Description |
|---------|-----------|------|-------------|
| orders-api | backend | 3000 | NestJS REST API |
| config-service | backend | 8080 | Configuration service |
| postgres | database | 5432 | PostgreSQL database |
| kafka | messaging | 9092 | Kafka message broker |
| main-gateway | gateway-system | 80/443 | Gateway API entry point |

## Prerequisites

1. Local Kubernetes cluster (Docker Desktop, minikube, or kind)
2. Gateway API CRDs installed:
   ```sh
   kubectl apply -f https://github.com/kubernetes-sigs/gateway-api/releases/download/v1.2.0/standard-install.yaml
   ```

## Deploy

```sh
kubectl apply -k kubernetes/
```

## Build Local Images

The orders-api requires building locally since `imagePullPolicy: Never`:

```sh
docker build -t orders-api:latest ./nestjs-api
```

## Port Forwards

Roxy loads port forward configuration from `~/.roxy/port-forwards.yaml` or `./roxy-config/port-forwards.yaml`.

See `roxy-config/port-forwards.yaml` for an example:

```yaml
port_forwards:
  - name: orders-api
    namespace: backend
    service: orders-api
    remote_port: 3000
    local_port: 3000
    auto_start: true
```

Port forwards appear as dotted lines from "Roxy" to services in the traffic flow diagram.

## Using with Roxy

1. Start Roxy and select the K8s tab
2. Choose your context (e.g., `docker-desktop`)
3. Select a namespace or "All Namespaces"
4. View the traffic flow: Internet → Gateway → HTTPRoute → Service
5. Port forwards from config appear as Roxy → Service (dotted lines)
