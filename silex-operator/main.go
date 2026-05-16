package main

import (
	"bytes"
	"context"
	"net/http"
	"path/filepath"
	"time"

	corev1 "k8s.io/api/core/v1"
	netv1 "k8s.io/api/networking/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/client-go/informers"
	"k8s.io/client-go/kubernetes"
	"k8s.io/client-go/rest"
	"k8s.io/client-go/tools/cache"
	"k8s.io/client-go/tools/clientcmd"
	"k8s.io/client-go/util/homedir"
)

var httpClient = &http.Client{
	Timeout: 2 * time.Second,
	Transport: &http.Transport{
		MaxIdleConns:        100,
		MaxIdleConnsPerHost: 100,
		IdleConnTimeout:     90 * time.Second,
	},
}

func createK8sClient() *kubernetes.Clientset {
	config, err := rest.InClusterConfig()
	if err != nil {
		kubeconfig := filepath.Join(homedir.HomeDir(), ".kube", "config")
		config, err = clientcmd.BuildConfigFromFlags("", kubeconfig)
		if err != nil {
			panic(err)
		}
	}

	clientset, err := kubernetes.NewForConfig(config)
	if err != nil {
		panic(err)
	}

	return clientset
}

func sendUpdate(host, ip string) {
	payload := []byte(`{"host":"` + host + `","ip":"` + ip + `"}`)
	req, err := http.NewRequest("POST", "http://127.0.0.1:9090/update", bytes.NewBuffer(payload))
	if err != nil {
		return
	}
	resp, err := httpClient.Do(req)
	if err == nil {
		resp.Body.Close()
	}
}

func processIngress(client *kubernetes.Clientset, ing *netv1.Ingress) {
	for _, rule := range ing.Spec.Rules {
		if rule.Host == "" || rule.HTTP == nil {
			continue
		}
		for _, path := range rule.HTTP.Paths {
			svcName := path.Backend.Service.Name
			if svcName == "" {
				continue
			}
			ep, err := client.CoreV1().Endpoints(ing.Namespace).Get(context.Background(), svcName, metav1.GetOptions{})
			if err != nil {
				continue
			}
			for _, subset := range ep.Subsets {
				for _, addr := range subset.Addresses {
					sendUpdate(rule.Host, addr.IP)
				}
			}
		}
	}
}

func processEndpoints(client *kubernetes.Clientset, ep *corev1.Endpoints) {
	ings, err := client.NetworkingV1().Ingresses(ep.Namespace).List(context.Background(), metav1.ListOptions{})
	if err != nil {
		return
	}
	for _, ing := range ings.Items {
		for _, rule := range ing.Spec.Rules {
			if rule.HTTP == nil {
				continue
			}
			for _, path := range rule.HTTP.Paths {
				if path.Backend.Service.Name == ep.Name {
					for _, subset := range ep.Subsets {
						for _, addr := range subset.Addresses {
							sendUpdate(rule.Host, addr.IP)
						}
					}
				}
			}
		}
	}
}

func main() {
	client := createK8sClient()
	factory := informers.NewSharedInformerFactory(client, 0)

	ingInformer := factory.Networking().V1().Ingresses().Informer()
	_, _ = ingInformer.AddEventHandler(cache.ResourceEventHandlerFuncs{
		AddFunc:    func(obj interface{}) { processIngress(client, obj.(*netv1.Ingress)) },
		UpdateFunc: func(old, new interface{}) { processIngress(client, new.(*netv1.Ingress)) },
		DeleteFunc: func(obj interface{}) { processIngress(client, obj.(*netv1.Ingress)) },
	})

	epInformer := factory.Core().V1().Endpoints().Informer()
	_, _ = epInformer.AddEventHandler(cache.ResourceEventHandlerFuncs{
		AddFunc:    func(obj interface{}) { processEndpoints(client, obj.(*corev1.Endpoints)) },
		UpdateFunc: func(old, new interface{}) { processEndpoints(client, new.(*corev1.Endpoints)) },
		DeleteFunc: func(obj interface{}) { processEndpoints(client, obj.(*corev1.Endpoints)) },
	})

	stopCh := make(chan struct{})
	factory.Start(stopCh)
	factory.WaitForCacheSync(stopCh)
	<-stopCh
}
