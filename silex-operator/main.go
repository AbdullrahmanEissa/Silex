package main

import (
	"context"
	"fmt"
	"os"
	"os/signal"
	"syscall"

	corev1 "k8s.io/api/core/v1"
	networkingv1 "k8s.io/api/networking/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/client-go/informers"
	"k8s.io/client-go/kubernetes"
	"k8s.io/client-go/tools/cache"

	"silex-operator/pkg/k8s"
	"silex-operator/pkg/sync"
	"silex-operator/pkg/types"
)

func main() {
	client, err := k8s.NewClient()
	if err != nil {
		os.Exit(1)
	}

	factory := informers.NewSharedInformerFactory(client, 0)

	ingInformer := factory.Networking().V1().Ingresses().Informer()
	_, _ = ingInformer.AddEventHandler(cache.ResourceEventHandlerFuncs{
		AddFunc:    func(obj interface{}) { processIngress(client, obj.(*networkingv1.Ingress)) },
		UpdateFunc: func(old, new interface{}) { processIngress(client, new.(*networkingv1.Ingress)) },
		DeleteFunc: func(obj interface{}) { processIngress(client, obj.(*networkingv1.Ingress)) },
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

	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, syscall.SIGINT, syscall.SIGTERM)
	<-sigCh
	close(stopCh)
}

func processIngress(client *kubernetes.Clientset, ing *networkingv1.Ingress) {
	for _, rule := range ing.Spec.Rules {
		if rule.Host == "" || rule.HTTP == nil {
			continue
		}
		for _, path := range rule.HTTP.Paths {
			if path.Backend.Service != nil {
				svcName := path.Backend.Service.Name
				ep, err := client.CoreV1().Endpoints(ing.Namespace).Get(context.Background(), svcName, metav1.GetOptions{})
				if err != nil {
					continue
				}
				port := path.Backend.Service.Port.Number
				sendUpdatesFromEndpoints(rule.Host, port, ep)
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
				if path.Backend.Service != nil && path.Backend.Service.Name == ep.Name {
					port := path.Backend.Service.Port.Number
					sendUpdatesFromEndpoints(rule.Host, port, ep)
				}
			}
		}
	}
}

func sendUpdatesFromEndpoints(host string, port int32, ep *corev1.Endpoints) {
	for _, subset := range ep.Subsets {
		for _, addr := range subset.Addresses {
			target := fmt.Sprintf("%s:%d", addr.IP, port)
			payload := types.RoutePayload{
				Host: host,
				IP:   target,
			}
			_ = sync.SendRouteUpdate(payload)
		}
	}
}
