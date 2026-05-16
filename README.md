```markdown
# Silex: High-Performance Cloud-Native Ingress & Operator

Silex is a ultra-lightweight, zero-overhead Custom Ingress Controller and Operator architecture designed for edge computing and resource-constrained environments. By shifting the request routing mechanics to raw byte manipulation in Rust and utilizing a native Go Operator for event-driven memory synchronization, Silex achieves sub-millisecond routing latency with near-zero memory utilization.

## Architecture

Silex splits the responsibilities of traditional ingress controllers into three highly optimized, decoupled components:


```

[External Traffic] ──(Port 80)──> [Silex Ingress (Rust)] ──> [Target Pod IP]
▲
(Internal HTTP POST)
│
[K8s API Server] ──(Watch Events)──> [Silex Operator (Go)]

```

*   **Silex Ingress (Rust Engine):** Sitting directly on the host network (`Port 80`), this kernel-speed reverse proxy operates directly on raw TCP byte streams. It parses only the essential bytes needed to extract the `Host` header and path variables, executing memory-synchronized `DashMap` lookups without HTTP context allocation overhead or connection-breaking reloads.
*   **Silex Operator (Go Controller):** A native Kubernetes controller that maintains a highly efficient `SharedInformerFactory` loop watching `Ingress` and `Endpoints` resources. Upon any cluster mutations, it extracts live backend Pod IPs and pushes updates via atomic internal HTTP payloads to the Rust Ingress runtime memory table.
*   **Silex CLI (Go Injector):** A zero-dependency administration binary utilizing the native standard library to directly build and inject valid in-memory API objects (`Deployments`, `Services`, `Ingresses`) to the K8s API server, bypassing disk-bound YAML serialization.

## Quick Start

### Prerequisites
* Kubernetes Cluster (e.g., K3s, Minikube, or Managed K8s)
* Proper RBAC permissions to apply cluster-scoped configurations

### 1. Deploy the Control Plane
Apply the unified manifest to initialize the `silex-system` namespace, configure the necessary RBAC roles, and spin up both the Ingress engine and the internal Go control loops:

```bash
kubectl apply -f [https://raw.githubusercontent.com/your-username/silex/main/deploy/silex-all-in-one.yaml](https://raw.githubusercontent.com/your-username/silex/main/deploy/silex-all-in-one.yaml)

```

### 2. Verify Control Plane Status

Ensure all system pods are up and running properly:

```bash
kubectl get pods -n silex-system

```

## CLI Usage

Silex bypasses declarative local file templates and enables programmatic cluster interaction via the binary interface.

### Installation

Compile and drop the static binary into your systems global path execution tree:

```bash
cd silex-cli
go build -o silex main.go
sudo mv silex /usr/local/bin/

```

### Injecting Deployments

To instantly deploy a microservice, map its target execution port, and automatically instruct the control plane to populate the routing topology, execute:

```bash
silex deploy <app-name> --image=<image-url> --port=<target-port>

```

#### Example:

```bash
silex deploy production-frontend --image=nginx:alpine --port=80

```

**Output:**

```text
production-frontend.silex.local

```

The app is immediately exposed through the host layer without triggering a single router service restart. Map the emitted virtual host string to your cluster gateway ingress interface to test upstream payload routing.

```

---
