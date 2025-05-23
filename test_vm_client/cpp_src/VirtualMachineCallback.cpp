/* 
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
 * SPDX-License-Identifier: BSD-3-Clause-Clear
 */

#include "VirtualMachineCallback.h"
#include <log/log.h>
#include <stdio.h>


using namespace aidl::vendor::qti::AvfQcvmManager;


::ndk::ScopedAStatus VirtualMachineCallback::onStarting() {
    ALOGI("test_vm_client: Received Callback, VM is starting");
    std::cout << "test_vm_client: Received Callback, VM is starting" << std::endl;
    return ndk::ScopedAStatus::ok();
}


::ndk::ScopedAStatus VirtualMachineCallback::onUserspaceReady() {
    ALOGI("test_vm_client: Received Callback, VM Usersapce is Ready");
    std::cout << "test_vm_client: Received Callback, VM Usersapce is Ready" << std::endl;
    return ndk::ScopedAStatus::ok();
}

::ndk::ScopedAStatus VirtualMachineCallback::onShutdownInitiated() {
    ALOGI("test_vm_client: Received Callback, VM's Shutdown has started");
    std::cout << "test_vm_client: Received Callback, VM's Shutdown has started" << std::endl;
    return ndk::ScopedAStatus::ok();
}


::ndk::ScopedAStatus VirtualMachineCallback::onCrashed() {
    ALOGI("test_vm_client: Received Callback, VM has Crashed");
    std::cout << "test_vm_client: Received Callback, VM has Crashed" << std::endl;
    return ndk::ScopedAStatus::ok();

}


::ndk::ScopedAStatus VirtualMachineCallback::onStopped() {
    ALOGI("test_vm_client: Received Callback, VM has Stopped");
    std::cout << "test_vm_client: Received Callback, VM has Stopped" << std::endl;
    return ndk::ScopedAStatus::ok();
}

::ndk::ScopedAStatus VirtualMachineCallback::onError(VirtualMachineError in_error) {
    ALOGI("test_vm_client: Received Callback, VM has an error");
    std::cout << "test_vm_client: Received Callback, VM has an error" << std::endl;
    return ndk::ScopedAStatus::ok();
}