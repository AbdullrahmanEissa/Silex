package main

import (
	"context"
	"flag"
	"fmt"
	"os"
	"path/filepath"

	appsv1 "k8s.io/api/apps/v1"
	corev1 "k8s.io/api/core/v1"
	netv1 "k8s.io/api/networking/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/util/intstr"
	"k8s.io/client-go/kubernetes"
	"k8s.io/client-go/tools/clientcmd"
	"k8s.io/client-go/util/homedir"
)

func main() {
	if len(os.Args) < 3 || os.Args[1] != "deploy" {
		os.Exit(1)
	}

	appName := os.Args[2]

	deployCmd := flag.NewFlagSet("deploy", flag.ExitOnError)
	image := deployCmd.String("image", "", "")
	port := deployCmd.Int("port", 0, "")

	if err := deployCmd.Parse(os.Args[3:]); err != nil || *image == "" || *port == 0 {
		os.Exit(1)
	}

	kubeconfig := filepath.Join(homedir.HomeDir(), ".kube", "config")
	config, err := clientcmd.BuildConfigFromFlags("", kubeconfig)
	if err != nil {
		os.Exit(1)
	}

	client, err := kubernetes.NewForConfig(config)
	if err != nil {
		os.Exit(1)
	}

	ctx := context.Background()
	ns := "default"
	replicas := int32(1)
	targetPort := int32(*port)

	deploy := &appsv1.Deployment{
		ObjectMeta: metav1.ObjectMeta{Name: appName},
		Spec: appsv1.DeploymentSpec{
			Replicas: &replicas,
			Selector: &metav1.LabelSelector{MatchLabels: map[string]string{"app": appName}},
			Template: corev1.PodTemplateSpec{
				ObjectMeta: metav1.ObjectMeta{Labels: map[string]string{"app": appName}},
				Spec: corev1.PodSpec{
					Containers: []corev1.Container{
						{
							Name:  appName,
							Image: *image,
							Ports: []corev1.ContainerPort{{ContainerPort: targetPort}},
						},
					},
				},
			},
		},
	}

	_, err = client.AppsV1().Deployments(ns).Create(ctx, deploy, metav1.CreateOptions{})
	if err != nil {
		os.Exit(1)
	}

	svc := &corev1.Service{
		ObjectMeta: metav1.ObjectMeta{Name: appName},
		Spec: corev1.ServiceSpec{
			Selector: map[string]string{"app": appName},
			Ports: []corev1.ServicePort{
				{
					Port:       80,
					TargetPort: intstr.FromInt(int(*port)),
				},
			},
			Type: corev1.ServiceTypeClusterIP,
		},
	}

	_, err = client.CoreV1().Services(ns).Create(ctx, svc, metav1.CreateOptions{})
	if err != nil {
		os.Exit(1)
	}

	pathType := netv1.PathTypePrefix
	ing := &netv1.Ingress{
		ObjectMeta: metav1.ObjectMeta{Name: appName},
		Spec: netv1.IngressSpec{
			Rules: []netv1.IngressRule{
				{
					Host: fmt.Sprintf("%s.silex.local", appName),
					IngressRuleValue: netv1.IngressRuleValue{
						HTTP: &netv1.HTTPIngressRuleValue{
							Paths: []netv1.HTTPIngressPath{
								{
									Path:     "/",
									PathType: &pathType,
									Backend: netv1.IngressBackend{
										Service: &netv1.IngressServiceBackend{
											Name: appName,
											Port: netv1.ServiceBackendPort{Number: 80},
										},
									},
								},
							},
						},
					},
				},
			},
		},
	}

	_, err = client.NetworkingV1().Ingresses(ns).Create(ctx, ing, metav1.CreateOptions{})
	if err != nil {
		os.Exit(1)
	}

	fmt.Printf("%s.silex.local\n", appName)
}
