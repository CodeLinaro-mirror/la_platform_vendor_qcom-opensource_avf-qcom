/*
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
 * SPDX-License-Identifier: BSD-3-Clause-Clear
 */

#include "VirtualMachineCallback.h"
#include <log/log.h>
#include <stdio.h>


using namespace aidl::vendor::qti::AvfQcvmManager;


VirtualMachineCallback::VirtualMachineCallback(std::shared_ptr<std::atomic<VmState>> state)
    : vm_state_(state) {}

::ndk::ScopedAStatus VirtualMachineCallback::onStarting() {
    *vm_state_ = VmState::Started;
    ALOGI("avf_qcvm_test_client: Received Callback, VM is starting");
    std::cout << "avf_qcvm_test_client: Received Callback, VM is starting" << std::endl;
    return ndk::ScopedAStatus::ok();
}


::ndk::ScopedAStatus VirtualMachineCallback::onUserspaceReady() {
    *vm_state_ = VmState::UserspaceReady;
    ALOGI("avf_qcvm_test_client: Received Callback, VM Usersapce is Ready");
    std::cout << "avf_qcvm_test_client: Received Callback, VM Usersapce is Ready" << std::endl;
    return ndk::ScopedAStatus::ok();
}

::ndk::ScopedAStatus VirtualMachineCallback::onShutdownInitiated() {
    *vm_state_ = VmState::ShutdownInitiated;
    ALOGI("avf_qcvm_test_client: Received Callback, VM's Shutdown has started");
    std::cout << "avf_qcvm_test_client: Received Callback, VM's Shutdown has started" << std::endl;
    return ndk::ScopedAStatus::ok();
}


::ndk::ScopedAStatus VirtualMachineCallback::onCrashed() {
    *vm_state_ = VmState::Crashed;
    ALOGI("avf_qcvm_test_client: Received Callback, VM has Crashed");
    std::cout << "avf_qcvm_test_client: Received Callback, VM has Crashed" << std::endl;
    return ndk::ScopedAStatus::ok();

}


::ndk::ScopedAStatus VirtualMachineCallback::onStopped() {
    *vm_state_ = VmState::Stopped;
    ALOGI("avf_qcvm_test_client: Received Callback, VM has Stopped");
    std::cout << "avf_qcvm_test_client: Received Callback, VM has Stopped" << std::endl;
    return ndk::ScopedAStatus::ok();
}

::ndk::ScopedAStatus VirtualMachineCallback::onError(VirtualMachineError in_error) {
    ALOGI("avf_qcvm_test_client: Received Callback, VM has an error");
    std::cout << "avf_qcvm_test_client: Received Callback, VM has an error" << std::endl;
    return ndk::ScopedAStatus::ok();
}