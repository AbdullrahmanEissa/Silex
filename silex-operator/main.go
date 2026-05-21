package main

import (
	"context"
	"encoding/base64"
	"fmt"
	"os"
	"os/signal"
	"syscall"

	discoveryv1 "k8s.io/api/discovery/v1"
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

	sliceInformer := factory.Discovery().V1().EndpointSlices().Informer()
	_, _ = sliceInformer.AddEventHandler(cache.ResourceEventHandlerFuncs{
		AddFunc:    func(obj interface{}) { processEndpointSlice(client, obj.(*discoveryv1.EndpointSlice)) },
		UpdateFunc: func(old, new interface{}) { processEndpointSlice(client, new.(*discoveryv1.EndpointSlice)) },
		DeleteFunc: func(obj interface{}) { processEndpointSlice(client, obj.(*discoveryv1.EndpointSlice)) },
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
	for _, tls := range ing.Spec.TLS {
		secret, err := client.CoreV1().Secrets(ing.Namespace).Get(context.Background(), tls.SecretName, metav1.GetOptions{})
		if err == nil {
			certData := base64.StdEncoding.EncodeToString(secret.Data["tls.crt"])
			keyData := base64.StdEncoding.EncodeToString(secret.Data["tls.key"])
			for _, host := range tls.Hosts {
				_ = sync.SendTLSUpdate(types.TLSPayload{
					Host: host,
					Cert: certData,
					Key:  keyData,
				})
			}
		}
	}

	for _, rule := range ing.Spec.Rules {
		if rule.Host == "" || rule.HTTP == nil {
			continue
		}
		for _, path := range rule.HTTP.Paths {
			if path.Backend.Service != nil {
				svcName := path.Backend.Service.Name
				labelSelector := fmt.Sprintf("kubernetes.io/service-name=%s", svcName)
				slices, err := client.DiscoveryV1().EndpointSlices(ing.Namespace).List(context.Background(), metav1.ListOptions{LabelSelector: labelSelector})
				if err != nil {
					continue
				}
				port := path.Backend.Service.Port.Number
				for _, slice := range slices.Items {
					sendUpdatesFromSlice(rule.Host, port, &slice)
				}
			}
		}
	}
}

func processEndpointSlice(client *kubernetes.Clientset, slice *discoveryv1.EndpointSlice) {
	svcName, ok := slice.Labels["kubernetes.io/service-name"]
	if !ok {
		return
	}

	ings, err := client.NetworkingV1().Ingresses(slice.Namespace).List(context.Background(), metav1.ListOptions{})
	if err != nil {
		return
	}
	for _, ing := range ings.Items {
		for _, rule := range ing.Spec.Rules {
			if rule.HTTP == nil {
				continue
			}
			for _, path := range rule.HTTP.Paths {
				if path.Backend.Service != nil && path.Backend.Service.Name == svcName {
					port := path.Backend.Service.Port.Number
					sendUpdatesFromSlice(rule.Host, port, slice)
				}
			}
		}
	}
}

func sendUpdatesFromSlice(host string, port int32, slice *discoveryv1.EndpointSlice) {
	for _, ep := range slice.Endpoints {
		if ep.Conditions.Ready != nil && *ep.Conditions.Ready {
			for _, ip := range ep.Addresses {
				target := fmt.Sprintf("%s:%d", ip, port)
				payload := types.RoutePayload{
					Host: host,
					IP:   target,
				}
				_ = sync.SendRouteUpdate(payload)
			}
		}
	}
}
