/*
Copyright 2025.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

package e2e

import (
    "os/exec"
    "time"

    . "github.com/onsi/ginkgo/v2"
    . "github.com/onsi/gomega"

    "argon.github.io/ingress/test/utils"
)

var _ = Describe("Minimal", Ordered, func() {
    SetDefaultEventuallyTimeout(2 * time.Minute)
    SetDefaultEventuallyPollingInterval(1 * time.Second)

    It("cluster is reachable", func() {
        cmd := exec.Command("kubectl", "get", "nodes", "-o", "name")
        out, err := utils.Run(cmd)
        Expect(err).NotTo(HaveOccurred())
        Expect(out).NotTo(BeEmpty())
    })

    It("can create namespace and run a pod", func() {
        const ns = "e2e-smoke"
        By("creating namespace")
        cmd := exec.Command("kubectl", "create", "ns", ns)
        _, err := utils.Run(cmd)
        Expect(err).NotTo(HaveOccurred())
        defer func() { _, _ = utils.Run(exec.Command("kubectl", "delete", "ns", ns, "--wait=true")) }()

        By("running echo pod")
        cmd = exec.Command("kubectl", "run", "echo", "--image=busybox:1.36", "--restart=Never", "-n", ns, "--",
            "sh", "-c", "echo ok && sleep 1")
        _, err = utils.Run(cmd)
        Expect(err).NotTo(HaveOccurred())

        By("waiting pod to succeed")
        Eventually(func(g Gomega) {
            cmd := exec.Command("kubectl", "get", "pod", "echo", "-n", ns, "-o", "jsonpath={.status.phase}")
            out, err := utils.Run(cmd)
            g.Expect(err).NotTo(HaveOccurred())
            g.Expect(out).To(Equal("Succeeded"))
        }, 2*time.Minute, 2*time.Second).Should(Succeed())

        By("verifying logs")
        cmd = exec.Command("kubectl", "logs", "echo", "-n", ns)
        out, err := utils.Run(cmd)
        Expect(err).NotTo(HaveOccurred())
        Expect(out).To(ContainSubstring("ok"))
    })
})

