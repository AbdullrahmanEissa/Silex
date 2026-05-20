# ⚡ Silex Ingress
**A Next-Generation, Zero-Hop, High-Throughput Kubernetes Ingress Controller.**

Silex is a highly opinionated, ultra-fast Ingress Controller built from the ground up for modern Cloud-Native environments. By decoupling the Control Plane (written in Go) from the Data Plane (written in Rust), Silex achieves extreme raw throughput while maintaining zero-downtime dynamic routing.

## 🚀 The Core Problem & The Silex Solution
Traditional Reverse Proxies (like Nginx) suffer from a fundamental architectural flaw in Kubernetes: **The Reload Penalty**. Every time a new Ingress is added, the proxy must reload its configuration, causing temporary CPU spikes and potential connection drops.

**Silex solves this via:**
1. **Dynamic In-Memory State:** $O(1)$ route injection without *ever* reloading the proxy process.
2. **Zero-Allocation Parsing:** Network streams are processed directly via pointer references, eliminating heap overhead.
3. **Zero-Hop Routing:** The Go Operator bypasses `kube-proxy` entirely by watching modern K8s `EndpointSlices` instead of `Services`, routing traffic directly to the exact healthy Pod IPs.

---

## 📊 Benchmark: Silex vs. Nginx Proxy
We subjected both Silex and Nginx to a brutal stress test on the exact same hardware, routing traffic to the same fast backend. 

**Test Conditions:** 
Tool: `wrk` | Threads: 12 | Connections: 400 | Duration: 30s | Environment: Local Linux Machine.

| Metric | Silex Ingress (Rust Data Plane) | Nginx Reverse Proxy | Performance Gain |
| :--- | :--- | :--- | :--- |
| **Requests/Sec** | **81,682 req/sec** 🚀 | 13,387 req/sec 🐢 | **~6x Faster** |
| **Average Latency** | **5.11 ms** | 34.00 ms | **~85% Reduction** |
| **Data Transfer** | **20.95 MB/sec** | 3.43 MB/sec | **Massive Bandwidth Increase** |

---

## ⚔️ Reproducing the Benchmark (Step-by-Step)
For reviewers, content creators, and engineers: you can easily reproduce this throughput massacre on your own machine. Execute the following commands one by one.

### Step 1: Install Prerequisites
Install the load testing tool (`wrk`) and `nginx`.
```bash
sudo apt update

```

```bash
sudo apt install wrk nginx -y

```

### Step 2: Setup the Fast Backend (Port 8080)

Create a dummy file for the backend to serve:

```bash
echo "Hello from Fast Backend" | sudo tee /var/www/html/backend.html

```

Configure Nginx to act as the Backend on port 8080:

```bash
sudo tee /etc/nginx/sites-available/backend << 'EOF'
server {
    listen 8080;
    location / {
        root /var/www/html;
        try_files /backend.html =404;
    }
}
EOF

```

### Step 3: Setup Nginx as a Reverse Proxy (Port 8081)

Configure Nginx to proxy traffic to the backend, optimized for maximum speed:

```bash
sudo tee /etc/nginx/sites-available/nginx-proxy << 'EOF'
server {
    listen 8081;
    server_name bench.local;
    location / {
        proxy_pass [http://127.0.0.1:8080](http://127.0.0.1:8080);
        proxy_set_header Host $host;
        proxy_http_version 1.1;
        proxy_set_header Connection "";
    }
}
EOF

```

Enable the sites and restart Nginx:

```bash
sudo ln -sf /etc/nginx/sites-available/backend /etc/nginx/sites-enabled/

```

```bash
sudo ln -sf /etc/nginx/sites-available/nginx-proxy /etc/nginx/sites-enabled/

```

```bash
sudo rm -f /etc/nginx/sites-enabled/default

```

```bash
sudo systemctl restart nginx

```

### Step 4: Build and Start Silex Ingress (Port 80)

Clear port 80 just in case, then compile and run the Rust Data Plane:

```bash
sudo fuser -k 80/tcp 9090/tcp

```

```bash
cd silex-ingress

```

```bash
cargo build --release

```

```bash
sudo ./target/release/silex-ingress

```

*(⚠️ IMPORTANT: Leave this terminal open and running. Open a NEW terminal for the next steps).*

### Step 5: Inject Route & Fire the Benchmark!

In your **NEW** terminal, inject the route into Silex's memory (simulating the Go Operator):

```bash
curl -X POST [http://127.0.0.1:9090](http://127.0.0.1:9090) -d '{"host": "bench.local", "ip": "127.0.0.1:8080"}'

```

**Run the Nginx Benchmark:**

```bash
wrk -t12 -c400 -d30s -H "Host: bench.local" [http://127.0.0.1:8081/](http://127.0.0.1:8081/)

```

**Run the Silex Benchmark:**

```bash
wrk -t12 -c400 -d30s -H "Host: bench.local" [http://127.0.0.1:80/](http://127.0.0.1:80/)

```

---

## 🛠️ Running the Full Kubernetes Operator

To run the complete system inside a Kubernetes cluster (Control Plane + Data Plane):

1. Point the Operator to your K8s cluster:

```bash
cd silex-operator
go run main.go

```

2. The Go Operator will automatically watch for `Ingress` and `EndpointSlices` events, bypassing `kube-proxy`, and instantly push state changes to the Rust Data Plane on port 9090.

---

## 📄 License

Check the `LICENSE` file for details.
*Built with passion by a DevOps / System Engineer tired of proxy reloads.*

```
