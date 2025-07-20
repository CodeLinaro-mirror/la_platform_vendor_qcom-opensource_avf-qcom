/*
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
 * SPDX-License-Identifier: BSD-3-Clause-Clear
 */

#include <android/binder_manager.h>
#include <android/binder_process.h>
#include "VirtualMachineCallback.h"
#include <aidl/vendor/qti/AvfQcvmManager/IAvfQcvmManager.h>
#include <aidl/vendor/qti/AvfQcvmManager/IVirtualMachine.h>
#include <log/log.h>
#include <thread>
#include <chrono>
#include <stdio.h>

using namespace aidl::vendor::qti::AvfQcvmManager;

int main() {

    std::string name =
        std::string(IAvfQcvmManager::descriptor)
        + "/default";

    /* Get a Binder Handle to AvfQcvmManager */
    ndk::SpAIBinder sysBinder = ndk::SpAIBinder(AServiceManager_waitForService(name.c_str()));

    /* Create  an IAvfQcvmManager Pointer*/
     std::shared_ptr<IAvfQcvmManager> server = IAvfQcvmManager::fromBinder(sysBinder);

    std::shared_ptr<IVirtualMachine> vm = nullptr;

    server->getVm("trustedvm", &vm);

    std::shared_ptr<IVirtualMachineCallback> vm_callback = ndk::SharedRefBase::make<VirtualMachineCallback>();
    std::cout << "Starting TUI VM" << std::endl;
    vm->start(vm_callback);
    std::cout << "Sleeping for 5 seconds..." << std::endl;
    std::this_thread::sleep_for(std::chrono::seconds(5));
    std::cout << "Awake now!" << std::endl;
    ALOGI("test_vm_client: Requesting the VM to shutdown");
    std::cout << "Calling to request stop" << std::endl;
    vm->request_stop(vm_callback);

    ABinderProcess_joinThreadPool();

    /* This line should not be reached */
    return EXIT_FAILURE;
}