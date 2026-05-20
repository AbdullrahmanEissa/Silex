# ⚡ Silex Ingress
**A Next-Generation, Zero-Hop, High-Throughput Kubernetes Ingress Controller.**

Silex is a highly opinionated, ultra-fast Ingress Controller built from the ground up for modern Cloud-Native environments. By decoupling the Control Plane (written in Go) from the Data Plane (written in Rust), Silex achieves extreme raw throughput while maintaining zero-downtime dynamic routing.

## 🚀 The Core Problem & The Silex Solution
Traditional Reverse Proxies (like Nginx) suffer from a fundamental architectural flaw in Kubernetes: **The Reload Penalty**. Every time a new Ingress is added, the proxy must reload its configuration, causing temporary CPU spikes and potential connection drops. Furthermore, traditional proxies rely heavily on heap memory allocation for parsing requests.

**Silex solves this via:**
1. **Dynamic In-Memory State:** $O(1)$ route injection without *ever* reloading the proxy process.
2. **Zero-Allocation Parsing:** Network streams are processed directly via pointer references, eliminating Garbage Collection pauses and heap overhead.
3. **Zero-Hop Routing:** The Go Operator bypasses `kube-proxy` entirely by watching `Endpoints` instead of `Services`, routing traffic directly to the exact Pod IPs.

---

## 📊 Benchmark: Silex vs. Nginx Proxy
We subjected both Silex and Nginx to a brutal stress test on the exact same hardware, routing traffic to the same fast backend. 

**Test Conditions:** 
Tool: `wrk` | Threads: 12 | Connections: 400 | Duration: 30s | Environment: Local Kubernetes Cluster.

| Metric | Silex Ingress (Rust Data Plane) | Nginx Reverse Proxy | Performance Gain |
| :--- | :--- | :--- | :--- |
| **Requests/Sec** | **81,682 req/sec** 🚀 | 13,387 req/sec 🐢 | **~6x Faster** |
| **Average Latency** | **5.11 ms** | 34.00 ms | **~85% Reduction** |
| **Data Transfer** | **20.95 MB/sec** | 3.43 MB/sec | **Massive Bandwidth Increase** |

*Note: Silex maintained stable memory consumption throughout the test due to its lock-free data structures and strict Rust memory safety.*

---

## 🧠 High-Level Architecture (How it works)
Silex is composed of two primary micro-components tailored for strict separation of concerns:

### 1. The Brain: `silex-operator` (Golang)
- Acts as a custom Kubernetes Controller using `client-go`.
- Listens to `Ingress` and `Endpoints` events in real-time.
- Resolves the exact topology of the backend pods.
- Pushes state changes incrementally to the Data Plane via a lightweight internal Sync API.

### 2. The Muscle: `silex-ingress` (Rust)
- A bare-metal speed TCP/HTTP router.
- Uses advanced Lock-Free Concurrency models to ensure no thread blocks another during high traffic.
- Maintains routing tables purely in RAM.
- Features custom-built Circuit Breaking and Health Probing to instantly isolate failing nodes without halting traffic.

---

## 🛠️ Quick Start (Local Development)

### 1. Start the Data Plane (Rust)
```bash
cd silex-ingress
cargo build --release
sudo ./target/release/silex-ingress

```

*(Silex listens on Port 80 for traffic, and Port 9090 for Operator sync).*

### 2. Start the Control Plane (Go)

In a separate terminal, point the operator to your Kubernetes cluster:

```bash
cd silex-operator
go run main.go

```

Deploy any standard Kubernetes `Ingress` resource, and watch Silex route traffic instantly without a single reload.

---

## 📄 License

This project is proprietary / Open Source (Check LICENSE file). Architecture designed for extreme performance edge cases.

*Built with passion by a DevOps / System Engineer tired of proxy reloads.*
